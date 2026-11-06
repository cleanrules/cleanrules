package io.github.cleanrules;

import java.util.List;
import java.util.Objects;
import java.util.Optional;
import java.util.OptionalLong;

/** A row of a {@link DecisionTable}: one rule. The engine makes these. */
public final class TableRow {
    private final String name;
    private final Long priority;
    private final String description;
    private final List<Optional<Expr>> inputs;
    private final List<Optional<Expr>> outputs;

    TableRow(String name, Long priority, String description, List<Optional<Expr>> inputs,
            List<Optional<Expr>> outputs) {
        this.name = name;
        this.priority = priority;
        this.description = description;
        this.inputs = List.copyOf(inputs);
        this.outputs = List.copyOf(outputs);
    }

    /**
     * The rule's name; a row without one is the rule {@code row_<n>}, counting from 1.
     *
     * @return the name, or empty
     */
    public Optional<String> name() {
        return Optional.ofNullable(name);
    }

    /**
     * The rule's own priority.
     *
     * @return the priority, or empty
     */
    public OptionalLong priority() {
        return priority == null ? OptionalLong.empty() : OptionalLong.of(priority);
    }

    /**
     * The rule's description.
     *
     * @return the description, or empty
     */
    public Optional<String> description() {
        return Optional.ofNullable(description);
    }

    /**
     * One condition per input, testing that input ({@code age < 18}); empty for any value.
     *
     * @return the input cells, unmodifiable
     */
    public List<Optional<Expr>> inputs() {
        return inputs;
    }

    /**
     * One value per output; empty for no value.
     *
     * @return the output cells, unmodifiable
     */
    public List<Optional<Expr>> outputs() {
        return outputs;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof TableRow row
                && Objects.equals(name, row.name)
                && Objects.equals(priority, row.priority)
                && Objects.equals(description, row.description)
                && inputs.equals(row.inputs)
                && outputs.equals(row.outputs);
    }

    @Override
    public int hashCode() {
        return Objects.hash(name, priority, description, inputs, outputs);
    }

    @Override
    public String toString() {
        return "TableRow[" + name + ", priority=" + priority + ", description=" + description + ", inputs="
                + inputs + ", outputs=" + outputs + "]";
    }
}
