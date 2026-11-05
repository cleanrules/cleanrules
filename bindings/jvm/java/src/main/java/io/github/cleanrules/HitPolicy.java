package io.github.cleanrules;

import java.util.Locale;

/** How a rule set selects among the rules that fired ({@code spec/rules.md}, <i>Evaluation</i>). */
public enum HitPolicy {
    /** The first fired rule. */
    FIRST,
    /** The fired rule with the highest priority; ties go to rule order. */
    PRIORITY,
    /** The fired rule; more than one is an error. */
    UNIQUE,
    /** The first fired rule; fired rules with different payloads are an error. */
    ANY,
    /** Every fired rule, in rule order. */
    RULE_ORDER,
    /** Every fired rule, in rule order, optionally aggregated. */
    COLLECT;

    /**
     * The name in documents and in the JSON forms, such as {@code rule_order}.
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
    public static HitPolicy fromJsonName(String name) {
        for (HitPolicy constant : values()) {
            if (constant.jsonName().equals(name)) {
                return constant;
            }
        }
        throw new IllegalArgumentException("no HitPolicy is named " + name);
    }
}
