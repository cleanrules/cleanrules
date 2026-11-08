package io.github.cleanrules.ffi;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.ActionException;
import io.github.cleanrules.Aggregate;
import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.EvaluationException;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.HitPolicyException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.internal.Bridge;
import io.github.cleanrules.internal.DataReader;
import io.github.cleanrules.internal.Errors;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.PathTree;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import io.github.cleanrules.internal.Values;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;

/**
 * A rule set, ready to evaluate ({@code spec/rules.md}): read from a document
 * in JSON, YAML, the rule syntax or CSV (a decision table), evaluated on Java
 * data or on JSON, written, summarized, and run with actions.
 *
 * <p>Java data is read as {@code spec/data-model.md} says (<i>Java and
 * Scala</i>): only the paths the rule set reads ({@link #fieldPaths()}), each
 * object once. {@code null} and {@code Optional.empty()} are null;
 * {@link Boolean}; {@link Byte}, {@link Short}, {@link Integer}, {@link Long}
 * and {@link java.math.BigInteger} are ints (beyond 64 bits, a null with reason
 * {@code int_overflow}); {@link Float} and {@link Double} are floats (NaN is
 * null); {@link java.math.BigDecimal} is a decimal (beyond 38 digits, a null
 * with reason {@code decimal_overflow}); {@link CharSequence} is a string;
 * {@link java.time.LocalDate} and {@code java.sql.Date} are dates;
 * {@link java.time.LocalDateTime} is a naive datetime;
 * {@link java.time.Instant}, {@link java.util.Date} and
 * {@code java.sql.Timestamp} are aware datetimes in {@code Z},
 * {@link java.time.OffsetDateTime} at its offset and
 * {@link java.time.ZonedDateTime} in its zone; lists and arrays (except
 * {@code byte[]}) are lists; maps with string keys are maps, in their order
 * when they are {@link java.util.SequencedMap}s and sorted otherwise; records
 * are maps of their components, and other objects maps of their public fields
 * and JavaBean accessors. Anything else, and every other object of the Java
 * platform, is opaque: compared by identity, and given back as itself. A getter
 * that throws gives a null with reason {@code read_error}; in strict mode, the
 * {@link EvaluationException} has the exception as its cause.
 *
 * <p>A rule set owns native memory: close it when done, or the cleaner frees
 * it once it is unreachable; its decisions stay valid. Rule sets never change,
 * so one may evaluate on several threads at once; closing it while another
 * thread evaluates frees it when that evaluation ends.
 */
public final class RuleSet implements AutoCloseable {
    static {
        Bridge.register(new Bridge.Ffi() {
            @Override
            public RuleSet ruleSet(MemorySegment owned) {
                return new RuleSet(owned);
            }

            @Override
            public Handle handle(RuleSet ruleset) {
                return ruleset.handle;
            }

            @Override
            public Handle handle(Decision decision) {
                return decision.handle();
            }

            @Override
            public List<Object> opaques(Decision decision) {
                return decision.opaques();
            }
        });
    }

    private final Handle handle;
    private volatile PathTree tree;

    private RuleSet(MemorySegment pointer) {
        handle = Handle.own(this, pointer, Native::rulesetFree, "RuleSet");
    }

    // ---- Reading and writing.

    /**
     * Read a rule set: a rule set, a decision table (compiled), or one rule.
     *
     * @param text the document's text
     * @param format its syntax
     * @return the rule set
     * @throws CleanRulesException if the document is invalid or holds a condition
     */
    public static RuleSet read(String text, Format format) {
        return read(text, format, null);
    }

    /**
     * Read a rule set within limits.
     *
     * @param text the document's text
     * @param format its syntax
     * @param limits the limits, or null for the defaults
     * @return the rule set
     * @throws CleanRulesException if the document is invalid or holds a condition
     */
    public static RuleSet read(String text, Format format, Limits limits) {
        return read(Text.document(Objects.requireNonNull(text, "text")), format, limits);
    }

    /**
     * Read a rule set from UTF-8 bytes.
     *
     * @param text the document's bytes
     * @param format its syntax
     * @return the rule set
     * @throws CleanRulesException if the document is invalid or holds a condition
     */
    public static RuleSet read(byte[] text, Format format) {
        return read(text, format, null);
    }

    /**
     * Read a rule set from UTF-8 bytes within limits.
     *
     * @param text the document's bytes
     * @param format its syntax
     * @param limits the limits, or null for the defaults
     * @return the rule set
     * @throws CleanRulesException if the document is invalid or holds a condition
     */
    public static RuleSet read(byte[] text, Format format, Limits limits) {
        Objects.requireNonNull(text, "text");
        Objects.requireNonNull(format, "format");
        Native.ensureLoaded();
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(Native.rulesetRead(
                    scope.bytes(text), text.length, Native.format(format), scope.limits(limits), out, scope.error()));
            return new RuleSet(out.get(ADDRESS, 0));
        }
    }

    /**
     * The rule set a document gives: its rule set, the one its decision table
     * compiles to, or a rule set of its one rule with the default settings.
     *
     * @param document the document
     * @return the rule set
     * @throws FormatException if the document holds a condition
     */
    public static RuleSet fromDocument(Document document) {
        Objects.requireNonNull(document, "document");
        MemorySegment pointer = document.acquire();
        try {
            return new RuleSet(Scope.value((out, error) -> Native.rulesetFromDocument(pointer, out, error)));
        } finally {
            document.release();
        }
    }

    /**
     * Write the rule set as JSON, in canonical form.
     *
     * @return the JSON text
     */
    public String write() {
        return write(Format.JSON);
    }

    /**
     * Write the rule set in JSON, YAML or the rule syntax.
     *
     * @param format the syntax
     * @return the text
     * @throws UnsupportedException if the format cannot hold this rule set
     */
    public String write(Format format) {
        Objects.requireNonNull(format, "format");
        return with(pointer ->
                Scope.text((out, len, error) -> Native.rulesetWrite(pointer, Native.format(format), out, len, error)));
    }

    /**
     * The rule set's summary ({@code spec/explain.md}, <i>Summaries</i>) as text.
     *
     * @return the summary
     */
    public String summary() {
        return summary(Format.TEXT);
    }

    /**
     * The rule set's summary as text, or as a JSON or YAML document.
     *
     * @param format TEXT, JSON or YAML
     * @return the summary
     */
    public String summary(Format format) {
        Objects.requireNonNull(format, "format");
        return with(pointer ->
                Scope.text((out, len, error) -> Native.rulesetSummary(pointer, Native.format(format), out, len, error)));
    }

    // ---- What it is.

    /**
     * The number of rules.
     *
     * @return the number of rules
     */
    public int size() {
        return with(pointer -> Math.toIntExact(Native.rulesetLen(pointer)));
    }

    /**
     * The name of a rule.
     *
     * @param rule the rule's position, from 0
     * @return its name
     * @throws IndexOutOfBoundsException if there is no such rule
     */
    public String ruleName(int rule) {
        Objects.checkIndex(rule, size());
        return with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment name = scope.pointer();
                MemorySegment len = scope.size();
                scope.check(Native.rulesetRuleName(pointer, rule, name, len, scope.error()));
                return Scope.borrowedText(name, len);
            }
        });
    }

    /**
     * The names of the rules, in rule order.
     *
     * @return the names
     */
    public List<String> ruleNames() {
        int size = size();
        List<String> names = new ArrayList<>(size);
        for (int i = 0; i < size; i++) {
            names.add(ruleName(i));
        }
        return Collections.unmodifiableList(names);
    }

    /**
     * The hit policy.
     *
     * @return the policy
     */
    public HitPolicy hitPolicy() {
        return with(pointer -> HitPolicy.fromJsonName(Native.rulesetHitPolicy(pointer)));
    }

    /**
     * The aggregate of a {@code collect} rule set, which is then its single output.
     *
     * @return the aggregate, or empty
     */
    public Optional<Aggregate> aggregate() {
        return with(pointer -> Optional.ofNullable(Native.rulesetAggregate(pointer)).map(Aggregate::fromJsonName));
    }

    /**
     * The rule set's logic, by name ({@code boolean}).
     *
     * @return the logic's name
     */
    public String logic() {
        return with(Native::rulesetLogic);
    }

    /**
     * Whether the rule set is strict ({@code spec/rules.md}, <i>Strict mode and
     * errors</i>): a missing field or a type mismatch is then an error.
     *
     * @return whether it is strict
     */
    public boolean isStrict() {
        return with(Native::rulesetStrict);
    }

    /**
     * The paths of the input that the rule set reads, its conditions' and
     * payloads', in the order of its summary: the only ones read from Java
     * data.
     *
     * @return the paths: lists of {@link String} keys and {@link Integer} indexes
     */
    public List<List<Object>> fieldPaths() {
        return with(pointer -> Values.paths(Scope.value((out, error) -> Native.rulesetFieldPaths(pointer, out, error))));
    }

    private PathTree tree() {
        PathTree read = tree;
        if (read == null) {
            read = PathTree.of(fieldPaths());
            tree = read;
        }
        return read;
    }

    // ---- Evaluating.

    /**
     * Evaluate the rule set on Java data. It never runs actions.
     *
     * @param data the input, such as a map, a record or a JavaBean
     * @return the decision
     * @throws EvaluationException in strict mode, when the data lacks a field
     *     or holds a value of another type (and {@link HitPolicyException} when the
     *     hit policy cannot decide, in any mode)
     */
    public Decision evaluate(Object data) {
        return evaluate(data, null);
    }

    /**
     * Evaluate the rule set on Java data within limits: lists and maps nested
     * deeper than the limits' JSON nesting, and data of more than
     * {@link Limits#maxValues()} values, are a {@link FormatException}
     * ({@code limit_exceeded}).
     *
     * @param data the input
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws CleanRulesException if the evaluation fails
     */
    public Decision evaluate(Object data, Limits limits) {
        return with(pointer -> {
            try (Scope scope = new Scope();
                    DataReader reader = DataReader.within(limits)) {
                MemorySegment input = reader.read(data, tree());
                scope.onClose(() -> Native.valueFree(input));
                MemorySegment out = scope.pointer();
                scope.check(
                        Native.rulesetEvaluateValue(pointer, input, scope.limits(limits), out, scope.error()),
                        reader.causes);
                return new Decision(out.get(ADDRESS, 0), reader.causes.opaques);
            }
        });
    }

    /**
     * Evaluate the rule set on an input given as a JSON data value
     * ({@code spec/json-format.md}, <i>Values</i>), such as
     * {@code {"customer": {"years": 7}}}.
     *
     * @param json the input's JSON text
     * @return the decision
     * @throws CleanRulesException if the JSON is invalid or the evaluation fails
     */
    public Decision evaluateJson(String json) {
        return evaluateJson(json, null);
    }

    /**
     * Evaluate the rule set on a JSON input within limits.
     *
     * @param json the input's JSON text
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws CleanRulesException if the JSON is invalid or the evaluation fails
     */
    public Decision evaluateJson(String json, Limits limits) {
        return evaluateJson(Text.document(Objects.requireNonNull(json, "json")), limits);
    }

    /**
     * Evaluate the rule set on a JSON input given as UTF-8 bytes.
     *
     * @param json the input's JSON bytes
     * @return the decision
     * @throws CleanRulesException if the JSON is invalid or the evaluation fails
     */
    public Decision evaluateJson(byte[] json) {
        return evaluateJson(json, null);
    }

    /**
     * Evaluate the rule set on a JSON input given as UTF-8 bytes, within limits.
     *
     * @param json the input's JSON bytes
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws CleanRulesException if the JSON is invalid or the evaluation fails
     */
    public Decision evaluateJson(byte[] json, Limits limits) {
        Objects.requireNonNull(json, "json");
        return with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.pointer();
                scope.check(Native.rulesetEvaluate(
                        pointer, scope.bytes(json), json.length, scope.limits(limits), out, scope.error()));
                return new Decision(out.get(ADDRESS, 0), List.of());
            }
        });
    }

    /**
     * Evaluate the rule set on each input of a list, in order.
     *
     * @param inputs the inputs, as Java data, in the order they are iterated
     * @return one decision per input
     * @throws CleanRulesException for the first input that fails, with its
     *     position as {@link CleanRulesException#row()}
     */
    public List<Decision> evaluateMany(Iterable<?> inputs) {
        return evaluateMany(inputs, null);
    }

    /**
     * Evaluate the rule set on each input of a list, in order, within limits.
     *
     * @param inputs the inputs, as Java data
     * @param limits the limits, or null for the defaults
     * @return one decision per input
     * @throws CleanRulesException for the first input that fails, with its row
     */
    public List<Decision> evaluateMany(Iterable<?> inputs, Limits limits) {
        Objects.requireNonNull(inputs, "inputs");
        return with(pointer -> {
            try (Scope scope = new Scope();
                    DataReader reader = DataReader.within(limits)) {
                MemorySegment list = reader.readMany(inputs, tree());
                scope.onClose(() -> Native.valueFree(list));
                return batch(pointer, list, scope.limits(limits), scope, reader.causes);
            }
        });
    }

    /**
     * Evaluate the rule set on each input of a JSON array, in order.
     *
     * @param json the inputs' JSON text: an array of data values
     * @return one decision per input
     * @throws CleanRulesException if the JSON is invalid, is not an array, or an
     *     input fails
     */
    public List<Decision> evaluateManyJson(String json) {
        return evaluateManyJson(json, null);
    }

    /**
     * Evaluate the rule set on each input of a JSON array, in order, within limits.
     *
     * @param json the inputs' JSON text: an array of data values
     * @param limits the limits, or null for the defaults
     * @return one decision per input
     * @throws CleanRulesException if the JSON is invalid, is not an array, or an
     *     input fails
     */
    public List<Decision> evaluateManyJson(String json, Limits limits) {
        return evaluateManyJson(Text.document(Objects.requireNonNull(json, "json")), limits);
    }

    /**
     * Evaluate the rule set on each input of a JSON array given as UTF-8
     * bytes, in order, within limits.
     *
     * @param bytes the inputs' JSON bytes: an array of data values
     * @param limits the limits, or null for the defaults
     * @return one decision per input
     * @throws CleanRulesException if the JSON is invalid, is not an array, or an
     *     input fails
     */
    public List<Decision> evaluateManyJson(byte[] bytes, Limits limits) {
        Objects.requireNonNull(bytes, "bytes");
        return with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment limitsPointer = scope.limits(limits);
                MemorySegment out = scope.pointer();
                scope.check(Native.valueFromJson(scope.bytes(bytes), bytes.length, limitsPointer, out, scope.error()));
                MemorySegment list = out.get(ADDRESS, 0);
                scope.onClose(() -> Native.valueFree(list));
                return batch(pointer, list, limitsPointer, scope, null);
            }
        });
    }

    private static List<Decision> batch(
            MemorySegment ruleset, MemorySegment inputs, MemorySegment limits, Scope scope, Errors.Causes causes) {
        MemorySegment out = scope.pointer();
        scope.check(Native.rulesetEvaluateMany(ruleset, inputs, limits, out, scope.error()), causes);
        MemorySegment batch = out.get(ADDRESS, 0);
        List<Object> opaques = causes == null ? List.of() : causes.opaques;
        List<Decision> decisions = new ArrayList<>();
        try {
            long count = Native.batchLen(batch);
            for (long i = 0; i < count; i++) {
                decisions.add(Decision.copyOf(Native.batchGet(batch, i), opaques));
            }
        } catch (RuntimeException | Error thrown) {
            decisions.forEach(Decision::close);
            throw thrown;
        } finally {
            Native.batchFree(batch);
        }
        return Collections.unmodifiableList(decisions);
    }

    /**
     * Read a recorded decision in JSON, as {@link Decision#write()} writes it,
     * and check that it belongs to this rule set.
     *
     * @param text the decision's JSON text
     * @return the decision
     * @throws FormatException if it is invalid, or does not belong to this rule
     *     set ({@code decision_mismatch})
     */
    public Decision readDecision(String text) {
        return readDecision(text, Format.JSON, null);
    }

    /**
     * Read a recorded decision in JSON or YAML.
     *
     * @param text the decision's text
     * @param format JSON or YAML
     * @return the decision
     * @throws CleanRulesException if it is invalid or does not belong to this rule set
     */
    public Decision readDecision(String text, Format format) {
        return readDecision(text, format, null);
    }

    /**
     * Read a recorded decision in JSON or YAML within limits. Its opaque values
     * come back as {@code null}: the document keeps only their type's name.
     *
     * @param text the decision's text
     * @param format JSON or YAML
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws CleanRulesException if it is invalid or does not belong to this rule set
     */
    public Decision readDecision(String text, Format format, Limits limits) {
        return readDecision(Text.document(Objects.requireNonNull(text, "text")), format, limits);
    }

    /**
     * Read a recorded decision from UTF-8 bytes within limits.
     *
     * @param text the decision's bytes
     * @param format JSON or YAML
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws CleanRulesException if it is invalid or does not belong to this rule set
     */
    public Decision readDecision(byte[] text, Format format, Limits limits) {
        Objects.requireNonNull(text, "text");
        Objects.requireNonNull(format, "format");
        return with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.pointer();
                scope.check(Native.rulesetReadDecision(
                        pointer,
                        scope.bytes(text),
                        text.length,
                        Native.format(format),
                        scope.limits(limits),
                        out,
                        scope.error()));
                return new Decision(out.get(ADDRESS, 0), List.of());
            }
        });
    }

    // ---- Actions.

    /**
     * Check that every action the rule set names is among {@code names}, in
     * rule order ({@code spec/rules.md}, <i>Actions</i>): what {@link #run}
     * checks before it evaluates.
     *
     * @param names the names of the actions there are
     * @throws RegistryException for the first rule whose action is not there,
     *     with the rule, the name and the names there are
     * @throws IllegalArgumentException for an empty or repeated name
     */
    public void checkActions(Collection<String> names) {
        Objects.requireNonNull(names, "names");
        List<String> list = List.copyOf(names);
        with(pointer -> {
            try (Scope scope = new Scope()) {
                MemorySegment registry = Actions.registry(list, scope);
                scope.check(Native.rulesetCheckActions(pointer, registry, scope.error()));
            }
            return null;
        });
    }

    /**
     * Evaluate the rule set on Java data, then call the actions of the
     * selected rules, in rule order, on this thread ({@code spec/rules.md},
     * <i>Actions</i>). Every action the rule set names must be in
     * {@code actions}, which is checked before the data is read.
     *
     * @param data the input
     * @param actions the actions, by the names the rules give them
     * @return the decision
     * @throws RegistryException if a rule names an action that is not there
     * @throws ActionException if an action throws: the run stops there, and the
     *     exception has what it threw as its cause, and the decision
     */
    public Decision run(Object data, Map<String, ? extends Action> actions) {
        return run(data, actions, null);
    }

    /**
     * Evaluate the rule set on Java data within limits, then call the actions
     * of the selected rules.
     *
     * @param data the input
     * @param actions the actions, by the names the rules give them
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws RegistryException if a rule names an action that is not there
     * @throws ActionException if an action throws
     */
    public Decision run(Object data, Map<String, ? extends Action> actions, Limits limits) {
        Objects.requireNonNull(actions, "actions");
        List<String> names = new ArrayList<>(actions.size());
        List<Action> calls = new ArrayList<>(actions.size());
        actions.forEach((name, action) -> {
            names.add(Objects.requireNonNull(name, "an action's name is null"));
            calls.add(Objects.requireNonNull(action, () -> "the action " + name + " is null"));
        });
        return with(pointer -> {
            try (Scope scope = new Scope();
                    DataReader reader = DataReader.within(limits)) {
                MemorySegment registry = Actions.registry(names, scope);
                // Unknown names fail before anything is read.
                scope.check(Native.rulesetCheckActions(pointer, registry, scope.error()));
                MemorySegment input = reader.read(data, tree());
                scope.onClose(() -> Native.valueFree(input));
                MemorySegment limitsPointer = scope.limits(limits);
                MemorySegment out = scope.pointer();
                MemorySegment error = scope.error();
                Actions.Frame frame = Actions.push(calls, data, reader.causes.opaques);
                int status;
                try {
                    status = Native.rulesetRun(pointer, registry, input, limitsPointer, out, error);
                } finally {
                    Actions.pop(frame);
                }
                if (status != Native.CR_OK) {
                    reader.causes.action = frame.failure;
                    List<Object> opaques = reader.causes.opaques;
                    reader.causes.decisions = borrowed -> Decision.copyOf(borrowed, opaques);
                    scope.causes = reader.causes;
                    RuntimeException failure = scope.failure(status);
                    // An error of the virtual machine is never wrapped: it goes on as it is.
                    if (frame.failure instanceof VirtualMachineError fatal) {
                        if (failure instanceof ActionException action) {
                            action.nativeDecision().ifPresent(Decision::close);
                        }
                        throw fatal;
                    }
                    throw failure;
                }
                return new Decision(out.get(ADDRESS, 0), reader.causes.opaques);
            }
        });
    }

    private <T> T with(java.util.function.Function<MemorySegment, T> call) {
        MemorySegment pointer = handle.acquire();
        try {
            return call.apply(pointer);
        } finally {
            handle.release();
        }
    }

    /** Free the native rule set now; its decisions stay valid, and closing again does nothing. */
    @Override
    public void close() {
        handle.closeFromOwner();
    }
}
