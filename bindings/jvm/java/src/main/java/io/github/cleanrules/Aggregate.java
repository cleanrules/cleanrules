package io.github.cleanrules;

import java.util.Locale;

/** How a {@link HitPolicy#COLLECT} rule set aggregates its outputs ({@code spec/rules.md}, <i>Outputs</i>). */
public enum Aggregate {
    /** The sum of the outputs. */
    SUM,
    /** The smallest output. */
    MIN,
    /** The largest output. */
    MAX,
    /** The number of selected rules. */
    COUNT;

    /**
     * The name in documents and in the JSON forms, such as {@code sum}.
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
    public static Aggregate fromJsonName(String name) {
        for (Aggregate constant : values()) {
            if (constant.jsonName().equals(name)) {
                return constant;
            }
        }
        throw new IllegalArgumentException("no Aggregate is named " + name);
    }
}
