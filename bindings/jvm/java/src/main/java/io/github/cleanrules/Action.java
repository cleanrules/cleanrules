package io.github.cleanrules;

/**
 * An action: code that {@link RuleSet#run(Object)} calls when its rule is
 * selected, never {@link RuleSet#evaluate(Object)} ({@code spec/rules.md},
 * <i>Actions</i>). It is given to a rule directly
 * ({@link Rule#action(Action)}), or bound to the name a rule gives
 * ({@link RuleSet#bind(java.util.Map)}). An action that throws stops the run
 * with an {@link ActionException} whose cause is what it threw. Actions run on
 * the thread that called {@code run}, in rule order.
 */
@FunctionalInterface
public interface Action {
    /**
     * Do what the rule says.
     *
     * @param context the rule, the input, the rule's result and output, and the decision
     * @throws Exception to fail, which stops the run
     */
    void run(ActionContext context) throws Exception;
}
