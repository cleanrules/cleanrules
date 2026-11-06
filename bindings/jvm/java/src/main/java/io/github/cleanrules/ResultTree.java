package io.github.cleanrules;

import io.github.cleanrules.internal.Build;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import io.github.cleanrules.internal.Values;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * The result of evaluating a node and each of its children ({@code spec/ast.md},
 * <i>Evaluating one node</i>; {@code spec/json-format.md}, <i>Result trees</i>),
 * from {@link Expr#evaluateTree(Object)}, {@link Expr#evaluateValueTree(Object)}
 * or {@link ActionContext#tree()}.
 *
 * <pre>{@code
 * ResultTree tree = field("age").ge(18).and(field("country").isIn("ES")).evaluateTree(Map.of("age", 20));
 * tree.truth();                               // Optional[UNKNOWN]: country is missing
 * tree.children().get(1).reason().get().code(); // "missing_field"
 * }</pre>
 *
 * <p>A node in a condition position has a truth value; every other node a
 * value, given back as {@code spec/data-model.md} says (<i>Values given
 * back</i>), opaque values as the very objects read. The core evaluates and
 * builds the tree; result trees are immutable and may be shared between
 * threads. A tree holds native memory, which the cleaner frees once no part
 * of it is reachable.
 */
public final class ResultTree {
    /** What every result of one tree shares: the native tree and its value. */
    private static final class Shared {
        private final Handle tree;
        private final Handle value;

        Shared(MemorySegment tree, MemorySegment value) {
            this.tree = Handle.own(this, tree, Native::treeFree, "ResultTree");
            this.value = Handle.own(this, value, Native::valueFree, "ResultTree");
        }
    }

    private static final byte[] CHILDREN = Text.name("children");

    private final Shared shared;
    /** The result's position in pre-order, which is its node's. */
    private final int index;
    /** The child positions from the root to this result. */
    private final int[] route;
    private final Truth truth;
    private final Object value;
    private final Reason reason;
    private final List<ResultTree> children;
    private volatile Expr node;

    private ResultTree(Shared shared, int index, int[] route, Map<String, Object> result, int[] next) {
        this.shared = shared;
        this.index = index;
        this.route = route;
        if (result.containsKey("truth")) {
            Object found = result.get("truth");
            this.truth = found == null ? Truth.UNKNOWN : (Boolean) found ? Truth.TRUE : Truth.FALSE;
            this.value = null;
        } else {
            this.truth = null;
            this.value = result.get("value");
        }
        this.reason = Read.reason(result.get("reason"));
        Object found = result.get("children");
        List<ResultTree> made = new ArrayList<>();
        if (found != null) {
            List<?> items = Read.list(found);
            for (int i = 0; i < items.size(); i++) {
                int[] deeper = Arrays.copyOf(route, route.length + 1);
                deeper[route.length] = i;
                next[0]++;
                made.add(new ResultTree(shared, next[0], deeper, Read.map(items.get(i)), next));
            }
        }
        this.children = Collections.unmodifiableList(made);
    }

    /** The tree of an owned {@code cr_tree *}, with the objects of its opaque values. */
    static ResultTree of(MemorySegment tree, List<Object> opaques) {
        MemorySegment value;
        try {
            value = Scope.value((out, error) -> Native.treeValue(tree, out, error));
        } catch (RuntimeException | Error thrown) {
            Native.treeFree(tree);
            throw thrown;
        }
        Shared shared = new Shared(tree, value);
        MemorySegment pointer = shared.value.acquire();
        try {
            Map<String, Object> root = Read.map(Values.toJava(pointer, opaques));
            return new ResultTree(shared, 0, new int[0], root, new int[] {0});
        } finally {
            shared.value.release();
        }
    }

    /**
     * The node evaluated. The halves of a {@code between} are comparisons the
     * evaluation made.
     *
     * @return the node
     */
    public Expr node() {
        Expr made = node;
        if (made == null) {
            MemorySegment pointer = shared.tree.acquire();
            try {
                made = Expr.make((out, error, scope) -> Native.treeNode(pointer, index, out, error));
            } finally {
                shared.tree.release();
            }
            node = made;
        }
        return made;
    }

    /**
     * The truth value, for a node in a condition position.
     *
     * @return the truth value, or empty for a value
     */
    public Optional<Truth> truth() {
        return Optional.ofNullable(truth);
    }

    /**
     * The value, for a node in a value position; {@code null} when it is null,
     * and for a condition.
     *
     * @return the value, or null
     */
    public Object value() {
        return value;
    }

    /**
     * Why the truth value is unknown or the value null, if it is.
     *
     * @return the reason, or empty
     */
    public Optional<Reason> reason() {
        return Optional.ofNullable(reason);
    }

    /**
     * The children's results, in order.
     *
     * @return the children, unmodifiable
     */
    public List<ResultTree> children() {
        return children;
    }

    /**
     * This result and its children in the JSON format ({@code spec/json-format.md},
     * <i>Result trees</i>).
     *
     * @return the JSON text
     */
    public String toJson() {
        MemorySegment pointer = shared.value.acquire();
        try (Scope scope = new Scope()) {
            MemorySegment found = pointer;
            MemorySegment key = scope.bytes(CHILDREN);
            for (int step : route) {
                MemorySegment list = Native.valueMapGet(found, key, CHILDREN.length);
                found = Native.valueListGet(list, step);
            }
            MemorySegment at = found;
            return Scope.text((out, len, error) -> Native.valueToJson(at, out, len, error));
        } finally {
            shared.value.release();
        }
    }

    /**
     * The node and its truth value or value, such as {@code ResultTree(age >= 18: true)}.
     *
     * @return the text
     */
    @Override
    public String toString() {
        String outcome = truth != null
                ? truth.jsonName()
                : value instanceof String text ? Build.quote(text) : String.valueOf(value);
        return "ResultTree(" + node() + ": " + outcome + ")";
    }
}
