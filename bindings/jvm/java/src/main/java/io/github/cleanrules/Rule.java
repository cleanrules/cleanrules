package io.github.cleanrules;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.internal.Build;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import io.github.cleanrules.internal.Values;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.SequencedMap;
import java.util.function.Function;

/**
 * A rule ({@code spec/rules.md}, <i>Rules</i>): a name, a condition, a payload,
 * and a priority, tags, a description, metadata and an action.
 *
 * <pre>{@code
 * Rule vip = Rule.named("vip")
 *         .when(field("customer", "years").gt(5))
 *         .then("discount", new BigDecimal("0.15"))
 *         .priority(10)
 *         .tags("loyalty")
 *         .action("notify_sales");
 * }</pre>
 *
 * <p>Rules are immutable: each step gives a new rule, which the core checks
 * (an empty name, a value as the condition, a condition in the payload, an
 * empty action name are a {@link RuleDefinitionException}). Payload values are
 * nodes, or Java constants as in {@link Expr}. Rules compare by identity, and
 * may be shared between threads.
 */
public final class Rule implements DocumentContent {
    /** The core's rule, shared by the rules that differ only in their action's code. */
    private static final class Core {
        private final Handle handle;

        Core(MemorySegment pointer) {
            handle = Handle.own(this, pointer, Native::ruleFree, "Rule");
        }

        <T> T with(Function<MemorySegment, T> call) {
            MemorySegment pointer = handle.acquire();
            try {
                return call.apply(pointer);
            } finally {
                handle.release();
            }
        }
    }

    private final Core core;
    private final String name;
    private final Expr when;
    private final SequencedMap<String, Expr> then;
    private final long priority;
    private final String description;
    private final String actionName;
    private final Action action;

    private Rule(
            Core core,
            String name,
            Expr when,
            SequencedMap<String, Expr> then,
            long priority,
            String description,
            String actionName,
            Action action) {
        this.core = core;
        this.name = name;
        this.when = when;
        this.then = then;
        this.priority = priority;
        this.description = description;
        this.actionName = actionName;
        this.action = action;
    }

    /** The first step of a rule: its name, then {@link #when(Expr)}. */
    public static final class Named {
        private final String name;

        private Named(String name) {
            this.name = name;
        }

        /**
         * The rule with this condition: it fires when the condition is true.
         *
         * @param condition the condition
         * @return the rule, with no payload, priority 0 and no action
         * @throws RuleDefinitionException for an empty name, or a value as the condition
         * @throws ExpressionException for the null constant as the condition
         */
        public Rule when(Expr condition) {
            Objects.requireNonNull(condition, "condition");
            byte[] bytes = Text.name(name);
            MemorySegment made = condition.with(pointer ->
                    step((out, error, scope) -> Native.ruleNew(scope.bytes(bytes), bytes.length, pointer, out, error)));
            return new Rule(new Core(made), name, condition, new LinkedHashMap<>(), 0, null, null, null);
        }
    }

    /**
     * Start a rule with its name, unique within a rule set.
     *
     * @param name the name
     * @return the first step: give it a condition with {@link Named#when(Expr)}
     */
    public static Named named(String name) {
        return new Named(Objects.requireNonNull(name, "name"));
    }

    @FunctionalInterface
    private interface Step {
        int call(MemorySegment out, MemorySegment error, Scope scope);
    }

    /** The owned {@code cr_rule *} a constructor gives. */
    private static MemorySegment step(Step call) {
        Native.ensureLoaded();
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(call.call(out, scope.error(), scope));
            return out.get(ADDRESS, 0);
        }
    }

    @FunctionalInterface
    private interface RuleStep {
        int call(MemorySegment rule, MemorySegment out, MemorySegment error, Scope scope);
    }

    private MemorySegment next(RuleStep call) {
        return core.with(pointer -> step((out, error, scope) -> call.call(pointer, out, error, scope)));
    }

    private Rule copy(MemorySegment made, SequencedMap<String, Expr> payload, long newPriority,
            String newDescription, String newActionName, Action newAction) {
        return new Rule(new Core(made), name, when, payload, newPriority, newDescription, newActionName, newAction);
    }

    // ---- Steps.

    /**
     * This rule with {@code key} in its payload: a new key goes last, a key it
     * has keeps its place and takes the new value.
     *
     * @param key the payload key
     * @param value a value node, such as {@code field("order", "total").mul(0.1)}, or a Java constant
     * @return the new rule
     * @throws RuleDefinitionException if the value is a condition
     * @throws ExpressionException if the value is not a constant (a NaN...)
     */
    public Rule then(String key, Object value) {
        Objects.requireNonNull(key, "key");
        Expr node = Expr.lift(value);
        byte[] bytes = Text.name(key);
        MemorySegment made = node.with(nodePointer -> next((rule, out, error, scope) ->
                Native.ruleWithPayload(rule, scope.bytes(bytes), bytes.length, nodePointer, out, error)));
        SequencedMap<String, Expr> payload = new LinkedHashMap<>(then);
        payload.put(key, node);
        return copy(made, payload, priority, description, actionName, action);
    }

    /**
     * This rule with each entry of {@code payload} added as
     * {@link #then(String, Object)} does: in the map's order for a
     * {@link SequencedMap}, otherwise sorted by key.
     *
     * @param payload the keys and values
     * @return the new rule
     * @throws RuleDefinitionException if a value is a condition
     */
    public Rule then(Map<String, ?> payload) {
        Objects.requireNonNull(payload, "payload");
        List<String> keys = new ArrayList<>(payload.keySet());
        if (!(payload instanceof SequencedMap<?, ?>)) {
            keys.sort(Rule::byCodePoint);
        }
        Rule rule = this;
        for (String key : keys) {
            rule = rule.then(key, payload.get(key));
        }
        return rule;
    }

    /** Strings ordered by Unicode code point, as the core orders them. */
    static int byCodePoint(String left, String right) {
        return java.util.Arrays.compare(left.codePoints().toArray(), right.codePoints().toArray());
    }

    /**
     * This rule with a priority, for the {@link HitPolicy#PRIORITY} hit policy: higher wins.
     *
     * @param priority the priority
     * @return the new rule
     */
    public Rule priority(long priority) {
        MemorySegment made = next((rule, out, error, scope) -> Native.ruleWithPriority(rule, priority, out, error));
        return copy(made, then, priority, description, actionName, action);
    }

    /**
     * This rule with these tags instead of its own, kept sorted and without repeats.
     *
     * @param tags the tags
     * @return the new rule
     */
    public Rule tags(String... tags) {
        return tags(List.of(tags));
    }

    /**
     * This rule with these tags instead of its own, kept sorted and without repeats.
     *
     * @param tags the tags
     * @return the new rule
     */
    public Rule tags(Collection<String> tags) {
        MemorySegment list = Build.data(List.copyOf(tags));
        try {
            MemorySegment made = next((rule, out, error, scope) -> Native.ruleWithTags(rule, list, out, error));
            return copy(made, then, priority, description, actionName, action);
        } finally {
            Native.valueFree(list);
        }
    }

    /**
     * This rule with a description.
     *
     * @param description the description
     * @return the new rule
     */
    public Rule description(String description) {
        byte[] bytes = Text.name(Objects.requireNonNull(description, "description"));
        MemorySegment made = next((rule, out, error, scope) ->
                Native.ruleWithDescription(rule, scope.bytes(bytes), bytes.length, out, error));
        return copy(made, then, priority, description, actionName, action);
    }

    /**
     * This rule with this metadata instead of its own: free-form data (an
     * owner, a ticket...), read as data ({@code spec/data-model.md}).
     *
     * @param metadata the metadata
     * @return the new rule
     * @throws RuleDefinitionException if a value is not data (an opaque object, an integer beyond 64 bits...)
     * @throws FormatException with the code {@code limit_exceeded} for metadata of more
     *     than the default {@code maxValues} values, naming this rule
     */
    public Rule metadata(Map<String, ?> metadata) {
        MemorySegment map = Build.data(Objects.requireNonNull(metadata, "metadata"), name());
        try {
            MemorySegment made = next((rule, out, error, scope) -> Native.ruleWithMetadata(rule, map, out, error));
            return copy(made, then, priority, description, actionName, action);
        } finally {
            Native.valueFree(map);
        }
    }

    /**
     * This rule naming its action, which {@link RuleSet#bind(Map)} resolves.
     * A named action is data, written with the rule; the code it had, if any,
     * is dropped.
     *
     * @param name the action's name
     * @return the new rule
     * @throws RuleDefinitionException for an empty name
     */
    public Rule action(String name) {
        byte[] bytes = Text.name(Objects.requireNonNull(name, "name"));
        MemorySegment made = next((rule, out, error, scope) ->
                Native.ruleWithAction(rule, scope.bytes(bytes), bytes.length, out, error));
        return copy(made, then, priority, description, name, null);
    }

    /**
     * This rule with code as its action, which {@link RuleSet#run(Object)}
     * calls when the rule is selected. A rule that names its action keeps the
     * name, and this code is bound to it.
     *
     * @param action the code
     * @return the new rule
     */
    public Rule action(Action action) {
        return new Rule(core, name, when, then, priority, description, actionName,
                Objects.requireNonNull(action, "action"));
    }

    // ---- What the rule holds.

    /**
     * The name.
     *
     * @return the name
     */
    public String name() {
        return name;
    }

    /**
     * The condition.
     *
     * @return the condition
     */
    public Expr when() {
        return when;
    }

    /**
     * The payload, in order; empty when the rule has none.
     *
     * @return the keys and their value nodes, unmodifiable
     */
    public SequencedMap<String, Expr> then() {
        return Collections.unmodifiableSequencedMap(then);
    }

    /**
     * The priority.
     *
     * @return the priority
     */
    public long priority() {
        return priority;
    }

    /**
     * The tags, sorted and without repeats, as the core keeps them.
     *
     * @return the tags, unmodifiable
     */
    @SuppressWarnings("unchecked") // The core gives a list of strings.
    public List<String> tags() {
        return (List<String>) core.with(pointer ->
                Values.take(Scope.value((out, error) -> Native.ruleTags(pointer, out, error)), List.of()));
    }

    /**
     * The description.
     *
     * @return the description, or empty
     */
    public Optional<String> description() {
        return Optional.ofNullable(description);
    }

    /**
     * The metadata, as the core keeps it ({@code spec/data-model.md}, <i>Values given back</i>).
     *
     * @return the metadata, unmodifiable
     */
    @SuppressWarnings("unchecked") // The core gives a map.
    public SequencedMap<String, Object> metadata() {
        return (SequencedMap<String, Object>) core.with(pointer ->
                Values.take(Scope.value((out, error) -> Native.ruleMetadata(pointer, out, error)), List.of()));
    }

    /**
     * The name of the rule's action, if it names one (bound or not).
     *
     * @return the name, or empty
     */
    public Optional<String> actionName() {
        return Optional.ofNullable(actionName);
    }

    /**
     * The action's code: given to the rule, or bound to its name; empty if none.
     *
     * @return the code, or empty
     */
    public Optional<Action> action() {
        return Optional.ofNullable(action);
    }

    @FunctionalInterface
    private interface Lend {
        int call(MemorySegment text, MemorySegment len, MemorySegment error);
    }

    /** A text the core lends, copied; null when it lends none. */
    private static String lend(Lend call) {
        try (Scope scope = new Scope()) {
            MemorySegment text = scope.pointer();
            MemorySegment len = scope.size();
            scope.check(call.call(text, len, scope.error()));
            return text.get(ADDRESS, 0).equals(MemorySegment.NULL) ? null : Scope.borrowedText(text, len);
        }
    }

    /**
     * The rule of an owned {@code cr_rule *}, read from a document or a rule
     * set: it keeps the core's own rule, so it writes back exactly.
     */
    static Rule fromCore(MemorySegment owned) {
        Core core = new Core(owned);
        return core.with(pointer -> {
            String name = lend((text, len, error) -> Native.ruleName(pointer, text, len, error));
            Expr when = Expr.make((out, error, scope) -> Native.ruleWhen(pointer, out, error));
            SequencedMap<String, Expr> payload = new LinkedHashMap<>();
            long entries = Native.rulePayloadLen(pointer);
            for (long i = 0; i < entries; i++) {
                long index = i;
                String key = lend((text, len, error) -> Native.rulePayloadKey(pointer, index, text, len, error));
                payload.put(key, Expr.make((out, error, scope) -> Native.rulePayloadValue(pointer, index, out, error)));
            }
            String description = lend((text, len, error) -> Native.ruleDescription(pointer, text, len, error));
            String actionName = lend((text, len, error) -> Native.ruleAction(pointer, text, len, error));
            return new Rule(core, name, when, payload, Native.rulePriority(pointer), description, actionName, null);
        });
    }

    /** The core's rule, held until {@link #release()}. */
    MemorySegment acquire() {
        return core.handle.acquire();
    }

    /** End what {@link #acquire()} began. */
    void release() {
        core.handle.release();
    }

    /**
     * The rule's name and condition, such as {@code Rule(vip: customer.years > 5)}.
     *
     * @return the text
     */
    @Override
    public String toString() {
        return "Rule(" + name + ": " + when + ")";
    }
}
