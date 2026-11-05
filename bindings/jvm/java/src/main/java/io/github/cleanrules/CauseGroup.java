package io.github.cleanrules;

import java.util.List;
import java.util.Objects;

/**
 * Requirements that must all hold ({@link GroupKind#ALL}) or any of which
 * suffices ({@link GroupKind#ANY}). The engine makes these.
 */
public final class CauseGroup implements Requirement {
    private final GroupKind kind;
    private final List<Requirement> items;

    CauseGroup(GroupKind kind, List<Requirement> items) {
        this.kind = kind;
        this.items = List.copyOf(items);
    }

    /**
     * How the requirements combine.
     *
     * @return the kind
     */
    public GroupKind kind() {
        return kind;
    }

    /**
     * The requirements, in order.
     *
     * @return the requirements
     */
    public List<Requirement> items() {
        return items;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof CauseGroup group && kind == group.kind && items.equals(group.items);
    }

    @Override
    public int hashCode() {
        return Objects.hash(kind, items);
    }

    @Override
    public String toString() {
        return "CauseGroup[" + kind.jsonName() + ", " + items + "]";
    }
}
