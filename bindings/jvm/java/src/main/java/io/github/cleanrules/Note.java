package io.github.cleanrules;

import java.util.Objects;
import java.util.OptionalLong;

/** Why a rule that fired was not selected ({@code spec/explain.md}, <i>Notes</i>). The engine makes these. */
public final class Note {
    private final NoteKind kind;
    private final String winner;
    private final OptionalLong priority;
    private final OptionalLong winnerPriority;

    Note(NoteKind kind, String winner, OptionalLong priority, OptionalLong winnerPriority) {
        this.kind = kind;
        this.winner = winner;
        this.priority = priority;
        this.winnerPriority = winnerPriority;
    }

    /**
     * Why the rule was not selected.
     *
     * @return the kind
     */
    public NoteKind kind() {
        return kind;
    }

    /**
     * The rule selected instead.
     *
     * @return its name
     */
    public String winner() {
        return winner;
    }

    /**
     * This rule's priority, under {@link HitPolicy#PRIORITY}.
     *
     * @return the priority, or empty
     */
    public OptionalLong priority() {
        return priority;
    }

    /**
     * The winner's priority, under {@link HitPolicy#PRIORITY}.
     *
     * @return the priority, or empty
     */
    public OptionalLong winnerPriority() {
        return winnerPriority;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof Note note
                && kind == note.kind
                && winner.equals(note.winner)
                && priority.equals(note.priority)
                && winnerPriority.equals(note.winnerPriority);
    }

    @Override
    public int hashCode() {
        return Objects.hash(kind, winner, priority, winnerPriority);
    }

    @Override
    public String toString() {
        return "Note[" + kind.jsonName() + ", winner=" + winner + ", priority=" + priority + ", winnerPriority="
                + winnerPriority + "]";
    }
}
