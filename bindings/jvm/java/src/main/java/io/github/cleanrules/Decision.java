package io.github.cleanrules;

import io.github.cleanrules.internal.Bridge;
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
import java.util.function.Function;

/**
 * The outcome of evaluating a {@link RuleSet} on one input
 * ({@code spec/rules.md}): every rule's result, the rules the hit policy
 * selected, the outputs, and the explanation. The engine makes these.
 *
 * <p>Outputs come back as {@code spec/data-model.md} says (<i>Java and
 * Scala</i>, <i>Values given back</i>): {@code null}, {@link Boolean},
 * {@link Long}, {@link Double}, {@link java.math.BigDecimal}, {@link String},
 * {@link java.time.LocalDate}, {@link java.time.LocalDateTime},
 * {@link java.time.ZonedDateTime}, unmodifiable lists and maps in the core's
 * order, and, for an opaque value, the very object that was read
 * ({@code null} in a decision read from a document).
 *
 * <p>Decisions are immutable and may be shared between threads. A decision
 * holds native memory: {@link #close()} frees it at once, or the cleaner
 * frees it once the decision is unreachable, so closing is optional. What was
 * read before closing stays readable ({@link #results()}, {@link #selected()},
 * {@link #fired()}, an explanation already made); what needs the native
 * decision then throws {@link IllegalStateException}.
 */
public final class Decision implements AutoCloseable {
    /** The explanation's text, made by the core when first read, once. */
    private static final class Text implements java.util.function.Supplier<String> {
        private final Decision decision;
        private volatile String text;

        Text(Decision decision) {
            this.decision = decision;
        }

        @Override
        public String get() {
            String made = text;
            if (made == null) {
                made = decision.nativeDecision.explain();
                text = made;
            }
            return made;
        }
    }

    private final RuleSet ruleSet;
    private final io.github.cleanrules.ffi.Decision nativeDecision;
    private final List<RuleResult> results;
    private final List<RuleResult> selected;
    private volatile Explanation explanation;

    @SuppressWarnings("unchecked") // The core gives a map or a null for a rule's output.
    Decision(RuleSet ruleSet, io.github.cleanrules.ffi.Decision nativeDecision) {
        this.ruleSet = ruleSet;
        this.nativeDecision = nativeDecision;
        List<Rule> rules = ruleSet.rules();
        List<RuleResult> all = new ArrayList<>(rules.size());
        for (int i = 0; i < rules.size(); i++) {
            all.add(new RuleResult(
                    rules.get(i),
                    i,
                    nativeDecision.firedAt(i),
                    nativeDecision.truth(i),
                    nativeDecision.ruleOutput(i).orElse(null)));
        }
        this.results = Collections.unmodifiableList(all);
        List<RuleResult> chosen = new ArrayList<>();
        for (int position : nativeDecision.selected()) {
            chosen.add(all.get(position));
        }
        this.selected = Collections.unmodifiableList(chosen);
    }

    /** Call {@code call} with the native decision, held for the call. */
    <T> T withNative(Function<MemorySegment, T> call) {
        Handle handle = Bridge.ffi().handle(nativeDecision);
        MemorySegment pointer = handle.acquire();
        try {
            return call.apply(pointer);
        } finally {
            handle.release();
        }
    }

    /**
     * The rule set that decided.
     *
     * @return the rule set
     */
    public RuleSet ruleSet() {
        return ruleSet;
    }

    /**
     * The hit policy that decided.
     *
     * @return the hit policy
     */
    public HitPolicy hitPolicy() {
        return ruleSet.hitPolicy();
    }

    /**
     * Every rule's result, in rule order.
     *
     * @return the results
     */
    public List<RuleResult> results() {
        return results;
    }

    /**
     * The results the hit policy selected, in rule order.
     *
     * @return the selected results
     */
    public List<RuleResult> selected() {
        return selected;
    }

    /**
     * The names of the rules that fired, in rule order, selected or not.
     *
     * @return the names
     */
    public List<String> fired() {
        List<String> names = new ArrayList<>();
        for (RuleResult result : results) {
            if (result.fired()) {
                names.add(result.rule().name());
            }
        }
        return Collections.unmodifiableList(names);
    }

    /**
     * The payloads of the selected rules, in rule order; an empty map for a
     * rule without one.
     *
     * @return the payloads
     */
    public List<Map<String, Object>> outputs() {
        return nativeDecision.outputs();
    }

    /**
     * The single output: the selected rule's payload, or the aggregate.
     *
     * @return the output, or empty when no rule was selected
     * @throws HitPolicyException for {@link HitPolicy#RULE_ORDER}, and
     *     {@link HitPolicy#COLLECT} without an aggregate, which have several
     *     outputs (read {@link #outputs()})
     */
    @SuppressWarnings("unchecked") // The core gives a map or a null.
    public Optional<Map<String, Object>> output() {
        return Optional.ofNullable((Map<String, Object>) nativeDecision.output());
    }

    /**
     * With {@link HitPolicy#COLLECT} and an aggregate, its values.
     *
     * @return the aggregated values, or empty without an aggregate or when no rule was selected
     */
    public Optional<Map<String, Object>> aggregated() {
        return ruleSet.aggregate().isEmpty() ? Optional.empty() : output();
    }

    /** The result tree of the condition of the rule at {@code position}. */
    ResultTree tree(int position) {
        List<Object> opaques = Bridge.ffi().opaques(nativeDecision);
        MemorySegment tree = withNative(pointer ->
                Scope.value((out, error) -> Native.decisionRuleTree(pointer, position, out, error)));
        return ResultTree.of(tree, opaques);
    }

    /**
     * Explain every rule: selected, fired but not selected, not fired, or
     * unknown, with the conditions it needed, the values read and
     * counterfactuals ({@code spec/explain.md}).
     *
     * @return the explanation
     */
    public Explanation explain() {
        Explanation made = explanation;
        if (made == null) {
            List<Object> opaques = Bridge.ffi().opaques(nativeDecision);
            Object value = withNative(pointer -> Values.take(
                    Scope.value((out, error) -> Native.decisionExplanation(pointer, out, error)), opaques));
            made = new Explanation(this, Read.map(value), new Text(this));
            explanation = made;
        }
        return made;
    }

    /**
     * One rule's explanation, typically of a rule that did not fire or was not selected.
     *
     * @param name the rule's name
     * @return its explanation
     * @throws RuleNotFoundException if the rule set has no rule with that name
     */
    public RuleExplanation whyNot(String name) {
        return explain().forRule(Objects.requireNonNull(name, "name"));
    }

    /**
     * The decision document of the JSON format ({@code spec/json-format.md},
     * <i>Decisions</i>), which {@link RuleSet#readDecision(String)} reads back.
     *
     * @return the JSON text
     */
    public String toJson() {
        return nativeDecision.write();
    }

    /**
     * The decision document in JSON or YAML.
     *
     * @param format JSON or YAML
     * @return the text
     * @throws UnsupportedException for the rule syntax and CSV, which hold no decisions
     */
    public String write(Format format) {
        return nativeDecision.write(format);
    }

    /**
     * Free the native decision now; closing again does nothing. Optional: the
     * cleaner frees it once the decision is unreachable.
     */
    @Override
    public void close() {
        nativeDecision.close();
    }

    /**
     * The hit policy and the selected rules, such as {@code Decision(priority: selected [vip])}.
     *
     * @return the text
     */
    @Override
    public String toString() {
        List<String> names = new ArrayList<>();
        for (RuleResult result : selected) {
            names.add(result.rule().name());
        }
        return "Decision(" + hitPolicy().jsonName() + ": selected " + names + ")";
    }
}
