package io.github.cleanrules;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.internal.Build;
import io.github.cleanrules.internal.DataReader;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.PathTree;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import io.github.cleanrules.internal.Values;
import java.lang.foreign.MemorySegment;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Objects;
import java.util.function.Function;

/**
 * A node of a condition tree ({@code spec/ast.md}): a condition such as
 * {@code customer.age >= 18}, or a value such as {@code order.total * 0.9}.
 * Conditions are data, never Java code, so a rule can be explained, written
 * and audited.
 *
 * <p>Build nodes with {@link Conditions} and the methods here:
 *
 * <pre>{@code
 * import static io.github.cleanrules.Conditions.*;
 *
 * Expr adult = field("customer", "age").ge(18).and(field("country").isIn("ES", "PT"));
 * }</pre>
 *
 * <p>Java values next to a node become constants ({@code ge(18)}): {@code null}
 * and {@link java.util.Optional}, booleans, numbers ({@link java.math.BigDecimal}
 * stays exact), strings, enum constants (their name) and {@code java.time}
 * dates and datetimes, as {@code spec/data-model.md} reads them. A NaN, an
 * integer beyond 64 bits, a decimal beyond 38 digits, a datetime finer than a
 * microsecond and anything that is not a scalar are an
 * {@link ExpressionException}. The core builds every node and applies the
 * construction rules, so a node that breaks one (a condition compared, a
 * {@code null} in a comparison, an invalid pattern) is an
 * {@link ExpressionException} with the core's message.
 *
 * <p>Nodes are immutable and may be shared between threads. {@link #equals}
 * is structural identity ({@link #sameAs}): the same shape, tags and
 * constants, constants compared by type and value, so {@code 1}, {@code 1.0}
 * and {@code true} differ while {@code 0.0} and {@code -0.0} do not; and
 * {@link #hashCode()} agrees with it. {@link #toString()} is the node's text in
 * the rule syntax ({@code spec/text.md}), as explanations write it.
 *
 * <p>A node holds native memory, which the cleaner frees once the node is
 * unreachable.
 */
public abstract sealed class Expr implements DocumentContent permits Field, Const, Expr.Composite {
    private final Handle handle;
    private volatile String text;
    private volatile int hash;
    private volatile boolean hashed;
    private volatile PathTree paths;

    Expr(MemorySegment pointer) {
        this.handle = Handle.own(this, pointer, Native::nodeFree, "Expr");
    }

    /** A node that is neither a field nor a constant. */
    static final class Composite extends Expr {
        Composite(MemorySegment pointer) {
            super(pointer);
        }
    }

    /** The node of an owned {@code cr_node *}, of the class of its tag. */
    static Expr wrap(MemorySegment pointer) {
        String tag = Native.nodeTag(pointer);
        if (tag == null) {
            throw new IllegalStateException("cleanrules gave no node and no error; please report it");
        }
        return switch (tag) {
            case "field" -> new Field(pointer);
            case "const" -> new Const(pointer);
            default -> new Composite(pointer);
        };
    }

    /** A call of a {@code cr_node_*} constructor. */
    @FunctionalInterface
    interface NodeCall {
        int call(MemorySegment out, MemorySegment error, Scope scope);
    }

    /** The node a constructor gives. */
    static Expr make(NodeCall call) {
        Native.ensureLoaded();
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(call.call(out, scope.error(), scope));
            return wrap(out.get(ADDRESS, 0));
        }
    }

    /** {@code value} if it is a node, else a constant. */
    static Expr lift(Object value) {
        return value instanceof Expr node ? node : Conditions.constant(value);
    }

    /** Call {@code call} with the native node, held for the call. */
    <T> T with(Function<MemorySegment, T> call) {
        MemorySegment pointer = handle.acquire();
        try {
            return call.apply(pointer);
        } finally {
            handle.release();
        }
    }

    /** Call {@code call} with the native nodes of {@code nodes}, held for the call. */
    static <T> T with(List<Expr> nodes, Function<MemorySegment[], T> call) {
        MemorySegment[] pointers = new MemorySegment[nodes.size()];
        int held = 0;
        try {
            for (; held < pointers.length; held++) {
                pointers[held] = nodes.get(held).handle.acquire();
            }
            return call.apply(pointers);
        } finally {
            for (int i = 0; i < held; i++) {
                nodes.get(i).handle.release();
            }
        }
    }

    private static Expr unary(Expr operand, UnaryCall call) {
        return operand.with(pointer -> make((out, error, scope) -> call.call(pointer, out, error)));
    }

    @FunctionalInterface
    private interface UnaryCall {
        int call(MemorySegment operand, MemorySegment out, MemorySegment error);
    }

    private static Expr binary(String op, Expr left, Object right, boolean arithmetic) {
        Expr other = lift(right);
        byte[] name = Text.name(op);
        return with(List.of(left, other), pointers -> make((out, error, scope) -> arithmetic
                ? Native.nodeArith(scope.bytes(name), name.length, pointers[0], pointers[1], out, error)
                : Native.nodeCompare(scope.bytes(name), name.length, pointers[0], pointers[1], out, error)));
    }

    static Expr connective(boolean and, List<Expr> operands) {
        for (Expr operand : operands) {
            Objects.requireNonNull(operand, "an operand is null");
        }
        return with(operands, pointers -> make((out, error, scope) -> {
            MemorySegment array = scope.pointers(pointers);
            return and
                    ? Native.nodeAnd(array, pointers.length, out, error)
                    : Native.nodeOr(array, pointers.length, out, error);
        }));
    }

    // ---- Introspection.

    /**
     * The node's tag, such as {@code gt} or {@code and} ({@code spec/ast.md},
     * <i>Catalogue</i>).
     *
     * @return the tag
     */
    public String tag() {
        return with(Native::nodeTag);
    }

    /**
     * Where the node may appear.
     *
     * @return {@link Kind#CONDITION}, {@link Kind#VALUE} or {@link Kind#ANY}
     */
    public Kind kind() {
        return Kind.fromJsonName(with(Native::nodeKind));
    }

    /**
     * The direct children, in order: a comparison's left and right, the
     * operands of {@code and}, {@code between}'s operand, low and high...
     *
     * @return the children, an unmodifiable list
     */
    public List<Expr> children() {
        return with(pointer -> {
            int count = Math.toIntExact(Native.nodeChildrenLen(pointer));
            List<Expr> children = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                long index = i;
                children.add(make((out, error, scope) -> Native.nodeChild(pointer, index, out, error)));
            }
            return Collections.unmodifiableList(children);
        });
    }

    /**
     * This node and all its descendants, parents first (pre-order).
     *
     * @return the nodes, an unmodifiable list
     */
    public List<Expr> walk() {
        List<Expr> nodes = new ArrayList<>();
        ArrayDeque<Expr> stack = new ArrayDeque<>();
        stack.push(this);
        while (!stack.isEmpty()) {
            Expr node = stack.pop();
            nodes.add(node);
            List<Expr> children = node.children();
            for (int i = children.size() - 1; i >= 0; i--) {
                stack.push(children.get(i));
            }
        }
        return Collections.unmodifiableList(nodes);
    }

    /**
     * The paths of the fields the node reads, in pre-order and each once. A
     * path may be the prefix of another.
     *
     * @return the paths: lists of {@link String} keys and {@link Integer} indexes
     */
    public List<List<Object>> fieldPaths() {
        return with(pointer -> Values.paths(Scope.value((out, error) -> Native.nodeFieldPaths(pointer, out, error))));
    }

    // ---- Evaluating one node.

    /**
     * Evaluate this condition on {@code data} and give its result tree
     * ({@code spec/ast.md}, <i>Evaluating one node</i>), under Boolean logic
     * and not strict. Data is read as {@link RuleSet#evaluate(Object)} reads
     * it: only the paths this node reads.
     *
     * @param data the input
     * @return the result tree, whose root holds the truth value
     * @throws ExpressionException if this is a value node, such as {@code a + 1}
     */
    public ResultTree evaluateTree(Object data) {
        return evaluateTree(data, "boolean", false);
    }

    /**
     * Evaluate this condition on {@code data} under a logic, strict or not,
     * and give its result tree.
     *
     * @param data the input
     * @param logic the logic, by name ({@code boolean})
     * @param strict whether a missing field or a type mismatch is an error instead of unknown
     * @return the result tree
     * @throws ExpressionException if this is a value node
     * @throws RegistryException for an unknown logic
     * @throws EvaluationException in strict mode, for a missing field or a type mismatch
     */
    public ResultTree evaluateTree(Object data, String logic, boolean strict) {
        byte[] name = Text.name(Objects.requireNonNull(logic, "logic"));
        return tree(data, (node, input, out, scope) -> Native.treeNewCondition(
                node,
                input,
                // An empty name is not NULL: the core refuses it.
                name.length == 0 ? scope.bytes(new byte[] {0}) : scope.bytes(name),
                name.length,
                strict,
                out,
                scope.error()));
    }

    /**
     * Evaluate this value node on {@code data} and give its result tree, not strict.
     *
     * @param data the input
     * @return the result tree, whose root holds the value
     * @throws ExpressionException if this is a condition, such as {@code a > 1}
     */
    public ResultTree evaluateValueTree(Object data) {
        return evaluateValueTree(data, false);
    }

    /**
     * Evaluate this value node on {@code data}, strict or not, and give its result tree.
     *
     * @param data the input
     * @param strict whether a missing field or a type mismatch is an error instead of null
     * @return the result tree
     * @throws ExpressionException if this is a condition
     * @throws EvaluationException in strict mode, for a missing field or a type mismatch
     */
    public ResultTree evaluateValueTree(Object data, boolean strict) {
        return tree(data, (node, input, out, scope) -> Native.treeNewValue(node, input, strict, out, scope.error()));
    }

    @FunctionalInterface
    private interface TreeCall {
        int call(MemorySegment node, MemorySegment input, MemorySegment out, Scope scope);
    }

    private ResultTree tree(Object data, TreeCall call) {
        PathTree read = paths;
        if (read == null) {
            read = PathTree.of(fieldPaths());
            paths = read;
        }
        PathTree along = read;
        return with(pointer -> {
            try (Scope scope = new Scope();
                    DataReader reader = new DataReader()) {
                MemorySegment input = reader.read(data, along);
                scope.onClose(() -> Native.valueFree(input));
                MemorySegment out = scope.pointer();
                scope.check(call.call(pointer, input, out, scope), reader.causes);
                return ResultTree.of(out.get(ADDRESS, 0), reader.causes.opaques);
            }
        });
    }

    /**
     * Whether {@code other} is structurally identical: the same shape, tags
     * and constants ({@code spec/ast.md}, <i>Structural identity</i>).
     *
     * @param other the other node
     * @return whether they are the same node
     */
    public boolean sameAs(Expr other) {
        if (other == null) {
            return false;
        }
        if (other == this) {
            return true;
        }
        return with(List.of(this, other), pointers -> Native.nodeSame(pointers[0], pointers[1]));
    }

    /**
     * The node in the JSON format ({@code spec/json-format.md},
     * <i>Conditions</i>): the node itself, not a document.
     *
     * @return the JSON text
     */
    public String toJson() {
        return with(pointer -> Scope.text((out, len, error) -> Native.nodeWrite(pointer, 1, out, len, error)));
    }

    /** The node's own data ({@code cr_node_detail}), as a Java value. */
    Object detail() {
        return with(pointer -> Values.take(Scope.value((out, error) -> Native.nodeDetail(pointer, out, error)), List.of()));
    }

    /**
     * The node's text in the rule syntax ({@code spec/text.md},
     * <i>Printing</i>), such as {@code customer.age >= 18 AND country IN ("ES", "PT")}.
     *
     * @return the text
     */
    @Override
    public String toString() {
        String written = text;
        if (written == null) {
            written = with(pointer -> Scope.text((out, len, error) -> Native.nodeWrite(pointer, 3, out, len, error)));
            text = written;
        }
        return written;
    }

    /**
     * Structural identity, as {@link #sameAs(Expr)}.
     *
     * @param other the other object
     * @return whether it is a node structurally identical to this one
     */
    @Override
    public boolean equals(Object other) {
        return other instanceof Expr node && sameAs(node);
    }

    /**
     * A hash that agrees with {@link #equals(Object)}, from the core's
     * ({@code cr_node_hash}): the same on every platform and in every run for
     * one version of the library, but it may change between versions, so
     * never store it.
     *
     * @return the hash
     */
    @Override
    public int hashCode() {
        if (!hashed) {
            long full = with(Native::nodeHash);
            hash = (int) (full ^ (full >>> 32));
            hashed = true;
        }
        return hash;
    }

    // ---- Comparisons.

    /**
     * {@code this == other}.
     *
     * @param other a node, or a Java constant
     * @return an {@code eq} node
     * @throws ExpressionException if either side is a condition or the null constant
     */
    public Expr eq(Object other) {
        return binary("eq", this, other, false);
    }

    /**
     * {@code this != other}.
     *
     * @param other a node, or a Java constant
     * @return an {@code ne} node
     * @throws ExpressionException if either side is a condition or the null constant
     */
    public Expr ne(Object other) {
        return binary("ne", this, other, false);
    }

    /**
     * {@code this < other}.
     *
     * @param other a node, or a Java constant
     * @return an {@code lt} node
     * @throws ExpressionException if either side is a condition or the null constant
     */
    public Expr lt(Object other) {
        return binary("lt", this, other, false);
    }

    /**
     * {@code this <= other}.
     *
     * @param other a node, or a Java constant
     * @return an {@code le} node
     * @throws ExpressionException if either side is a condition or the null constant
     */
    public Expr le(Object other) {
        return binary("le", this, other, false);
    }

    /**
     * {@code this > other}.
     *
     * @param other a node, or a Java constant
     * @return a {@code gt} node
     * @throws ExpressionException if either side is a condition or the null constant
     */
    public Expr gt(Object other) {
        return binary("gt", this, other, false);
    }

    /**
     * {@code this >= other}.
     *
     * @param other a node, or a Java constant
     * @return a {@code ge} node
     * @throws ExpressionException if either side is a condition or the null constant
     */
    public Expr ge(Object other) {
        return binary("ge", this, other, false);
    }

    // ---- Connectives.

    /**
     * This condition and the others: an {@code and}, flattened.
     *
     * @param others the other conditions
     * @return an {@code and} node
     * @throws ExpressionException if an operand is a value
     */
    public Expr and(Expr... others) {
        List<Expr> operands = new ArrayList<>(others.length + 1);
        operands.add(this);
        operands.addAll(List.of(others));
        return connective(true, operands);
    }

    /**
     * This condition or the others: an {@code or}, flattened.
     *
     * @param others the other conditions
     * @return an {@code or} node
     * @throws ExpressionException if an operand is a value
     */
    public Expr or(Expr... others) {
        List<Expr> operands = new ArrayList<>(others.length + 1);
        operands.add(this);
        operands.addAll(List.of(others));
        return connective(false, operands);
    }

    /**
     * The negation of this condition.
     *
     * @return a {@code not} node
     * @throws ExpressionException if this is a value
     */
    public Expr not() {
        return unary(this, Native::nodeNot);
    }

    // ---- Arithmetic.

    /**
     * {@code this + other}.
     *
     * @param other a node, or a Java number
     * @return an {@code add} node
     * @throws ExpressionException if an operand is a condition or a constant that is not a number
     */
    public Expr add(Object other) {
        return binary("add", this, other, true);
    }

    /**
     * {@code this - other}.
     *
     * @param other a node, or a Java number
     * @return a {@code sub} node
     * @throws ExpressionException if an operand is a condition or a constant that is not a number
     */
    public Expr sub(Object other) {
        return binary("sub", this, other, true);
    }

    /**
     * {@code this * other}.
     *
     * @param other a node, or a Java number
     * @return a {@code mul} node
     * @throws ExpressionException if an operand is a condition or a constant that is not a number
     */
    public Expr mul(Object other) {
        return binary("mul", this, other, true);
    }

    /**
     * {@code this / other}, always in float64.
     *
     * @param other a node, or a Java number
     * @return a {@code div} node
     * @throws ExpressionException if an operand is a condition or a constant that is not a number
     */
    public Expr div(Object other) {
        return binary("div", this, other, true);
    }

    /**
     * The negation of this number.
     *
     * @return a {@code neg} node
     * @throws ExpressionException if this is a condition or a constant that is not a number
     */
    public Expr neg() {
        return unary(this, Native::nodeNeg);
    }

    /**
     * The absolute value of this number.
     *
     * @return an {@code abs} node
     * @throws ExpressionException if this is a condition or a constant that is not a number
     */
    public Expr abs() {
        return unary(this, Native::nodeAbs);
    }

    // ---- Predicates.

    /**
     * Membership in constants, such as {@code field("country").isIn("ES", "PT")}.
     *
     * @param values non-null constants of one type group
     * @return an {@code in} node, whose values are kept sorted and without repeats
     * @throws ExpressionException if there are none, one is null, or they mix type groups
     */
    public Expr isIn(Object... values) {
        return isIn(java.util.Arrays.asList(values));
    }

    /**
     * Membership in a collection of constants (a list, a set...).
     *
     * @param values non-null constants of one type group
     * @return an {@code in} node, whose values are kept sorted and without repeats
     * @throws ExpressionException if there are none, one is null, or they mix type groups
     */
    public Expr isIn(Collection<?> values) {
        Objects.requireNonNull(values, "values");
        MemorySegment list = Build.constants(values);
        try {
            return with(pointer -> make((out, error, scope) -> Native.nodeIsIn(pointer, list, out, error)));
        } finally {
            Native.valueFree(list);
        }
    }

    /**
     * A range with both ends included: {@code low <= this <= high}.
     *
     * @param low the lower bound: a node, or a Java constant
     * @param high the upper bound: a node, or a Java constant
     * @return a {@code between} node
     * @throws ExpressionException if both bounds are constants not of one orderable type group
     */
    public Expr between(Object low, Object high) {
        return between(low, high, Closed.BOTH);
    }

    /**
     * A range with the ends {@code closed} says.
     *
     * @param low the lower bound: a node, or a Java constant
     * @param high the upper bound: a node, or a Java constant
     * @param closed the ends included
     * @return a {@code between} node
     * @throws ExpressionException if both bounds are constants not of one orderable type group
     */
    public Expr between(Object low, Object high, Closed closed) {
        Objects.requireNonNull(closed, "closed");
        Expr from = lift(low);
        Expr to = lift(high);
        byte[] name = Text.name(closed.jsonName());
        return with(List.of(this, from, to), pointers -> make((out, error, scope) -> Native.nodeBetween(
                pointers[0], pointers[1], pointers[2], scope.bytes(name), name.length, out, error)));
    }

    /**
     * Whether the value is missing or null: never unknown.
     *
     * @return an {@code is_null} node
     * @throws ExpressionException if this is a condition
     */
    public Expr isNull() {
        return unary(this, Native::nodeIsNull);
    }

    /**
     * A regular expression search, in the syntax of Rust's {@code regex}
     * crate: linear time, no lookaround, no backreferences; {@code (?i)} for
     * case-insensitive.
     *
     * @param pattern the pattern
     * @return a {@code matches} node
     * @throws ExpressionException if the pattern does not compile
     */
    public Expr matches(String pattern) {
        return text("matches", pattern);
    }

    /**
     * Whether the string starts with {@code prefix}, literally.
     *
     * @param prefix the prefix
     * @return a {@code starts_with} node
     */
    public Expr startsWith(String prefix) {
        return text("starts_with", prefix);
    }

    /**
     * Whether the string ends with {@code suffix}, literally.
     *
     * @param suffix the suffix
     * @return an {@code ends_with} node
     */
    public Expr endsWith(String suffix) {
        return text("ends_with", suffix);
    }

    /**
     * Whether the string contains {@code part}, literally: not list
     * membership ({@link #isIn(Object...)}) and not a pattern
     * ({@link #matches(String)}).
     *
     * @param part the part
     * @return a {@code contains} node
     */
    public Expr contains(String part) {
        return text("contains", part);
    }

    private Expr text(String op, String argument) {
        byte[] text = Text.name(Objects.requireNonNull(argument, op));
        if (op.equals("matches")) {
            return with(pointer -> make((out, error, scope) ->
                    Native.nodeMatches(pointer, scope.bytes(text), text.length, out, error)));
        }
        byte[] name = Text.name(op);
        return with(pointer -> make((out, error, scope) -> Native.nodeStringTest(
                scope.bytes(name), name.length, pointer, scope.bytes(text), text.length, out, error)));
    }
}
