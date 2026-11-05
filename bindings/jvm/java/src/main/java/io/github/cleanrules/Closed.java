package io.github.cleanrules;

import java.util.Locale;

/** Which ends of {@link Expr#between(Object, Object, Closed)} are included (the names of polars). */
public enum Closed {
    /** {@code low <= x <= high}, the default. */
    BOTH,
    /** {@code low <= x < high}. */
    LEFT,
    /** {@code low < x <= high}. */
    RIGHT,
    /** {@code low < x < high}. */
    NONE;

    /**
     * The name in documents, such as {@code both}.
     *
     * @return the name
     */
    public String jsonName() {
        return name().toLowerCase(Locale.ROOT);
    }

    static Closed fromJsonName(String name) {
        return valueOf(name.toUpperCase(Locale.ROOT));
    }
}
