package io.github.cleanrules;

import java.io.Serial;

/**
 * A name is not registered: a logic, or an action of a rule
 * ({@code registry_error}).
 */
public class RegistryException extends CleanRulesException {
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
    public RegistryException(String code, String message, String hint, String json, Throwable cause) {
        super(code, message, hint, json, cause);
    }
}
