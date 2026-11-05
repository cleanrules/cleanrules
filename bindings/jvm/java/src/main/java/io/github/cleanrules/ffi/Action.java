package io.github.cleanrules.ffi;

import io.github.cleanrules.ActionException;

/**
 * An action: code that {@link RuleSet#run} calls for each selected rule that
 * names it ({@code spec/rules.md}, <i>Actions</i>). An action that throws stops
 * the run with an {@link ActionException} whose cause is what it threw.
 * Actions run on the thread that called {@code run}, in rule order, and must
 * not return before they are done: they are synchronous.
 */
@FunctionalInterface
public interface Action {
    /**
     * Do what the rule says.
     *
     * @param call the rule, the input and the decision, valid during this call
     * @throws Exception to fail, which stops the run
     */
    void run(ActionCall call) throws Exception;
}
