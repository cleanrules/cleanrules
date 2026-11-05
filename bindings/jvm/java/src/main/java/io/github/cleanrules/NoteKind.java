package io.github.cleanrules;

import java.util.Locale;

/** Why a rule that fired was not selected ({@code spec/explain.md}, <i>Notes</i>). */
public enum NoteKind {
    /** Another rule has a higher priority. */
    LOWER_PRIORITY,
    /** Another rule has the same priority and comes first. */
    SAME_PRIORITY,
    /** Under {@link HitPolicy#ANY}, the rules agree and the first is selected. */
    ANY_AGREES,
    /** An earlier rule fired first. */
    EARLIER_RULE;

    /**
     * The name in documents, such as {@code lower_priority}.
     *
     * @return the name
     */
    public String jsonName() {
        return name().toLowerCase(Locale.ROOT);
    }

    static NoteKind fromJsonName(String name) {
        return valueOf(name.toUpperCase(Locale.ROOT));
    }
}
