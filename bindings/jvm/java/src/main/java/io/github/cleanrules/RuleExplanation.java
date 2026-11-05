package io.github.cleanrules;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Optional;

/**
 * The explanation of one rule ({@code spec/explain.md}). The engine makes
 * these; {@link #toString()} is the rule's part of the explanation's text.
 */
public final class RuleExplanation {
    private final String rule;
    private final Status status;
    private final Requirement requirement;
    private final Note note;
    private final Boolean achievable;
    private final String text;

    RuleExplanation(String rule, Status status, Requirement requirement, Note note, Boolean achievable, String text) {
        this.rule = rule;
        this.status = status;
        this.requirement = requirement;
        this.note = note;
        this.achievable = achievable;
        this.text = text;
    }

    /**
     * The rule's name.
     *
     * @return the name
     */
    public String rule() {
        return rule;
    }

    /**
     * What happened to the rule.
     *
     * @return the status
     */
    public Status status() {
        return status;
    }

    /**
     * What the rule needed: a cause, or a group of them.
     *
     * @return the requirement
     */
    public Requirement requirement() {
        return requirement;
    }

    /**
     * Why the rule was not selected, when it fired.
     *
     * @return the note, or empty
     */
    public Optional<Note> note() {
        return Optional.ofNullable(note);
    }

    /**
     * When the rule did not fire, whether changing the listed conditions can make it fire.
     *
     * @return whether it is achievable, or empty
     */
    public Optional<Boolean> achievable() {
        return Optional.ofNullable(achievable);
    }

    /**
     * The causes of the requirement, in order, without their grouping.
     *
     * @return the causes
     */
    public List<Cause> causes() {
        List<Cause> found = new ArrayList<>();
        ArrayDeque<Requirement> stack = new ArrayDeque<>();
        stack.push(requirement);
        while (!stack.isEmpty()) {
            switch (stack.pop()) {
                case Cause cause -> found.add(cause);
                case CauseGroup group -> {
                    for (int i = group.items().size() - 1; i >= 0; i--) {
                        stack.push(group.items().get(i));
                    }
                }
            }
        }
        return Collections.unmodifiableList(found);
    }

    /**
     * The rule's part of the explanation's text ({@code spec/explain.md}, <i>Text</i>).
     *
     * @return the text
     */
    @Override
    public String toString() {
        return text;
    }
}
