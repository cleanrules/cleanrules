package io.github.cleanrules;

import java.io.Serial;

/**
 * The hit policy cannot decide, or a decision with several outputs is asked
 * for one ({@code hit_policy_error}).
 */
public class HitPolicyException extends EvaluationException {
    @Serial
    private static final long serialVersionUID = 1L;

    /**
     * An error as the core reports it.
     *
     * @param code the error's code
     * @param message the message
     * @param hint the hint, or null
     * @param json the error's JSON form
     * @param cause the error that caused it, or null
     */
    public HitPolicyException(String code, String message, String hint, String json, Throwable cause) {
        super(code, message, hint, json, cause);
    }
}
