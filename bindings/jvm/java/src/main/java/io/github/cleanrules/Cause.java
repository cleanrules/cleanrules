package io.github.cleanrules;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import java.lang.foreign.MemorySegment;
import java.util.List;
import java.util.Optional;

/**
 * A condition in a rule's requirement, with what was read
 * ({@code spec/explain.md}, <i>Causes</i>). The engine makes these;
 * {@link #toString()} is the cause's line of the explanation, such as
 * {@code x > 10 is false (x = 8); short by 2}.
 */
public final class Cause implements Requirement {
    /**
     * A field the cause read, and its value.
     *
     * @param path the field's path: {@link String} keys and {@link Integer} indexes
     * @param value its value, as {@code spec/data-model.md} gives values back
     */
    public record FieldValue(List<Object> path, Object value) {}

    private final String nodeJson;
    private volatile Expr node;
    private final Truth truth;
    private final boolean negated;
    private final boolean holds;
    private final List<FieldValue> values;
    private final Reason reason;
    private final Counterfactual counterfactual;
    private final String text;

    Cause(String nodeJson, Truth truth, boolean negated, boolean holds, List<FieldValue> values, Reason reason,
            Counterfactual counterfactual, String text) {
        this.nodeJson = nodeJson;
        this.truth = truth;
        this.negated = negated;
        this.holds = holds;
        this.values = List.copyOf(values);
        this.reason = reason;
        this.counterfactual = counterfactual;
        this.text = text;
    }

    /**
     * The condition (a {@code between} half is the comparison it is).
     *
     * @return the condition
     */
    public Expr node() {
        Expr read = node;
        if (read == null) {
            byte[] bytes = Text.document(nodeJson);
            // The core wrote the node: it reads back, however deep.
            Limits limits = Limits.defaults()
                    .withMaxSize(Math.max(bytes.length, 1))
                    .withMaxNesting(Integer.MAX_VALUE)
                    .withMaxDepth(Integer.MAX_VALUE);
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.pointer();
                scope.check(Native.nodeRead(scope.bytes(bytes), bytes.length, 1, scope.limits(limits), out,
                        scope.error()));
                read = Expr.wrap(out.get(ADDRESS, 0));
            }
            node = read;
        }
        return read;
    }

    /**
     * The condition's truth value.
     *
     * @return the truth value
     */
    public Truth truth() {
        return truth;
    }

    /**
     * Whether the condition is under a negation, so the rule needed it false.
     *
     * @return whether it is negated
     */
    public boolean negated() {
        return negated;
    }

    /**
     * Whether the condition went the way the rule needed.
     *
     * @return whether it holds
     */
    public boolean holds() {
        return holds;
    }

    /**
     * The fields the condition read, with their values, in order.
     *
     * @return the fields and values
     */
    public List<FieldValue> values() {
        return values;
    }

    /**
     * Why the condition is unknown, if it is.
     *
     * @return the reason, or empty
     */
    public Optional<Reason> reason() {
        return Optional.ofNullable(reason);
    }

    /**
     * For a comparison that did not hold, what it needed.
     *
     * @return the counterfactual, or empty
     */
    public Optional<Counterfactual> counterfactual() {
        return Optional.ofNullable(counterfactual);
    }

    /**
     * The cause's line in the explanation's text.
     *
     * @return the text
     */
    @Override
    public String toString() {
        return text;
    }
}
