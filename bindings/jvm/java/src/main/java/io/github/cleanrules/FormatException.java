package io.github.cleanrules;

import java.io.Serial;

/**
 * A document's text or shape is invalid ({@code format_error}), or its version is
 * not supported ({@code unsupported_version}), it is too large or too deep
 * ({@code limit_exceeded}), or a recorded decision does not belong to the rule
 * set it is read with ({@code decision_mismatch}).
 */
public class FormatException extends CleanRulesException {
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
    public FormatException(String code, String message, String hint, String json, Throwable cause) {
        super(code, message, hint, json, cause);
    }
}
