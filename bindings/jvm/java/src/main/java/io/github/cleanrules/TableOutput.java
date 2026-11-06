package io.github.cleanrules;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Objects;

/**
 * An output column of a decision table: its payload key and its output
 * values, highest priority first; no values for any value. In a header,
 * {@code TableOutput.of("risk", "high", "low")} is the column
 * {@code THEN risk IN ('high', 'low')}: the values are Java constants, not
 * text. A table gives its values back as {@code spec/data-model.md} says
 * (<i>Values given back</i>).
 *
 * @param name the payload key
 * @param values the output values, highest priority first, unmodifiable
 */
public record TableOutput(String name, List<Object> values) {
    /**
     * An output column.
     *
     * @param name the payload key
     * @param values the output values, highest priority first; copied
     */
    public TableOutput {
        Objects.requireNonNull(name, "name");
        values = Collections.unmodifiableList(new ArrayList<>(Objects.requireNonNull(values, "values")));
    }

    /**
     * An output column whose cells may hold any value.
     *
     * @param name the payload key
     */
    public TableOutput(String name) {
        this(name, List.of());
    }

    /**
     * An output column with its output values.
     *
     * @param name the payload key
     * @param values the output values, highest priority first
     * @return the column
     */
    public static TableOutput of(String name, Object... values) {
        return new TableOutput(name, Arrays.asList(values));
    }
}
