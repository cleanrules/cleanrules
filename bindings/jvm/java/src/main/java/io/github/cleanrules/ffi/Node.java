package io.github.cleanrules.ffi;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.EvaluationException;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.Limits;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.internal.DataReader;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.PathTree;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import io.github.cleanrules.internal.Values;
import java.lang.foreign.MemorySegment;
import java.util.List;
import java.util.Objects;

/**
 * One node: a condition such as {@code customer.years > 5}, or a value such as
 * {@code order.total * 0.9}, read on its own (not as a document) and evaluated
 * on one input with its result tree ({@code spec/ast.md}, <i>Evaluating one
 * node</i>; {@code spec/json-format.md}, <i>Result trees</i>).
 *
 * <p>Java data is read as for {@link RuleSet#evaluate(Object)}: only the paths
 * the node reads. A node owns native memory: close it when done, or the
 * cleaner frees it once it is unreachable. Nodes never change, so they may be
 * used from several threads at once.
 */
public final class Node implements AutoCloseable {
    private final Handle handle;
    private volatile PathTree tree;

    private Node(MemorySegment pointer) {
        handle = Handle.own(this, pointer, Native::nodeFree, "Node");
    }

    /**
     * Read a node in JSON or YAML (the node itself, as in a rule's
     * {@code when}), or in the rule syntax.
     *
     * @param text the node's text
     * @param format JSON, YAML or TEXT
     * @return the node
     * @throws CleanRulesException if the text is not a valid node
     */
    public static Node read(String text, Format format) {
        return read(text, format, null);
    }

    /**
     * Read a node within limits.
     *
     * @param text the node's text
     * @param format JSON, YAML or TEXT
     * @param limits the limits, or null for the defaults
     * @return the node
     * @throws CleanRulesException if the text is not a valid node
     */
    public static Node read(String text, Format format, Limits limits) {
        return read(Text.document(Objects.requireNonNull(text, "text")), format, limits);
    }

    /**
     * Read a node from UTF-8 bytes.
     *
     * @param text the node's bytes
     * @param format JSON, YAML or TEXT
     * @return the node
     * @throws CleanRulesException if the text is not a valid node
     */
    public static Node read(byte[] text, Format format) {
        return read(text, format, null);
    }

    /**
     * Read a node from UTF-8 bytes within limits.
     *
     * @param text the node's bytes
     * @param format JSON, YAML or TEXT
     * @param limits the limits, or null for the defaults
     * @return the node
     * @throws CleanRulesException if the text is not a valid node
     */
    public static Node read(byte[] text, Format format, Limits limits) {
        Objects.requireNonNull(text, "text");
        Objects.requireNonNull(format, "format");
        Native.ensureLoaded();
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(Native.nodeRead(
                    scope.bytes(text), text.length, Native.format(format), scope.limits(limits), out, scope.error()));
            return new Node(out.get(ADDRESS, 0));
        }
    }

    /**
     * Write the node as JSON.
     *
     * @return the JSON text
     */
    public String write() {
        return write(Format.JSON);
    }

    /**
     * Write the node in JSON, YAML or the rule syntax.
     *
     * @param format the syntax
     * @return the text
     * @throws UnsupportedException for CSV
     */
    public String write(Format format) {
        Objects.requireNonNull(format, "format");
        return with(pointer ->
                Scope.text((out, len, error) -> Native.nodeWrite(pointer, Native.format(format), out, len, error)));
    }

    /**
     * The paths of the input that the node reads, in preorder and each once.
     *
     * @return the paths: lists of {@link String} keys and {@link Integer} indexes
     */
    public List<List<Object>> fieldPaths() {
        return with(pointer -> Values.paths(Scope.value((out, error) -> Native.nodeFieldPaths(pointer, out, error))));
    }

    private PathTree tree() {
        PathTree read = tree;
        if (read == null) {
            read = PathTree.of(fieldPaths());
            tree = read;
        }
        return read;
    }

    /**
     * Evaluate the node as a condition on Java data, under Boolean logic and
     * not strict: its result tree as JSON, whose root holds the truth value.
     *
     * @param data the input
     * @return the result tree's JSON text
     * @throws ExpressionException if the node is a value, not a condition
     */
    public String evaluateTree(Object data) {
        return evaluateTree(data, "boolean", false);
    }

    /**
     * Evaluate the node as a condition on Java data, under a logic, strict or not.
     *
     * @param data the input
     * @param logic the logic's name, such as {@code boolean}
     * @param strict whether a missing field or a type mismatch is an error
     * @return the result tree's JSON text
     * @throws RegistryException for an unknown logic
     * @throws ExpressionException if the node is a value, not a condition
     * @throws EvaluationException in strict mode, as for a rule set
     */
    public String evaluateTree(Object data, String logic, boolean strict) {
        Objects.requireNonNull(logic, "logic");
        return evaluateNative(data, (pointer, input, scope) -> condition(pointer, input, logic, strict, scope));
    }

    /**
     * Evaluate the node as a condition on a JSON input, under Boolean logic
     * and not strict.
     *
     * @param json the input's JSON text
     * @return the result tree's JSON text
     * @throws CleanRulesException if the JSON is invalid or the evaluation fails
     */
    public String evaluateTreeJson(String json) {
        return evaluateTreeJson(json, "boolean", false, null);
    }

    /**
     * Evaluate the node as a condition on a JSON input.
     *
     * @param json the input's JSON text
     * @param logic the logic's name, such as {@code boolean}
     * @param strict whether a missing field or a type mismatch is an error
     * @param limits the limits for reading the input, or null for the defaults
     * @return the result tree's JSON text
     * @throws CleanRulesException if the JSON is invalid or the evaluation fails
     */
    public String evaluateTreeJson(String json, String logic, boolean strict, Limits limits) {
        Objects.requireNonNull(logic, "logic");
        return evaluateJson(json, limits, (pointer, input, scope) -> condition(pointer, input, logic, strict, scope));
    }

    /**
     * Evaluate the node as a value on Java data, not strict: its result tree as
     * JSON, whose root holds the value.
     *
     * @param data the input
     * @return the result tree's JSON text
     * @throws ExpressionException if the node is a condition, not a value
     */
    public String evaluateValueTree(Object data) {
        return evaluateValueTree(data, false);
    }

    /**
     * Evaluate the node as a value on Java data, strict or not.
     *
     * @param data the input
     * @param strict whether a missing field or a type mismatch is an error
     * @return the result tree's JSON text
     * @throws CleanRulesException if the node is a condition, or the evaluation fails
     */
    public String evaluateValueTree(Object data, boolean strict) {
        return evaluateNative(data, (pointer, input, scope) -> value(pointer, input, strict, scope));
    }

    /**
     * Evaluate the node as a value on a JSON input, not strict.
     *
     * @param json the input's JSON text
     * @return the result tree's JSON text
     * @throws CleanRulesException if the JSON is invalid or the evaluation fails
     */
    public String evaluateValueTreeJson(String json) {
        return evaluateValueTreeJson(json, false, null);
    }

    /**
     * Evaluate the node as a value on a JSON input.
     *
     * @param json the input's JSON text
     * @param strict whether a missing field or a type mismatch is an error
     * @param limits the limits for reading the input, or null for the defaults
     * @return the result tree's JSON text
     * @throws CleanRulesException if the JSON is invalid or the evaluation fails
     */
    public String evaluateValueTreeJson(String json, boolean strict, Limits limits) {
        return evaluateJson(json, limits, (pointer, input, scope) -> value(pointer, input, strict, scope));
    }

    @FunctionalInterface
    private interface Tree {
        String evaluate(MemorySegment node, MemorySegment input, Scope scope);
    }

    private static String condition(
            MemorySegment node, MemorySegment input, String logic, boolean strict, Scope scope) {
        byte[] name = Text.name(logic);
        MemorySegment out = scope.pointer();
        MemorySegment len = scope.size();
        MemorySegment text = name.length == 0 ? scope.bytes(new byte[] {0}) : scope.bytes(name);
        int status = Native.nodeEvaluateTree(node, input, text, name.length, strict, out, len, scope.error());
        scope.check(status, scope.causes);
        return Scope.takeText(out, len);
    }

    private static String value(MemorySegment node, MemorySegment input, boolean strict, Scope scope) {
        MemorySegment out = scope.pointer();
        MemorySegment len = scope.size();
        scope.check(Native.nodeEvaluateValueTree(node, input, strict, out, len, scope.error()), scope.causes);
        return Scope.takeText(out, len);
    }

    private String evaluateNative(Object data, Tree tree) {
        return with(pointer -> {
            try (Scope scope = new Scope();
                    DataReader reader = new DataReader()) {
                MemorySegment input = reader.read(data, tree());
                scope.onClose(() -> Native.valueFree(input));
                scope.causes = reader.causes;
                return tree.evaluate(pointer, input, scope);
            }
        });
    }

    private String evaluateJson(String json, Limits limits, Tree tree) {
        byte[] bytes = Text.document(Objects.requireNonNull(json, "json"));
        return with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.pointer();
                scope.check(Native.valueFromJson(
                        scope.bytes(bytes), bytes.length, scope.limits(limits), out, scope.error()));
                MemorySegment input = out.get(ADDRESS, 0);
                scope.onClose(() -> Native.valueFree(input));
                return tree.evaluate(pointer, input, scope);
            }
        });
    }

    private <T> T with(java.util.function.Function<MemorySegment, T> call) {
        MemorySegment pointer = handle.acquire();
        try {
            return call.apply(pointer);
        } finally {
            handle.release();
        }
    }

    /** Free the native node now; closing again does nothing. */
    @Override
    public void close() {
        handle.closeFromOwner();
    }
}
