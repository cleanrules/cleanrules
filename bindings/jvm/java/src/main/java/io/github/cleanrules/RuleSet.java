package io.github.cleanrules;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import io.github.cleanrules.internal.Bridge;
import io.github.cleanrules.internal.Build;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.function.Function;

/**
 * Rules evaluated together under a hit policy ({@code spec/rules.md}).
 *
 * <pre>{@code
 * RuleSet rules = RuleSet.builder()
 *         .add(vip, loyal)
 *         .hitPolicy(HitPolicy.PRIORITY)
 *         .build();
 * Decision decision = rules.evaluate(Map.of("customer", Map.of("years", 7)));
 * }</pre>
 *
 * <p>A rule set is an immutable, iterable list of rules, looked up by name too.
 * The core checks it (repeated names, an aggregate without
 * {@link HitPolicy#COLLECT} are a {@link RuleDefinitionException}; an unknown
 * logic a {@link RegistryException}), evaluates it, and explains its
 * decisions. Java data is read as {@link io.github.cleanrules.ffi.RuleSet}
 * reads it ({@code spec/data-model.md}, <i>Java and Scala</i>): only the
 * paths the rules read. Rule sets may be shared between threads, and hold
 * native memory, which the cleaner frees once they are unreachable.
 */
public final class RuleSet implements Iterable<Rule>, DocumentContent {
    private final io.github.cleanrules.ffi.RuleSet nativeRuleSet;
    private final List<Rule> rules;
    private final Map<String, Rule> byName;

    private RuleSet(io.github.cleanrules.ffi.RuleSet nativeRuleSet, List<Rule> rules) {
        this.nativeRuleSet = nativeRuleSet;
        this.rules = Collections.unmodifiableList(rules);
        Map<String, Rule> names = new LinkedHashMap<>();
        for (Rule rule : rules) {
            names.put(rule.name(), rule);
        }
        this.byName = names;
    }

    /**
     * The rule set of a native one, read from a document: its rules are the
     * core's own, so it writes back exactly.
     */
    static RuleSet fromNative(io.github.cleanrules.ffi.RuleSet made) {
        Handle handle = Bridge.ffi().handle(made);
        MemorySegment pointer = handle.acquire();
        try {
            long count = Native.rulesetLen(pointer);
            List<Rule> rules = new ArrayList<>(Math.toIntExact(count));
            for (long i = 0; i < count; i++) {
                long position = i;
                rules.add(Rule.fromCore(Scope.value((out, error) -> Native.rulesetRule(pointer, position, out, error))));
            }
            return new RuleSet(made, rules);
        } finally {
            handle.release();
        }
    }

    /** The native rule set, to write it. */
    io.github.cleanrules.ffi.RuleSet nativeRuleSet() {
        return nativeRuleSet;
    }

    /**
     * A rule set of these rules, with the defaults: {@link HitPolicy#FIRST},
     * no aggregate, Boolean logic, not strict.
     *
     * @param rules the rules, in order
     * @return the rule set
     * @throws RuleDefinitionException if two rules have the same name
     */
    public static RuleSet of(Rule... rules) {
        return builder().add(rules).build();
    }

    /**
     * A rule set of these rules, with the defaults.
     *
     * @param rules the rules, in order
     * @return the rule set
     * @throws RuleDefinitionException if two rules have the same name
     */
    public static RuleSet of(Collection<Rule> rules) {
        return builder().add(rules).build();
    }

    /**
     * A builder, for a rule set with a hit policy, an aggregate, a logic or strict mode.
     *
     * @return a new builder
     */
    public static Builder builder() {
        return new Builder();
    }

    /** A rule set being put together; not for several threads at once. */
    public static final class Builder {
        private final List<Rule> rules = new ArrayList<>();
        private HitPolicy hitPolicy = HitPolicy.FIRST;
        private Aggregate aggregate;
        private String logic = "boolean";
        private boolean strict;

        private Builder() {}

        /**
         * Add rules, after those added before.
         *
         * @param more the rules
         * @return this builder
         */
        public Builder add(Rule... more) {
            return add(List.of(more));
        }

        /**
         * Add rules, after those added before.
         *
         * @param more the rules
         * @return this builder
         */
        public Builder add(Collection<Rule> more) {
            for (Rule rule : more) {
                rules.add(Objects.requireNonNull(rule, "a rule is null"));
            }
            return this;
        }

        /**
         * How the rules that fired become a decision; {@link HitPolicy#FIRST} by default.
         *
         * @param hitPolicy the hit policy
         * @return this builder
         */
        public Builder hitPolicy(HitPolicy hitPolicy) {
            this.hitPolicy = Objects.requireNonNull(hitPolicy, "hitPolicy");
            return this;
        }

        /**
         * With {@link HitPolicy#COLLECT}, how the outputs combine; null for none.
         *
         * @param aggregate the aggregate, or null
         * @return this builder
         */
        public Builder aggregate(Aggregate aggregate) {
            this.aggregate = aggregate;
            return this;
        }

        /**
         * The logic, by name; {@code boolean} (Kleene's three values) by default.
         *
         * @param logic the logic's name
         * @return this builder
         */
        public Builder logic(String logic) {
            this.logic = Objects.requireNonNull(logic, "logic");
            return this;
        }

        /**
         * Whether a missing field or a type mismatch is an
         * {@link EvaluationException} instead of unknown; false by default.
         *
         * @param strict whether evaluation is strict
         * @return this builder
         */
        public Builder strict(boolean strict) {
            this.strict = strict;
            return this;
        }

        /**
         * The rule set, checked by the core.
         *
         * @return the rule set
         * @throws RuleDefinitionException for repeated names, or an aggregate without collect
         * @throws RegistryException for an unknown logic
         */
        public RuleSet build() {
            Native.ensureLoaded();
            List<Rule> list = List.copyOf(rules);
            byte[] policy = Text.name(hitPolicy.jsonName());
            byte[] combine = aggregate == null ? null : Text.name(aggregate.jsonName());
            byte[] logicName = Text.name(logic);
            MemorySegment[] pointers = new MemorySegment[list.size()];
            int held = 0;
            MemorySegment made;
            try (Scope scope = new Scope()) {
                for (; held < pointers.length; held++) {
                    pointers[held] = list.get(held).acquire();
                }
                MemorySegment out = scope.pointer();
                scope.check(Native.rulesetNew(
                        scope.pointers(pointers),
                        pointers.length,
                        scope.bytes(policy),
                        policy.length,
                        combine == null ? MemorySegment.NULL : scope.bytes(combine),
                        combine == null ? 0 : combine.length,
                        scope.bytes(logicName),
                        logicName.length,
                        strict,
                        out,
                        scope.error()));
                made = out.get(ADDRESS, 0);
            } finally {
                for (int i = 0; i < held; i++) {
                    list.get(i).release();
                }
            }
            return new RuleSet(Bridge.ffi().ruleSet(made), list);
        }
    }

    // ---- The rules.

    /**
     * The rules, in order.
     *
     * @return the rules, unmodifiable
     */
    public List<Rule> rules() {
        return rules;
    }

    /**
     * The number of rules.
     *
     * @return the number of rules
     */
    public int size() {
        return rules.size();
    }

    @Override
    public Iterator<Rule> iterator() {
        return rules.iterator();
    }

    /**
     * The rule named {@code name}, if there is one.
     *
     * @param name the name
     * @return the rule, or empty
     */
    public Optional<Rule> get(String name) {
        return Optional.ofNullable(byName.get(name));
    }

    /**
     * The rule named {@code name}.
     *
     * @param name the name
     * @return the rule
     * @throws RuleNotFoundException if there is none, with the names there are
     */
    public Rule rule(String name) {
        return rules.get(position(name));
    }

    /**
     * Whether there is a rule named {@code name}.
     *
     * @param name the name
     * @return whether there is one
     */
    public boolean contains(String name) {
        return byName.containsKey(name);
    }

    /** The position of the rule named {@code name}; the core's {@code rule_not_found} if there is none. */
    int position(String name) {
        byte[] bytes = Text.name(Objects.requireNonNull(name, "name"));
        return withNative(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.size();
                scope.check(Native.rulesetRulePosition(pointer, scope.bytes(bytes), bytes.length, out, scope.error()));
                return Math.toIntExact(out.get(JAVA_LONG, 0));
            }
        });
    }

    private <T> T withNative(Function<MemorySegment, T> call) {
        Handle handle = Bridge.ffi().handle(nativeRuleSet);
        MemorySegment pointer = handle.acquire();
        try {
            return call.apply(pointer);
        } finally {
            handle.release();
        }
    }

    /**
     * The hit policy.
     *
     * @return the hit policy
     */
    public HitPolicy hitPolicy() {
        return nativeRuleSet.hitPolicy();
    }

    /**
     * The {@link HitPolicy#COLLECT} aggregate, if any.
     *
     * @return the aggregate, or empty
     */
    public Optional<Aggregate> aggregate() {
        return nativeRuleSet.aggregate();
    }

    /**
     * The logic's name.
     *
     * @return the name
     */
    public String logic() {
        return nativeRuleSet.logic();
    }

    /**
     * Whether evaluation is strict.
     *
     * @return whether it is strict
     */
    public boolean isStrict() {
        return nativeRuleSet.isStrict();
    }

    /**
     * The paths of the input the rules read, conditions and payloads, in the
     * order of the summary; Java data is read along these only.
     *
     * @return the paths: lists of {@link String} keys and {@link Integer} indexes
     */
    public List<List<Object>> fieldPaths() {
        return nativeRuleSet.fieldPaths();
    }

    // ---- Evaluating.

    /**
     * Evaluate the rule set on one input. It never runs actions.
     *
     * @param data the input, such as a map, a record or a JavaBean
     * @return the decision
     * @throws EvaluationException in strict mode, when the data lacks a field
     *     or holds a value of another type (and {@link HitPolicyException} when
     *     the hit policy cannot decide, in any mode)
     */
    public Decision evaluate(Object data) {
        return evaluate(data, null);
    }

    /**
     * Evaluate the rule set on one input within limits.
     *
     * @param data the input
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws CleanRulesException if the evaluation fails
     */
    public Decision evaluate(Object data, Limits limits) {
        return new Decision(this, nativeRuleSet.evaluate(data, limits));
    }

    /**
     * Evaluate the rule set on each input of a batch, in order.
     *
     * @param rows the inputs
     * @return the decisions, one per input
     * @throws EvaluationException as {@link #evaluate(Object)}, with the input's position as {@code row}
     */
    public List<Decision> evaluateMany(Iterable<?> rows) {
        return evaluateMany(rows, null);
    }

    /**
     * Evaluate the rule set on each input of a batch within limits.
     *
     * @param rows the inputs
     * @param limits the limits, or null for the defaults
     * @return the decisions, one per input
     * @throws CleanRulesException if an evaluation fails, with the input's position as {@code row}
     */
    public List<Decision> evaluateMany(Iterable<?> rows, Limits limits) {
        List<Decision> decisions = new ArrayList<>();
        for (io.github.cleanrules.ffi.Decision made : nativeRuleSet.evaluateMany(rows, limits)) {
            decisions.add(new Decision(this, made));
        }
        return Collections.unmodifiableList(decisions);
    }

    /**
     * Read a decision this rule set made, recorded as JSON
     * ({@link Decision#toJson()}), checked against the rule set
     * ({@code spec/json-format.md}, <i>Reading a decision</i>). Its opaque
     * values come back as {@code null}.
     *
     * @param text the decision document
     * @return the decision
     * @throws FormatException for invalid JSON, a document that is not a
     *     decision, or ({@code decision_mismatch}) one this rule set could not have made
     */
    public Decision readDecision(String text) {
        return readDecision(text, Format.JSON);
    }

    /**
     * Read a decision this rule set made, recorded as JSON or YAML.
     *
     * @param text the decision document
     * @param format JSON or YAML
     * @return the decision
     * @throws FormatException if the document is invalid or does not belong to this rule set
     */
    public Decision readDecision(String text, Format format) {
        return readDecision(text, format, null);
    }

    /**
     * Read a decision this rule set made, recorded as JSON or YAML, within
     * limits: the document's size and nesting, and the depth of its result
     * trees.
     *
     * @param text the decision document
     * @param format JSON or YAML
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws FormatException if the document is invalid, beyond the limits
     *     ({@code limit_exceeded}), or does not belong to this rule set
     */
    public Decision readDecision(String text, Format format, Limits limits) {
        return new Decision(this, nativeRuleSet.readDecision(text, format, limits));
    }

    // ---- Actions.

    /**
     * This rule set with its named actions resolved in a registry
     * ({@code spec/rules.md}, <i>Actions</i>): the only place names are looked
     * up, so a rule file can only call what the program registers. Every
     * named action is checked, in rule order, whether or not its rule would
     * fire; rules whose action is code keep it.
     *
     * @param actions each action's name and its code
     * @return a new rule set; this one is unchanged
     * @throws RegistryException for a named action the registry lacks, with
     *     the rule, the name and the registered names
     */
    public RuleSet bind(Map<String, ? extends Action> actions) {
        Objects.requireNonNull(actions, "actions");
        actions.forEach((name, action) -> {
            Objects.requireNonNull(name, "an action's name is null");
            Objects.requireNonNull(action, () -> "the action " + name + " is null");
        });
        nativeRuleSet.checkActions(actions.keySet());
        List<Rule> bound = new ArrayList<>(rules.size());
        for (Rule rule : rules) {
            bound.add(rule.actionName().map(name -> rule.action(actions.get(name))).orElse(rule));
        }
        return new RuleSet(nativeRuleSet, bound);
    }

    /**
     * Evaluate like {@link #evaluate(Object)}, then call the actions of the
     * selected rules, in rule order, on this thread, each with an
     * {@link ActionContext}. Named actions must be bound ({@link #bind}),
     * which is checked before anything is read.
     *
     * @param data the input
     * @return the decision
     * @throws RegistryException if a rule names an action that is not bound
     * @throws ActionException if an action throws: later actions do not run,
     *     and the exception has what it threw as its cause, the rules whose
     *     actions had run, and the decision
     */
    public Decision run(Object data) {
        return run(data, null);
    }

    /**
     * Evaluate within limits like {@link #evaluate(Object, Limits)}, then call
     * the actions of the selected rules as {@link #run(Object)} does.
     *
     * @param data the input
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws RegistryException if a rule names an action that is not bound
     * @throws ActionException if an action throws
     */
    public Decision run(Object data, Limits limits) {
        LinkedHashSet<String> bound = new LinkedHashSet<>();
        for (Rule rule : rules) {
            if (rule.action().isPresent()) {
                rule.actionName().ifPresent(bound::add);
            }
        }
        nativeRuleSet.checkActions(bound);
        Decision decision = evaluate(data, limits);
        List<String> executed = new ArrayList<>();
        for (RuleResult result : decision.selected()) {
            Optional<Action> action = result.rule().action();
            if (action.isEmpty()) {
                continue;
            }
            Map<String, Object> output = result.output().orElse(Map.of());
            ActionContext context =
                    new ActionContext(result.rule(), result.position(), data, result, output, decision);
            try {
                action.get().run(context);
            } catch (VirtualMachineError fatal) {
                throw fatal;
            } catch (Throwable thrown) {
                throw Build.action(
                        result.rule().name(), result.rule().actionName().orElse(null), executed, thrown, decision);
            }
            executed.add(result.rule().name());
        }
        return decision;
    }

    // ---- Describing.

    /**
     * Describe the rule set without evaluating it: rules, fields read,
     * outputs, tags ({@code spec/explain.md}, <i>Summaries</i>). Rules whose
     * action is code are marked as having one.
     *
     * @return the summary
     */
    public Summary summary() {
        boolean[] actions = new boolean[rules.size()];
        for (int i = 0; i < actions.length; i++) {
            actions[i] = rules.get(i).action().isPresent();
        }
        return new Summary(summary(actions, 1), () -> summary(actions, 3));
    }

    private String summary(boolean[] actions, int format) {
        return withNative(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment flags = scope.bools(actions);
                MemorySegment out = scope.pointer();
                MemorySegment len = scope.size();
                scope.check(Native.rulesetSummaryActions(pointer, flags, actions.length, format, out, len, scope.error()));
                return Scope.takeText(out, len);
            }
        });
    }

    /**
     * The rule names and the hit policy, such as {@code RuleSet([vip, loyal], priority)}.
     *
     * @return the text
     */
    @Override
    public String toString() {
        return "RuleSet(" + byName.keySet() + ", " + hitPolicy().jsonName() + ")";
    }
}
