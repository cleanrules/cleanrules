package io.github.cleanrules;

import java.io.Serial;
import java.util.Optional;

/**
 * An action threw during {@link RuleSet#run} or
 * {@link io.github.cleanrules.ffi.RuleSet#run}, which stopped there
 * ({@code action_error}). The exception the action threw is the
 * {@linkplain #getCause() cause}; {@link #rule()} names the rule whose action
 * failed, {@link #executed()} the rules whose actions had run, and
 * {@link #decision()} (after {@link RuleSet#run}) or {@link #nativeDecision()}
 * (after the native layer's run) gives the decision the actions saw.
 */
public class ActionException extends CleanRulesException {
    @Serial
    private static final long serialVersionUID = 2L;

    private final transient io.github.cleanrules.ffi.Decision nativeDecision;
    private final transient Decision decision;

    /**
     * An action's error of the native layer, as the core reports it.
     *
     * @param code the error's code, {@code action_error}
     * @param message the message
     * @param hint the hint, or null
     * @param json the error's JSON form
     * @param cause what the action threw, or null
     * @param nativeDecision the decision the actions ran for, or null
     */
    public ActionException(
            String code,
            String message,
            String hint,
            String json,
            Throwable cause,
            io.github.cleanrules.ffi.Decision nativeDecision) {
        this(code, message, hint, json, cause, nativeDecision, null);
    }

    /**
     * An action's error.
     *
     * @param code the error's code, {@code action_error}
     * @param message the message
     * @param hint the hint, or null
     * @param json the error's JSON form
     * @param cause what the action threw, or null
     * @param nativeDecision the native layer's decision the actions ran for, or null
     * @param decision the decision the actions ran for, or null
     */
    public ActionException(
            String code,
            String message,
            String hint,
            String json,
            Throwable cause,
            io.github.cleanrules.ffi.Decision nativeDecision,
            Decision decision) {
        super(code, message, hint, json, cause);
        this.nativeDecision = nativeDecision;
        this.decision = decision;
    }

    /**
     * The decision the actions ran for, when {@link RuleSet#run} threw: the
     * object the actions saw. Empty for the native layer's errors, and after
     * the exception was serialized.
     *
     * @return the decision, or empty
     */
    public Optional<Decision> decision() {
        return Optional.ofNullable(decision);
    }

    /**
     * The decision the actions ran for, when the native layer's
     * {@link io.github.cleanrules.ffi.RuleSet#run} threw: a copy that owns
     * native memory, so close it when done, or the cleaner frees it once
     * unreachable. Empty for {@link RuleSet#run}'s errors, and after the
     * exception was serialized.
     *
     * @return the decision, or empty
     */
    public Optional<io.github.cleanrules.ffi.Decision> nativeDecision() {
        return Optional.ofNullable(nativeDecision);
    }
}
