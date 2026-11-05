package io.github.cleanrules;

import java.io.Serial;

/**
 * A rule is looked up by a name the rule set does not have
 * ({@code rule_not_found}).
 */
public class RuleNotFoundException extends CleanRulesException {
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
    public RuleNotFoundException(String code, String message, String hint, String json, Throwable cause) {
        super(code, message, hint, json, cause);
    }
}
