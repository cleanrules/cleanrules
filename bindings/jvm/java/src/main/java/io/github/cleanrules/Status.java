package io.github.cleanrules;

import java.util.Locale;

/** What happened to a rule in a decision ({@code spec/explain.md}, <i>Rule status</i>). */
public enum Status {
    /** The rule fired and the hit policy selected it. */
    SELECTED,
    /** The rule fired but was not selected; its {@linkplain RuleExplanation#note() note} says why. */
    FIRED,
    /** The rule's condition was false. */
    NOT_FIRED,
    /** The rule's condition was unknown: data was missing or of another type. */
    UNKNOWN;

    /**
     * The name in documents, such as {@code not_fired}.
     *
     * @return the name
     */
    public String jsonName() {
        return name().toLowerCase(Locale.ROOT);
    }

    static Status fromJsonName(String name) {
        return valueOf(name.toUpperCase(Locale.ROOT));
    }
}
