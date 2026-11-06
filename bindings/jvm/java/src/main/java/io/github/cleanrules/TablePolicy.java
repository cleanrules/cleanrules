package io.github.cleanrules;

import java.util.Locale;

/**
 * A decision table's hit policy ({@code spec/tables.md}): those of
 * {@link HitPolicy} and {@link #OUTPUT_ORDER}.
 */
public enum TablePolicy {
    /** The first row that fires. */
    FIRST,
    /** The row with the highest output priority; ties go to row order. */
    PRIORITY,
    /** The only row that fires; more than one is a {@link HitPolicyException}. */
    UNIQUE,
    /** The first row that fires; the rows that fire must agree on their outputs. */
    ANY,
    /** Every row that fires, in row order. */
    RULE_ORDER,
    /** Every row that fires, in output priority order (DMN's O). */
    OUTPUT_ORDER,
    /** Every row that fires, in row order, optionally combined by an {@link Aggregate}. */
    COLLECT;

    /**
     * The name in documents, such as {@code output_order}.
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
    public static TablePolicy fromJsonName(String name) {
        for (TablePolicy constant : values()) {
            if (constant.jsonName().equals(name)) {
                return constant;
            }
        }
        throw new IllegalArgumentException("no TablePolicy is named " + name);
    }
}
