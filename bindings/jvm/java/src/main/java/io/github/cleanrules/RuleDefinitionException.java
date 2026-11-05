package io.github.cleanrules;

import java.io.Serial;

/**
 * A rule or rule set breaks a construction rule ({@code rule_definition_error}).
 */
public class RuleDefinitionException extends CleanRulesException {
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
    public RuleDefinitionException(String code, String message, String hint, String json, Throwable cause) {
        super(code, message, hint, json, cause);
    }
}
