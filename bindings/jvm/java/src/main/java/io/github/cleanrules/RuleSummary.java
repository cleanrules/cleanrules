package io.github.cleanrules;

import java.util.List;
import java.util.Objects;
import java.util.Optional;

/** A rule in a {@link Summary}. The engine makes these. */
public final class RuleSummary {
    private final String name;
    private final long priority;
    private final List<String> outputs;
    private final boolean action;
    private final String actionName;

    RuleSummary(String name, long priority, List<String> outputs, boolean action, String actionName) {
        this.name = name;
        this.priority = priority;
        this.outputs = List.copyOf(outputs);
        this.action = action;
        this.actionName = actionName;
    }

    /**
     * The rule's name.
     *
     * @return the name
     */
    public String name() {
        return name;
    }

    /**
     * The rule's priority.
     *
     * @return the priority
     */
    public long priority() {
        return priority;
    }

    /**
     * The payload keys, in order.
     *
     * @return the keys
     */
    public List<String> outputs() {
        return outputs;
    }

    /**
     * Whether the rule has an action, code or a name.
     *
     * @return whether it has one
     */
    public boolean action() {
        return action;
    }

    /**
     * The action's name, when the rule names it.
     *
     * @return the name, or empty
     */
    public Optional<String> actionName() {
        return Optional.ofNullable(actionName);
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof RuleSummary rule
                && name.equals(rule.name)
                && priority == rule.priority
                && outputs.equals(rule.outputs)
                && action == rule.action
                && Objects.equals(actionName, rule.actionName);
    }

    @Override
    public int hashCode() {
        return Objects.hash(name, priority, outputs, action, actionName);
    }

    @Override
    public String toString() {
        return "RuleSummary[" + name + ", priority=" + priority + ", outputs=" + outputs + ", action=" + action
                + ", actionName=" + actionName + "]";
    }
}
