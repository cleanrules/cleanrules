package io.github.cleanrules;

import java.util.Locale;

/** A truth value of Kleene's three-valued logic ({@code spec/ast.md}, <i>Truth values</i>). */
public enum Truth {
    /** True. */
    TRUE,
    /** False. */
    FALSE,
    /** Unknown: a missing field, a null or a type mismatch. */
    UNKNOWN;

    /**
     * The name in documents and in the JSON forms, such as {@code unknown}.
     *
     * @return the name
     */
    public String jsonName() {
        return name().toLowerCase(Locale.ROOT);
    }

    /**
     * The constant named {@code name} in documents, as {@link #jsonName()} gives it.
     *
     * @param name the name
     * @return the constant
     * @throws IllegalArgumentException if no constant has that name
     */
    public static Truth fromJsonName(String name) {
        for (Truth constant : values()) {
            if (constant.jsonName().equals(name)) {
                return constant;
            }
        }
        throw new IllegalArgumentException("no Truth is named " + name);
    }
}
