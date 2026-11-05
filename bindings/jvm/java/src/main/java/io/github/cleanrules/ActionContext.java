package io.github.cleanrules;

import java.util.Map;

/**
 * What an {@link Action} receives ({@code spec/rules.md}, <i>Actions</i>):
 * the rule, the input as given to {@link RuleSet#run(Object)}, the rule's
 * result and evaluated payload, and the whole decision. The engine makes these.
 */
public final class ActionContext {
    private final Rule rule;
    private final Object data;
    private final RuleResult result;
    private final Map<String, Object> output;
    private final Decision decision;

    ActionContext(Rule rule, Object data, RuleResult result, Map<String, Object> output, Decision decision) {
        this.rule = rule;
        this.data = data;
        this.result = result;
        this.output = output;
        this.decision = decision;
    }

    /**
     * The rule whose action runs.
     *
     * @return the rule
     */
    public Rule rule() {
        return rule;
    }

    /**
     * The input, the object given to {@link RuleSet#run(Object)}.
     *
     * @return the input
     */
    public Object data() {
        return data;
    }

    /**
     * The rule's result in the decision.
     *
     * @return the result
     */
    public RuleResult result() {
        return result;
    }

    /**
     * The rule's evaluated payload; empty when it has none.
     *
     * @return the payload, unmodifiable
     */
    public Map<String, Object> output() {
        return output;
    }

    /**
     * The whole decision: the object {@link RuleSet#run(Object)} returns.
     *
     * @return the decision
     */
    public Decision decision() {
        return decision;
    }
}
