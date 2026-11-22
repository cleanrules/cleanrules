package io.github.cleanrules;

import java.util.Locale;

/** What a document holds. */
public enum DocumentKind {
    /** A condition. */
    CONDITION,
    /** A rule. */
    RULE,
    /** A rule set. */
    RULESET,
    /** A decision table ({@code spec/tables.md}). */
    DECISION_TABLE,
    /**
     * A bundle ({@code spec/bundles.md}): read, written and hashed through
     * {@code ffi.Document}; the idiomatic API has no bundles yet.
     */
    BUNDLE,
    /**
     * A case file of test cases ({@code spec/tests.md}): read, written and
     * hashed through {@code ffi.Document}; the idiomatic API has no case
     * files yet.
     */
    CASES;

    /**
     * The name in documents and in the JSON forms, such as {@code decision_table}.
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
    public static DocumentKind fromJsonName(String name) {
        for (DocumentKind constant : values()) {
            if (constant.jsonName().equals(name)) {
                return constant;
            }
        }
        throw new IllegalArgumentException("no DocumentKind is named " + name);
    }
}
