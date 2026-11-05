package io.github.cleanrules;

import java.util.Locale;

/** Where a node may appear ({@code spec/ast.md}, <i>Node kinds</i>). */
public enum Kind {
    /** A condition: evaluates to a truth value. */
    CONDITION,
    /** A value: evaluates to a value. */
    VALUE,
    /** Either, depending on the data: a field, or a boolean constant. */
    ANY;

    /**
     * The name the core gives, such as {@code condition}.
     *
     * @return the name
     */
    public String jsonName() {
        return name().toLowerCase(Locale.ROOT);
    }

    static Kind fromJsonName(String name) {
        return valueOf(name.toUpperCase(Locale.ROOT));
    }
}
