package io.github.cleanrules;

import java.util.Locale;

/** How the requirements of a {@link CauseGroup} combine. */
public enum GroupKind {
    /** All of them must hold. */
    ALL,
    /** Any of them suffices. */
    ANY;

    /**
     * The name in documents: {@code all} or {@code any}.
     *
     * @return the name
     */
    public String jsonName() {
        return name().toLowerCase(Locale.ROOT);
    }
}
