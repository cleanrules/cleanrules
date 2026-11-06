package io.github.cleanrules;

import java.util.Map;

/**
 * What an {@link Action} receives ({@code spec/rules.md}, <i>Actions</i>):
 * the rule, the input as given to {@link RuleSet#run(Object)}, the rule's
 * result and evaluated payload, the result tree of its condition, and the
 * whole decision. The engine makes these.
 */
public final class ActionContext {
    private final Rule rule;
    private final Object data;
    private final RuleResult result;
    private final Map<String, Object> output;
    private final Decision decision;
    private final int position;
    private volatile ResultTree tree;

    ActionContext(
            Rule rule, int position, Object data, RuleResult result, Map<String, Object> output, Decision decision) {
        this.position = position;
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
     * The result tree of the rule's condition ({@code spec/json-format.md},
     * <i>Result trees</i>): each node's truth value or value, and why one is
     * unknown or null. Made by the core when first asked for, once.
     *
     * @return the result tree
     */
    public ResultTree tree() {
        ResultTree made = tree;
        if (made == null) {
            made = decision.tree(position);
            tree = made;
        }
        return made;
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
