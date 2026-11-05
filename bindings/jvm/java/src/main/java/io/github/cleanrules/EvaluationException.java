package io.github.cleanrules;

import java.io.Serial;

/**
 * Strict mode met a value it cannot read, or a batch was given something that
 * is not a list of inputs ({@code evaluation_error}); the parent of the other
 * evaluation errors.
 */
public class EvaluationException extends CleanRulesException {
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
    public EvaluationException(String code, String message, String hint, String json, Throwable cause) {
        super(code, message, hint, json, cause);
    }
}
