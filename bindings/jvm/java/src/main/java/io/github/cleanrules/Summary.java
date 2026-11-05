package io.github.cleanrules;

import io.github.cleanrules.internal.Json;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.SequencedMap;
import java.util.function.Supplier;

/**
 * What a rule set reads and produces, without evaluating it
 * ({@code spec/explain.md}, <i>Summaries</i>), read from the core's summary
 * document. {@link #toString()} is its text, the same in every language.
 */
public final class Summary {
    private final String json;
    private final Supplier<String> text;
    private final HitPolicy hitPolicy;
    private final Aggregate aggregate;
    private final String logic;
    private final boolean strict;
    private final List<RuleSummary> rules;
    private final List<FieldUsage> fields;
    private final List<String> outputs;
    private final SequencedMap<String, List<String>> tags;

    Summary(String json, Supplier<String> text) {
        this.json = json;
        this.text = text;
        Map<String, Object> data = Read.map(Json.object(json).values().get("summary"));
        hitPolicy = HitPolicy.fromJsonName((String) data.get("hit_policy"));
        aggregate = data.get("aggregate") == null ? null : Aggregate.fromJsonName((String) data.get("aggregate"));
        logic = (String) data.get("logic");
        strict = Boolean.TRUE.equals(data.get("strict"));
        List<RuleSummary> ruleList = new ArrayList<>();
        for (Object item : Read.list(data.get("rules"))) {
            Map<String, Object> rule = Read.map(item);
            ruleList.add(new RuleSummary(
                    (String) rule.get("name"),
                    Read.integer(rule.get("priority")),
                    Read.strings(rule.get("outputs")),
                    Boolean.TRUE.equals(rule.get("action")),
                    (String) rule.get("do")));
        }
        rules = Collections.unmodifiableList(ruleList);
        List<FieldUsage> usages = new ArrayList<>();
        for (Object item : Read.list(data.get("fields"))) {
            Map<String, Object> usage = Read.map(item);
            List<FieldUsage.Payload> payloads = new ArrayList<>();
            for (Object use : Read.list(usage.get("payloads"))) {
                Map<String, Object> pair = Read.map(use);
                payloads.add(new FieldUsage.Payload((String) pair.get("rule"), (String) pair.get("key")));
            }
            usages.add(new FieldUsage(
                    Read.path(usage.get("path")), Read.strings(usage.get("conditions")), payloads));
        }
        fields = Collections.unmodifiableList(usages);
        outputs = Read.strings(data.get("outputs"));
        SequencedMap<String, List<String>> tagMap = new LinkedHashMap<>();
        for (Object item : Read.list(data.get("tags"))) {
            Map<String, Object> entry = Read.map(item);
            tagMap.put((String) entry.get("tag"), Read.strings(entry.get("rules")));
        }
        tags = Collections.unmodifiableSequencedMap(tagMap);
    }

    /**
     * The hit policy.
     *
     * @return the hit policy
     */
    public HitPolicy hitPolicy() {
        return hitPolicy;
    }

    /**
     * The {@link HitPolicy#COLLECT} aggregate, if any.
     *
     * @return the aggregate, or empty
     */
    public Optional<Aggregate> aggregate() {
        return Optional.ofNullable(aggregate);
    }

    /**
     * The logic's name.
     *
     * @return the name
     */
    public String logic() {
        return logic;
    }

    /**
     * Whether evaluation is strict.
     *
     * @return whether it is strict
     */
    public boolean isStrict() {
        return strict;
    }

    /**
     * Each rule's name, priority, payload keys and action, in rule order.
     *
     * @return the rules
     */
    public List<RuleSummary> rules() {
        return rules;
    }

    /**
     * Every field read, in first-appearance order, with where it is read.
     *
     * @return the fields
     */
    public List<FieldUsage> fields() {
        return fields;
    }

    /**
     * The payload keys, in first-appearance order.
     *
     * @return the keys
     */
    public List<String> outputs() {
        return outputs;
    }

    /**
     * Each tag, sorted, with the names of its rules.
     *
     * @return the tags
     */
    public SequencedMap<String, List<String>> tags() {
        return tags;
    }

    /**
     * The summary document of the JSON format ({@code spec/json-format.md}, <i>Summaries</i>).
     *
     * @return the JSON text
     */
    public String toJson() {
        return json;
    }

    /**
     * The summary as text, the same in every language.
     *
     * @return the text
     */
    @Override
    public String toString() {
        return text.get();
    }
}
