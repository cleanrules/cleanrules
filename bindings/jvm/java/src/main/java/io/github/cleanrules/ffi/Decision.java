package io.github.cleanrules.ffi;

import static java.lang.foreign.ValueLayout.JAVA_BOOLEAN;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import io.github.cleanrules.Format;
import io.github.cleanrules.HitPolicyException;
import io.github.cleanrules.Truth;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Values;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;

/**
 * What a rule set made of one input ({@link RuleSet#evaluate(Object)}), or a
 * recorded decision read with its rule set ({@link RuleSet#readDecision}):
 * which rules fired and were selected, and the outputs ({@code spec/rules.md}).
 *
 * <p>Outputs and payloads come back as Java values ({@code spec/data-model.md},
 * <i>Java and Scala</i>, <i>Values given back</i>): {@code null},
 * {@link Boolean}, {@link Long}, {@link Double}, {@link java.math.BigDecimal},
 * {@link String}, {@link java.time.LocalDate},
 * {@link java.time.LocalDateTime}, {@link java.time.ZonedDateTime}, unmodifiable
 * lists and maps in the core's order, and, for an opaque value, the very object
 * that was read ({@code null} in a decision read from a document).
 *
 * <p>A decision owns native memory: close it when done, or the cleaner frees
 * it once it is unreachable. It keeps what it needs of its rule set, which may
 * be closed first. Decisions never change, so they may be used from several
 * threads at once.
 */
public final class Decision implements AutoCloseable {
    private final Handle handle;
    private final List<Object> opaques;

    Decision(MemorySegment pointer, List<Object> opaques) {
        this.handle = Handle.own(this, pointer, Native::decisionFree, "Decision");
        this.opaques = opaques;
    }

    /** An owned copy of a decision borrowed from a batch, an action call or an error. */
    static Decision copyOf(MemorySegment borrowed, List<Object> opaques) {
        MemorySegment copy = Scope.value((out, error) -> Native.decisionClone(borrowed, out, error));
        return new Decision(copy, opaques);
    }

    /** The handle, for the idiomatic API ({@code Bridge}). */
    Handle handle() {
        return handle;
    }

    /** The objects of the opaque values, by token, for the idiomatic API. */
    List<Object> opaques() {
        return opaques;
    }

    private <T> T with(java.util.function.Function<MemorySegment, T> call) {
        MemorySegment pointer = handle.acquire();
        try {
            return call.apply(pointer);
        } finally {
            handle.release();
        }
    }

    private int checked(int rule) {
        return Objects.checkIndex(rule, size());
    }

    /**
     * The number of rules of the decision's rule set.
     *
     * @return the number of rules
     */
    public int size() {
        return with(pointer -> Math.toIntExact(Native.decisionLen(pointer)));
    }

    /**
     * The name of a rule.
     *
     * @param rule the rule's position, from 0
     * @return its name
     * @throws IndexOutOfBoundsException if there is no such rule
     */
    public String ruleName(int rule) {
        checked(rule);
        return with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment name = scope.pointer();
                MemorySegment len = scope.size();
                scope.check(Native.decisionRuleName(pointer, rule, name, len, scope.error()));
                return Scope.borrowedText(name, len);
            }
        });
    }

    /**
     * Whether a rule fired: its condition passed the logic's activation test.
     *
     * @param rule the rule's position, from 0
     * @return whether it fired
     * @throws IndexOutOfBoundsException if there is no such rule
     */
    public boolean firedAt(int rule) {
        checked(rule);
        return with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.bool();
                scope.check(Native.decisionFired(pointer, rule, out, scope.error()));
                return out.get(JAVA_BOOLEAN, 0);
            }
        });
    }

    /**
     * The truth value of a rule's condition.
     *
     * @param rule the rule's position, from 0
     * @return its truth value
     * @throws IndexOutOfBoundsException if there is no such rule
     */
    public Truth truth(int rule) {
        checked(rule);
        return with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.int32();
                scope.check(Native.decisionTruth(pointer, rule, out, scope.error()));
                return Native.truth(out.get(JAVA_INT, 0));
            }
        });
    }

    /**
     * The positions of the rules the hit policy selected, in rule order.
     *
     * @return the positions
     */
    public List<Integer> selected() {
        return with(pointer -> {
            long count = Native.decisionSelectedLen(pointer);
            List<Integer> rules = new ArrayList<>();
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.size();
                for (long i = 0; i < count; i++) {
                    scope.check(Native.decisionSelected(pointer, i, out, scope.error()));
                    rules.add(Math.toIntExact(out.get(JAVA_LONG, 0)));
                }
            }
            return Collections.unmodifiableList(rules);
        });
    }

    /**
     * The decision's single output ({@code spec/rules.md}, <i>Outputs</i>):
     * the selected rule's payload as a map, the aggregate, or null when no rule
     * was selected.
     *
     * @return the output, or null
     * @throws HitPolicyException for a policy with several outputs
     *     ({@code rule_order}, or {@code collect} without an aggregate): use
     *     {@link #outputs()}
     */
    public Object output() {
        return with(pointer -> Values.take(
                Scope.value((out, error) -> Native.decisionOutputValue(pointer, out, error)), opaques));
    }

    /**
     * The outputs of the selected rules, in rule order: each rule's payload.
     *
     * @return the outputs
     */
    @SuppressWarnings("unchecked") // The core gives a list of maps.
    public List<Map<String, Object>> outputs() {
        return with(pointer -> (List<Map<String, Object>>) Values.take(
                Scope.value((out, error) -> Native.decisionOutputsValue(pointer, out, error)), opaques));
    }

    /**
     * The output of one rule: its evaluated payload, when it was evaluated
     * ({@code spec/rules.md}, <i>When payloads are evaluated</i>).
     *
     * @param rule the rule's position, from 0
     * @return its payload, or empty when it was not evaluated
     * @throws IndexOutOfBoundsException if there is no such rule
     */
    @SuppressWarnings("unchecked") // The core gives a map or a null.
    public Optional<Map<String, Object>> ruleOutput(int rule) {
        checked(rule);
        return with(pointer -> Optional.ofNullable((Map<String, Object>) Values.take(
                Scope.value((out, error) -> Native.decisionRuleOutput(pointer, rule, out, error)), opaques)));
    }

    /**
     * The single output as a JSON data value ({@code spec/json-format.md},
     * <i>Values</i>), {@code null} when no rule was selected.
     *
     * @return the JSON text
     * @throws HitPolicyException for a policy with several outputs
     */
    public String outputJson() {
        return with(pointer -> Scope.text((out, len, error) -> Native.decisionOutput(pointer, out, len, error)));
    }

    /**
     * The outputs of the selected rules as a JSON array.
     *
     * @return the JSON text
     */
    public String outputsJson() {
        return with(pointer -> Scope.text((out, len, error) -> Native.decisionOutputs(pointer, out, len, error)));
    }

    /**
     * Write the decision as a JSON document ({@code spec/json-format.md},
     * <i>Decisions</i>), which {@link RuleSet#readDecision} reads back.
     *
     * @return the JSON text
     */
    public String write() {
        return write(Format.JSON);
    }

    /**
     * Write the decision as a JSON or YAML document.
     *
     * @param format JSON or YAML
     * @return the text
     * @throws UnsupportedException for the rule syntax and CSV, which hold no decisions
     */
    public String write(Format format) {
        Objects.requireNonNull(format, "format");
        return with(pointer ->
                Scope.text((out, len, error) -> Native.decisionWrite(pointer, Native.format(format), out, len, error)));
    }

    /**
     * The decision's explanation ({@code spec/explain.md}) as text.
     *
     * @return the explanation
     */
    public String explain() {
        return explain(Format.TEXT);
    }

    /**
     * The decision's explanation as text, or as a JSON or YAML document.
     *
     * @param format TEXT, JSON or YAML
     * @return the explanation
     */
    public String explain(Format format) {
        Objects.requireNonNull(format, "format");
        return with(pointer ->
                Scope.text((out, len, error) -> Native.decisionExplain(pointer, Native.format(format), out, len, error)));
    }

    /** Free the native decision now; closing again does nothing. */
    @Override
    public void close() {
        handle.closeFromOwner();
    }
}
