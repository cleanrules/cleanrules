package io.github.cleanrules;

import java.io.Serial;

/**
 * A condition node breaks a construction rule ({@code expression_error}).
 */
public class ExpressionException extends CleanRulesException {
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
    public ExpressionException(String code, String message, String hint, String json, Throwable cause) {
        super(code, message, hint, json, cause);
    }
}
