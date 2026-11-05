package io.github.cleanrules;

import java.util.Map;
import java.util.Optional;

/** One rule's result in a {@link Decision}. The engine makes these. */
public final class RuleResult {
    private final Rule rule;
    private final boolean fired;
    private final Truth truth;
    private final Map<String, Object> output;

    RuleResult(Rule rule, boolean fired, Truth truth, Map<String, Object> output) {
        this.rule = rule;
        this.fired = fired;
        this.truth = truth;
        this.output = output;
    }

    /**
     * The rule.
     *
     * @return the rule
     */
    public Rule rule() {
        return rule;
    }

    /**
     * Whether the rule's condition was true.
     *
     * @return whether it fired
     */
    public boolean fired() {
        return fired;
    }

    /**
     * The truth value of the rule's condition.
     *
     * @return the truth value
     */
    public Truth truth() {
        return truth;
    }

    /**
     * The rule's evaluated payload, when the hit policy needed it
     * ({@code spec/rules.md}, <i>When payloads are evaluated</i>).
     *
     * @return the payload, unmodifiable, or empty
     */
    public Optional<Map<String, Object>> output() {
        return Optional.ofNullable(output);
    }

    @Override
    public String toString() {
        return "RuleResult(" + rule.name() + ": " + truth.jsonName() + ")";
    }
}
