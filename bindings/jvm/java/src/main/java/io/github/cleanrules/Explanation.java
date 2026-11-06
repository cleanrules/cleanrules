package io.github.cleanrules;

import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import java.time.Duration;
import java.time.Period;
import java.time.temporal.ChronoUnit;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.OptionalLong;
import java.util.function.Supplier;

/**
 * The explanation of a {@link Decision} ({@code spec/explain.md}): each rule's
 * status and what it needed, built by the core. {@link #toString()} is its
 * text, the same in every language, and {@link #toJson()} its JSON document.
 */
public final class Explanation {
    private final Decision decision;
    private final List<RuleExplanation> rules;
    private final List<String> selected;
    private final Supplier<String> text;

    /**
     * Read from the core's explanation value ({@code cr_decision_explain_value}):
     * the explanation document's structure, with {@code text} and {@code holds}.
     */
    Explanation(Decision decision, Map<String, Object> value, Supplier<String> text) {
        this.decision = decision;
        this.text = text;
        List<RuleExplanation> entries = new ArrayList<>();
        for (Object item : Read.list(value.get("rules"))) {
            Map<String, Object> entry = Read.map(item);
            entries.add(new RuleExplanation(
                    (String) entry.get("rule"),
                    Status.fromJsonName((String) entry.get("status")),
                    requirement(Read.map(entry.get("requirement"))),
                    note(entry.get("note")),
                    (Boolean) entry.get("achievable"),
                    (String) entry.get("text")));
        }
        this.rules = Collections.unmodifiableList(entries);
        this.selected = Read.strings(value.get("selected"));
    }

    private static Requirement requirement(Map<String, Object> value) {
        for (GroupKind kind : GroupKind.values()) {
            if (value.get(kind.jsonName()) instanceof List<?> group) {
                List<Requirement> items = new ArrayList<>();
                for (Object item : group) {
                    items.add(requirement(Read.map(item)));
                }
                return new CauseGroup(kind, items);
            }
        }
        List<Cause.FieldValue> values = new ArrayList<>();
        for (Object item : Read.list(value.get("values"))) {
            Map<String, Object> read = Read.map(item);
            values.add(new Cause.FieldValue(Read.path(read.get("path")), read.get("value")));
        }
        Object truth = value.get("truth");
        return new Cause(
                (String) value.get("condition"),
                truth == null ? Truth.UNKNOWN : (Boolean) truth ? Truth.TRUE : Truth.FALSE,
                (Boolean) value.get("negated"),
                (Boolean) value.get("holds"),
                values,
                Read.reason(value.get("reason")),
                counterfactual(value.get("counterfactual")),
                (String) value.get("text"));
    }

    private static Counterfactual counterfactual(Object value) {
        if (value == null) {
            return null;
        }
        Map<String, Object> counterfactual = Read.map(value);
        Object gap = counterfactual.get("gap");
        if (gap instanceof Map<?, ?> distance) {
            gap = distance.get("days") instanceof Long days
                    ? Period.ofDays(Math.toIntExact(days))
                    : Duration.of((Long) distance.get("microseconds"), ChronoUnit.MICROS);
        }
        return new Counterfactual(
                Read.path(counterfactual.get("path")),
                (String) counterfactual.get("op"),
                counterfactual.get("target"),
                gap);
    }

    private static Note note(Object value) {
        if (value == null) {
            return null;
        }
        Map<String, Object> note = Read.map(value);
        return new Note(
                NoteKind.fromJsonName((String) note.get("kind")),
                (String) note.get("winner"),
                optional(note.get("priority")),
                optional(note.get("winner_priority")));
    }

    private static OptionalLong optional(Object value) {
        return value == null ? OptionalLong.empty() : OptionalLong.of((Long) value);
    }

    /**
     * Each rule's explanation, in rule order.
     *
     * @return the rule explanations
     */
    public List<RuleExplanation> rules() {
        return rules;
    }

    /**
     * The names of the selected rules, in rule order.
     *
     * @return the names
     */
    public List<String> selected() {
        return selected;
    }

    /**
     * One rule's explanation.
     *
     * @param name the rule's name
     * @return its explanation
     * @throws RuleNotFoundException if the rule set has no rule with that name
     */
    public RuleExplanation forRule(String name) {
        return rules.get(decision.ruleSet().position(name));
    }

    /**
     * The explanation document of the JSON format ({@code spec/json-format.md},
     * <i>Explanations</i>), whose {@code at} pointers point into
     * {@link Decision#toJson()}.
     *
     * @return the JSON text
     */
    public String toJson() {
        return decision.withNative(pointer ->
                Scope.text((out, len, error) -> Native.decisionExplain(pointer, 1, out, len, error)));
    }

    /**
     * The explanation as text, the same in every language ({@code spec/explain.md}, <i>Text</i>).
     *
     * @return the text
     */
    @Override
    public String toString() {
        return text.get();
    }
}
