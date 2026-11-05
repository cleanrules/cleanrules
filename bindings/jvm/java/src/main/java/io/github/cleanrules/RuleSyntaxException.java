package io.github.cleanrules;

import java.io.Serial;

/**
 * A text in the rule syntax cannot be parsed ({@code rule_syntax_error}).
 */
public class RuleSyntaxException extends CleanRulesException {
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
    public RuleSyntaxException(String code, String message, String hint, String json, Throwable cause) {
        super(code, message, hint, json, cause);
    }
}
