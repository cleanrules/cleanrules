package io.github.cleanrules;

import io.github.cleanrules.ffi.Decision;
import java.io.Serial;
import java.util.Optional;

/**
 * An action threw during {@link io.github.cleanrules.ffi.RuleSet#run}, which stopped there
 * ({@code action_error}). The exception the action threw is the
 * {@linkplain #getCause() cause}; {@link #rule()} names the rule whose action
 * failed, {@link #executed()} the rules whose actions had run, and
 * {@link #decision()} gives the decision the actions saw.
 */
public class ActionException extends CleanRulesException {
    @Serial
    private static final long serialVersionUID = 1L;

    private final transient Decision decision;

    /**
     * An action's error as the core reports it.
     *
     * @param code the error's code, {@code action_error}
     * @param message the message
     * @param hint the hint, or null
     * @param json the error's JSON form
     * @param cause what the action threw, or null
     * @param decision the decision the actions ran for, or null
     */
    public ActionException(String code, String message, String hint, String json, Throwable cause, Decision decision) {
        super(code, message, hint, json, cause);
        this.decision = decision;
    }

    /**
     * The decision the actions ran for. It owns native memory: close it when
     * done, or the cleaner frees it once unreachable. Empty after the
     * exception was serialized.
     *
     * @return the decision, or empty
     */
    public Optional<Decision> decision() {
        return Optional.ofNullable(decision);
    }
}
