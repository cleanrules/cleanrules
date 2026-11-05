package io.github.cleanrules;

import java.util.List;
import java.util.Objects;

/** A field a rule set reads, and where ({@code spec/explain.md}, <i>Summaries</i>). The engine makes these. */
public final class FieldUsage {
    /**
     * A payload value that reads the field.
     *
     * @param rule the rule's name
     * @param key the payload key
     */
    public record Payload(String rule, String key) {}

    private final List<Object> path;
    private final List<String> conditions;
    private final List<Payload> payloads;

    FieldUsage(List<Object> path, List<String> conditions, List<Payload> payloads) {
        this.path = List.copyOf(path);
        this.conditions = List.copyOf(conditions);
        this.payloads = List.copyOf(payloads);
    }

    /**
     * The field's path.
     *
     * @return the path: {@link String} keys and {@link Integer} indexes
     */
    public List<Object> path() {
        return path;
    }

    /**
     * The rules whose conditions read it, in rule order.
     *
     * @return the rule names
     */
    public List<String> conditions() {
        return conditions;
    }

    /**
     * The payload values that read it, in order.
     *
     * @return the payload values
     */
    public List<Payload> payloads() {
        return payloads;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof FieldUsage usage
                && path.equals(usage.path)
                && conditions.equals(usage.conditions)
                && payloads.equals(usage.payloads);
    }

    @Override
    public int hashCode() {
        return Objects.hash(path, conditions, payloads);
    }

    @Override
    public String toString() {
        return "FieldUsage[" + path + ", conditions=" + conditions + ", payloads=" + payloads + "]";
    }
}
