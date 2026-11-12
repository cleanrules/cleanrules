package io.github.cleanrules.tests;

import io.github.cleanrules.ActionException;
import io.github.cleanrules.Aggregate;
import io.github.cleanrules.Cause;
import io.github.cleanrules.CauseGroup;
import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.Closed;
import io.github.cleanrules.Conditions;
import io.github.cleanrules.Const;
import io.github.cleanrules.Counterfactual;
import io.github.cleanrules.Decision;
import io.github.cleanrules.DecisionTable;
import io.github.cleanrules.DocumentContent;
import io.github.cleanrules.Documents;
import io.github.cleanrules.EvaluationException;
import io.github.cleanrules.Explanation;
import io.github.cleanrules.Expr;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Field;
import io.github.cleanrules.FieldUsage;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.HitPolicyException;
import io.github.cleanrules.Kind;
import io.github.cleanrules.Limits;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.Note;
import io.github.cleanrules.Reason;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.Requirement;
import io.github.cleanrules.ResultTree;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleDefinitionException;
import io.github.cleanrules.RuleExplanation;
import io.github.cleanrules.RuleNotFoundException;
import io.github.cleanrules.RuleResult;
import io.github.cleanrules.RuleSet;
import io.github.cleanrules.RuleSummary;
import io.github.cleanrules.RuleSyntaxException;
import io.github.cleanrules.Summary;
import io.github.cleanrules.TablePolicy;
import io.github.cleanrules.Truth;
import io.github.cleanrules.TypeMismatchException;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.ffi.Document;
import io.github.cleanrules.ffi.Node;
import io.github.cleanrules.tests.JavaValues.NoJavaForm;
import io.github.cleanrules.tests.JavaValues.NotGiven;
import io.github.cleanrules.tests.TestJson.Mode;
import io.github.cleanrules.tests.TestJson.Num;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.time.Period;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.function.Function;
import java.util.function.Supplier;
import java.util.function.UnaryOperator;
import java.util.stream.Stream;

/**
 * The JVM conformance runner (conformance/README.md, Runners): every vector of
 * conformance/vectors runs twice, as in each binding.
 *
 * <ul>
 *   <li>{@code json:<id>}: documents, inputs and results cross as JSON text
 *       through the native layer ({@code io.github.cleanrules.ffi}):
 *       {@code Document.read}/{@code write} in JSON, YAML, the rule syntax and
 *       CSV, {@code RuleSet.read}, {@code fromDocument}, {@code readDecision},
 *       {@code evaluateJson}, {@code evaluateManyJson}, {@code explain},
 *       {@code summary}, and {@code Node.evaluateTreeJson} and
 *       {@code evaluateValueTreeJson}. Decisions are also checked with the
 *       layer's readers ({@code ruleName}, {@code firedAt}, {@code truth},
 *       {@code selected}, {@code ruleOutput}, {@code outputs},
 *       {@code outputsJson}, {@code output}, {@code outputJson}).</li>
 *   <li>{@code native:<id>}: the idiomatic API ({@code io.github.cleanrules}).
 *       Conditions, rules and rule sets are built in code
 *       ({@link Conditions}, the methods of {@link Expr}, {@link Rule},
 *       {@link RuleSet#builder()}) and checked against the core's reading of
 *       the same JSON; inputs are Java data converted from the vector
 *       ({@link JavaValues#toJava}); results are compared as the values given
 *       back to Java (spec/data-model.md, Java and Scala, Values given back):
 *       {@link Decision} through its results, selection and outputs,
 *       {@link Explanation}, {@link Summary} and {@link ResultTree} through
 *       their objects, and each one's {@code toJson()} exactly. The same Java
 *       data also goes through the native layer ({@code RuleSet.evaluate},
 *       {@code evaluateMany}, {@code Node.evaluateTree}), whose JSON is
 *       compared exactly. {@code read} vectors go through
 *       {@link Documents#loads}, {@link Documents#dumps},
 *       {@link Documents#ruleFromText}, {@link DecisionTable} (grids) and
 *       {@link RuleSet#readDecision(String, Format, Limits)}.</li>
 * </ul>
 *
 * <p>The runner only drives the library and compares: results are parsed with
 * {@link TestJson} and compared with the vectors as trees, never with the
 * library's own equality.
 *
 * <p>A test fixture (Gradle's {@code java-test-fixtures}), with
 * {@link TestJson} and {@link JavaValues}: the Scala layer's tests extend it
 * with the route {@code scala:<id>} ({@link #runRoute}), in this package so
 * that they reach its package-private helpers.
 */
class Conformance {
    /** Vectors are only ever added: the count when this runner was written. */
    static final int MINIMUM_VECTORS = 2519;

    /** Why a vector does not run one way: each is counted and listed. */
    static final Map<String, String> SKIPS = Map.of(
            "grid",
            "grids are Java data: the native layer reads documents only, so table_grid vectors run in the native"
                    + " route (DecisionTable)",
            "decimal_infinity",
            "a decimal infinity in the vector's data or constants, which no Java value holds (spec/data-model.md,"
                    + " Java and Scala: BigDecimal cannot hold it, and a Double infinity is a float)",
            "zone_unknown_to_java",
            "a datetime in a zone that Java's zone database does not know, which no Java value holds",
            "native",
            "max_values bounds what is read from native data only, so evaluate vectors with limits run in the native"
                    + " route",
            "diagram",
            "diagrams (spec/visual.md) are drawn by the core and checked through the C ABI's runner; the Java and"
                    + " Scala APIs draw them from phase V.2");

    /** The exception class of each error code (spec/errors.md, Codes). */
    private static final Map<String, Class<? extends CleanRulesException>> CLASSES = Map.ofEntries(
            Map.entry("format_error", FormatException.class),
            Map.entry("unsupported_version", FormatException.class),
            Map.entry("limit_exceeded", FormatException.class),
            Map.entry("decision_mismatch", FormatException.class),
            Map.entry("expression_error", ExpressionException.class),
            Map.entry("rule_definition_error", RuleDefinitionException.class),
            Map.entry("rule_syntax_error", RuleSyntaxException.class),
            Map.entry("registry_error", RegistryException.class),
            Map.entry("rule_not_found", RuleNotFoundException.class),
            Map.entry("evaluation_error", EvaluationException.class),
            Map.entry("missing_field", MissingFieldException.class),
            Map.entry("type_mismatch", TypeMismatchException.class),
            Map.entry("hit_policy_error", HitPolicyException.class),
            Map.entry("action_error", ActionException.class),
            Map.entry("unsupported", UnsupportedException.class));

    /** The fields of an evaluation error that are compared (spec/errors.md, Comparing errors). */
    private static final List<String> EVALUATION = List.of("code", "rule", "path", "types", "rules", "row", "name");

    /** The context fields an error may carry, in the order of its JSON form. */
    private static final List<String> CONTEXT =
            List.of("pointer", "line", "column", "name", "rule", "path", "types", "rules", "row");

    static final List<String> SINGLE_HIT = List.of("first", "priority", "unique", "any");

    /** A vector's outcome one way: passed, failed with a message, or skipped for a reason of {@link #SKIPS}. */
    record Outcome(String failure, String skip) {
        static final Outcome PASSED = new Outcome(null, null);

        boolean passed() {
            return failure == null && skip == null;
        }
    }

    /** A vector the runner cannot run as written, or an answer that breaks the API's promises. */
    static final class VectorError extends RuntimeException {
        private static final long serialVersionUID = 1L;

        VectorError(String message) {
            super(message);
        }
    }

    /** What a step gave: its result, or the library's error. */
    record Attempt(Object result, CleanRulesException error) {}

    /** The vectors whose Java data used each Java class, by class. */
    final Map<String, Set<String>> forms = new TreeMap<>();

    /** The vector running, as {@code <route>:<id>}: what {@link #forms} counts. */
    String current = "";

    /** Run {@code vector} one way ({@code "json"} or {@code "native"}). */
    Outcome run(Map<?, ?> vector, String route) {
        current = route + ":" + vector.get("id");
        if (vector.get("given") instanceof Map<?, ?> given && "draw".equals(given.get("kind"))) {
            return new Outcome(null, "diagram");
        }
        try {
            String failure = switch (route) {
                case "json" -> runJson(vector);
                case "native" -> runNative(vector);
                default -> runRoute(route, vector);
            };
            return failure.isEmpty() ? Outcome.PASSED : new Outcome(failure, null);
        } catch (Skip skip) {
            return new Outcome(null, skip.reason);
        } catch (NoJavaForm missing) {
            return new Outcome(null, missing.reason);
        } catch (CleanRulesException error) {
            return new Outcome("uncaught " + error.getClass().getSimpleName() + " (" + error.code() + "): "
                    + error.getMessage(), null);
        } catch (VectorError | NotGiven error) {
            return new Outcome(error.getMessage(), null);
        } catch (RuntimeException error) {
            return new Outcome(error.toString(), null);
        }
    }

    /**
     * Run {@code vector} through another route: the Scala layer's
     * ({@code scala}, a subclass in the scala project's tests) or none.
     */
    protected String runRoute(String route, Map<?, ?> vector) {
        throw new VectorError("unknown route " + route);
    }

    /** A route that does not apply to a vector. */
    static final class Skip extends RuntimeException {
        private static final long serialVersionUID = 1L;

        final String reason;

        Skip(String reason) {
            super(reason, null, false, false);
            this.reason = reason;
        }
    }

    // ---- Reading the vectors.

    /** The vectors' root: the system property {@code cleanrules.conformance.vectors}. */
    static Path root() {
        return Path.of(System.getProperty("cleanrules.conformance.vectors", "../../../conformance/vectors"));
    }

    /** The vector files, by their path from the root, in the order of their paths. */
    static Map<String, List<?>> files() {
        Path root = root();
        Map<String, List<?>> files = new TreeMap<>();
        try (Stream<Path> found = Files.walk(root)) {
            for (Path file : found.filter(p -> p.toString().endsWith(".json")).sorted().toList()) {
                Object document = TestJson.parse(Files.readString(file));
                String name = root.relativize(file).toString().replace('\\', '/');
                files.put(name, array(need(document, "vectors")));
            }
        } catch (IOException failed) {
            throw new UncheckedIOException(failed);
        }
        return files;
    }

    /**
     * Vectors with their expected output altered, which every route must fail
     * (the self-check of each runner): the vector's id, the text of its
     * expected output, and what replaces it.
     */
    static final String[][] WRONG = {
        // A value, a truth value, an int that should be a float, an order.
        {"ast/nodes/div-int", "\"value\":3.5", "\"value\":3.25"},
        {"ast/missing/missing-field", "\"truth\":null", "\"truth\":false"},
        {"rules/batch/decisions", "\"value\":2}", "\"value\":2.0}"},
        {"explain/summaries/pricing", "\"priority\":10", "\"priority\":11"},
        {"explain/summaries/pricing", "\"outputs\":[\"discount\",\"total\"]", "\"outputs\":[\"total\",\"discount\"]"},
        // What the objects do not give (a cause's `at`) is still checked,
        // by toJson() and by the native layer.
        {"explain/counterfactuals/at-the-limit", "\"at\":\"/decision/results/0/when\"",
            "\"at\":\"/decision/results/0\""},
        {"explain/counterfactuals/at-the-limit", "\"gap\":0", "\"gap\":1"},
        {"explain/counterfactuals/datetime-gap", "\"microseconds\":9000000000", "\"microseconds\":9000000001"},
        {"explain/counterfactuals/decimal-gap", "\"gap\":{\"$decimal\":\"0.15\"}", "\"gap\":{\"$decimal\":\"0.150\"}"},
        {"ast/missing/strict-missing", "\"path\":[\"age\"]", "\"path\":[\"years\"]"},
        {"format/conditions/and-flattened", "\"path\":[\"c\"]", "\"path\":[\"d\"]"},
        {"format/tables/basic", "loyal,> 2,-,0.05", "loyal,> 3,-,0.05"},
        // A status, a note's priority, an aggregate, a tag, a datetime, an error's pointer.
        {"explain/counterfactuals/at-the-limit", "\"status\":\"not_fired\"", "\"status\":\"unknown\""},
        {"explain/notes/same-priority", "\"winner_priority\":10", "\"winner_priority\":11"},
        {"rules/aggregates/sum", "\"aggregated\":{\"n\":5.5", "\"aggregated\":{\"n\":5.25"},
        {"explain/summaries/pricing", "\"tag\":\"risk\"", "\"tag\":\"risky\""},
        {"ast/types/aware-by-instant", "08:30:00Z", "08:30:01Z"},
        {"format/tables/grid-header-empty", "\"pointer\":\"/header/1\"", "\"pointer\":\"/header/0\""},
        {"format/decisions/tree-too-deep", "\"code\":\"limit_exceeded\"", "\"code\":\"format_error\""},
    };

    @SuppressWarnings("unchecked") // Parsed JSON objects have string keys.
    static Map<String, Object> object(Object value) {
        if (!(value instanceof Map<?, ?>)) {
            throw new VectorError("not an object: " + TestJson.write(value));
        }
        return (Map<String, Object>) value;
    }

    static List<?> array(Object value) {
        if (!(value instanceof List<?> list)) {
            throw new VectorError("not an array: " + TestJson.write(value));
        }
        return list;
    }

    static Object need(Object value, String key) {
        Map<String, Object> members = object(value);
        if (!members.containsKey(key)) {
            throw new VectorError("no `" + key + "` in " + TestJson.write(value));
        }
        return members.get(key);
    }

    static String text(Object value, String key) {
        return (String) need(value, key);
    }

    /** The content of a document the library wrote, such as its {@code decision}. */
    static Object content(String text, String key) {
        return need(TestJson.parse(text), key);
    }

    static String rulesetDocument(Map<?, ?> given) {
        return "{\"version\":1,\"ruleset\":" + TestJson.write(need(given, "ruleset")) + "}";
    }

    static Limits limitsOf(Map<?, ?> given) {
        if (!(given.get("limits") instanceof Map<?, ?> limits)) {
            return null;
        }
        Limits made = Limits.defaults();
        for (Map.Entry<?, ?> limit : limits.entrySet()) {
            long n = Long.parseLong(((Num) limit.getValue()).text());
            made = switch ((String) limit.getKey()) {
                case "max_size" -> made.withMaxSize(n);
                case "max_nesting" -> made.withMaxNesting(n);
                case "max_depth" -> made.withMaxDepth(n);
                case "max_table_size" -> made.withMaxTableSize(n);
                case "max_yaml_size" -> made.withMaxYamlSize(n);
                case "max_text_size" -> made.withMaxTextSize(n);
                case "max_values" -> made.withMaxValues(n);
                default -> throw new VectorError("unknown limit " + limit.getKey());
            };
        }
        return made;
    }

    static Format format(String name) {
        return Format.valueOf(name.toUpperCase(Locale.ROOT));
    }

    /** The text a {@code read} vector reads, and its format. */
    record Source(String text, Format format) {}

    static Source sourceOf(Map<?, ?> given) {
        if (given.get("text") instanceof String text) {
            return new Source(text, Format.JSON);
        }
        if (given.containsKey("document")) {
            return new Source(TestJson.write(given.get("document")), Format.JSON);
        }
        if (given.get("yaml") instanceof String text) {
            return new Source(text, Format.YAML);
        }
        if (given.get("rules_text") instanceof String text) {
            return new Source(text, Format.TEXT);
        }
        if (given.get("table_csv") instanceof String text) {
            return new Source(text, Format.CSV);
        }
        throw new VectorError("nothing to read");
    }

    // ---- Comparing.

    static Attempt attempt(Supplier<?> step) {
        try {
            return new Attempt(step.get(), null);
        } catch (CleanRulesException error) {
            return new Attempt(null, error);
        }
    }

    static void checkClass(CleanRulesException error) {
        Class<? extends CleanRulesException> wanted = CLASSES.get(error.code());
        if (wanted == null) {
            throw new VectorError("an error with the unknown code " + error.code() + ": " + error.json());
        }
        if (!wanted.isInstance(error)) {
            throw new VectorError("the error " + error.code() + " is a " + error.getClass().getName() + ", not a "
                    + wanted.getName());
        }
    }

    /** An error's JSON form, as the core writes it ({@code json()}): its code and context fields. */
    static Map<String, Object> jsonForm(CleanRulesException error) {
        checkClass(error);
        Map<String, Object> written = object(TestJson.parse(error.json()));
        if (!error.code().equals(written.get("code"))) {
            throw new VectorError("code() " + error.code() + ", but json() " + error.json());
        }
        Map<String, Object> form = new LinkedHashMap<>();
        form.put("code", written.get("code"));
        for (String key : CONTEXT) {
            if (written.containsKey(key)) {
                form.put(key, written.get(key));
            }
        }
        return form;
    }

    /** An error's code and context fields, as its accessors give them. */
    static Map<String, Object> contextForm(CleanRulesException error) {
        checkClass(error);
        Map<String, Object> form = new LinkedHashMap<>();
        form.put("code", error.code());
        error.pointer().ifPresent(pointer -> form.put("pointer", pointer));
        error.line().ifPresent(line -> form.put("line", Num.of(line)));
        error.column().ifPresent(column -> form.put("column", Num.of(column)));
        error.name().ifPresent(name -> form.put("name", name));
        error.rule().ifPresent(rule -> form.put("rule", rule));
        error.path().ifPresent(path -> form.put("path", JavaValues.path(path)));
        if (!error.types().isEmpty()) {
            form.put("types", new ArrayList<Object>(error.types()));
        }
        if (!error.rules().isEmpty()) {
            form.put("rules", new ArrayList<Object>(error.rules()));
        }
        error.row().ifPresent(row -> form.put("row", Num.of(row)));
        return form;
    }

    private static Map<String, Object> only(Map<?, ?> members, List<String> keys) {
        Map<String, Object> kept = new LinkedHashMap<>();
        members.forEach((key, value) -> {
            if (keys.contains(key)) {
                kept.put((String) key, value);
            }
        });
        return kept;
    }

    private static boolean sameMembers(Map<String, Object> actual, Map<String, Object> wanted) {
        if (actual.size() != wanted.size()) {
            return false;
        }
        for (Map.Entry<String, Object> member : wanted.entrySet()) {
            if (!actual.containsKey(member.getKey()) || !TestJson.same(actual.get(member.getKey()), member.getValue())) {
                return false;
            }
        }
        return true;
    }

    static String compareError(Map<String, Object> actual, Map<?, ?> wanted, boolean read) {
        List<String> keys = read ? wanted.keySet().stream().map(String.class::cast).toList() : EVALUATION;
        Map<String, Object> got = only(actual, keys);
        Map<String, Object> want = only(wanted, keys);
        return sameMembers(got, want) ? "" : "error " + TestJson.write(got) + ", expected " + TestJson.write(want);
    }

    /** How a step's outcome is compared: whose errors, in which mode, with what view of the expected tree. */
    record How(boolean read, Function<CleanRulesException, Map<String, Object>> form, Mode mode,
            UnaryOperator<Object> view) {}

    static final How JSON = new How(false, Conformance::jsonForm, Mode.EXACT, UnaryOperator.identity());
    static final How JSON_READ = new How(true, Conformance::jsonForm, Mode.EXACT, UnaryOperator.identity());
    static final How NATIVE = new How(false, Conformance::contextForm, Mode.EXACT, UnaryOperator.identity());
    static final How NATIVE_READ = new How(true, Conformance::contextForm, Mode.EXACT, UnaryOperator.identity());
    static final How GIVEN = new How(false, Conformance::contextForm, Mode.GIVEN, UnaryOperator.identity());

    /** Compare what a step gave with the vector's {@code key}, or with its error. */
    static String compare(Attempt outcome, Map<?, ?> expect, String key, How how) {
        Object error = expect.get("error");
        if (outcome.error() == null && error == null) {
            Object want = how.view().apply(need(expect, key));
            if (TestJson.same(outcome.result(), want, how.mode())) {
                return "";
            }
            return "\n  got  " + TestJson.write(outcome.result()) + "\n  want " + TestJson.write(want);
        }
        if (outcome.error() == null) {
            return "got " + TestJson.write(outcome.result()) + ", expected the error " + TestJson.write(error);
        }
        if (error == null) {
            return "unexpected error " + TestJson.write(how.form().apply(outcome.error())) + ": "
                    + outcome.error().getMessage();
        }
        return compareError(how.form().apply(outcome.error()), object(error), how.read());
    }

    static String compareText(String what, String got, Object want) {
        return got.equals(want) ? "" : what + " " + TestJson.write(got) + ", expected " + TestJson.write(want);
    }

    static String join(List<String> issues) {
        return String.join("; ", issues.stream().filter(issue -> !issue.isEmpty()).toList());
    }

    // ---- The JSON route: the native layer.

    static Map<String, Object> outputOf(Map<?, ?> result) {
        if (!(result.get("then") instanceof Map<?, ?> then)) {
            return null;
        }
        Map<String, Object> output = new LinkedHashMap<>();
        then.forEach((key, tree) -> output.put((String) key, need(tree, "value")));
        return output;
    }

    static Truth truthOf(Object truth) {
        return truth == null ? Truth.UNKNOWN : (Boolean) truth ? Truth.TRUE : Truth.FALSE;
    }

    /**
     * A decision of the native layer read with its readers, against the
     * expected decision (as the C++ runner's {@code check_readers}); Java
     * values given back are compared in {@code mode}.
     */
    private static String checkReaders(io.github.cleanrules.ffi.Decision d, Map<?, ?> expected, Mode mode) {
        List<?> results = array(need(expected, "results"));
        if (d.size() != results.size()) {
            return "size() " + d.size() + ", expected " + results.size();
        }
        List<Map<String, Object>> outputs = new ArrayList<>();
        for (int i = 0; i < results.size(); i++) {
            Map<String, Object> result = object(results.get(i));
            String at = "rule " + i + ": ";
            if (!d.ruleName(i).equals(need(result, "rule"))) {
                return at + "ruleName() " + d.ruleName(i);
            }
            if (d.firedAt(i) != (Boolean) need(result, "fired")) {
                return at + "firedAt() differs";
            }
            Truth truth = truthOf(need(need(result, "when"), "truth"));
            if (d.truth(i) != truth) {
                return at + "truth() " + d.truth(i) + ", expected " + truth;
            }
            Map<String, Object> output = outputOf(result);
            outputs.add(output);
            Optional<Map<String, Object>> got = d.ruleOutput(i);
            if (output == null ? got.isPresent()
                    : got.isEmpty() || !TestJson.same(JavaValues.given(got.get()), output, mode)) {
                return at + "ruleOutput() " + got.map(JavaValues::given).map(TestJson::write) + ", expected "
                        + TestJson.write(output);
            }
        }
        List<Object> names = new ArrayList<>();
        List<Object> wantOutputs = new ArrayList<>();
        for (int position : d.selected()) {
            names.add(object(results.get(position)).get("rule"));
            Map<String, Object> output = outputs.get(position);
            wantOutputs.add(output == null ? new LinkedHashMap<>() : output);
        }
        if (!TestJson.same(names, need(expected, "selected"))) {
            return "selected() " + TestJson.write(names);
        }
        if (!TestJson.same(TestJson.parse(d.outputsJson()), wantOutputs)) {
            return "outputsJson() " + d.outputsJson() + ", expected " + TestJson.write(wantOutputs);
        }
        if (!TestJson.same(JavaValues.given(d.outputs()), wantOutputs, mode)) {
            return "outputs() " + TestJson.write(JavaValues.given(d.outputs())) + ", expected "
                    + TestJson.write(wantOutputs);
        }
        String policy = text(expected, "hit_policy");
        Object want;
        if (expected.containsKey("aggregated")) {
            want = expected.get("aggregated");
        } else if (SINGLE_HIT.contains(policy)) {
            want = wantOutputs.isEmpty() ? null : wantOutputs.getFirst();
        } else {
            if (!(attempt(d::outputJson).error() instanceof HitPolicyException)
                    || !(attempt(d::output).error() instanceof HitPolicyException)) {
                return "outputJson() and output() under " + policy + ": expected a HitPolicyException";
            }
            return "";
        }
        if (!TestJson.same(TestJson.parse(d.outputJson()), want)) {
            return "outputJson() " + d.outputJson() + ", expected " + TestJson.write(want);
        }
        if (!TestJson.same(JavaValues.given(d.output()), want, mode)) {
            return "output() " + TestJson.write(JavaValues.given(d.output())) + ", expected " + TestJson.write(want);
        }
        return "";
    }

    private String readJson(Map<?, ?> given, Map<?, ?> expect) {
        if (given.containsKey("table_grid")) {
            throw new Skip("grid");
        }
        Source source = sourceOf(given);
        Limits limits = limitsOf(given);
        if (given.containsKey("ruleset")) {
            try (io.github.cleanrules.ffi.RuleSet rules =
                    io.github.cleanrules.ffi.RuleSet.read(rulesetDocument(given), Format.JSON)) {
                io.github.cleanrules.ffi.Decision[] read = new io.github.cleanrules.ffi.Decision[1];
                String issue = compare(
                        attempt(() -> {
                            read[0] = rules.readDecision(source.text(), source.format(), limits);
                            return TestJson.parse(read[0].write());
                        }),
                        expect,
                        "document",
                        JSON_READ);
                if (!issue.isEmpty() || read[0] == null) {
                    return issue;
                }
                try (io.github.cleanrules.ffi.Decision decision = read[0]) {
                    String readers = checkReaders(decision, object(need(need(expect, "document"), "decision")),
                            Mode.RECORDED);
                    return readers.isEmpty() ? "" : "the decision read: " + readers;
                }
            }
        }
        Attempt outcome = attempt(() -> Document.read(source.text(), source.format(), limits));
        if (outcome.error() != null) {
            return compare(outcome, expect, "document", JSON_READ);
        }
        try (Document document = (Document) outcome.result()) {
            if (expect.containsKey("error")) {
                // Read, but not writable in the form the vector names.
                if (!(given.get("write") instanceof String target)) {
                    return "read a document, expected the error " + TestJson.write(expect.get("error"));
                }
                return compare(attempt(() -> document.write(format(target))), expect, "document", JSON_READ);
            }
            List<String> issues = new ArrayList<>();
            if (expect.get("document") instanceof Map<?, ?> wanted) {
                issues.add(compare(attempt(() -> TestJson.parse(document.write())), expect, "document", JSON_READ));
                String key = document.kind().jsonName();
                if (!wanted.containsKey(key)) {
                    issues.add("kind() " + document.kind() + ", expected the document " + TestJson.write(wanted));
                }
            }
            if (expect.containsKey("printed")) {
                issues.add(compareText("printed", document.write(Format.TEXT), expect.get("printed")));
            }
            if (expect.containsKey("csv")) {
                issues.add(compareText("wrote the CSV", document.write(Format.CSV), expect.get("csv")));
            }
            if (expect.containsKey("compiled")) {
                issues.add(compare(
                        attempt(() -> {
                            try (io.github.cleanrules.ffi.RuleSet compiled =
                                    io.github.cleanrules.ffi.RuleSet.fromDocument(document)) {
                                return content(compiled.write(), "ruleset");
                            }
                        }),
                        expect,
                        "compiled",
                        JSON_READ));
            }
            return join(issues);
        }
    }

    private String runJson(Map<?, ?> vector) {
        Map<String, Object> given = object(need(vector, "given"));
        Map<String, Object> expect = object(need(vector, "expect"));
        String kind = text(given, "kind");
        String logic = given.get("logic") instanceof String name ? name : "boolean";
        boolean strict = Boolean.TRUE.equals(given.get("strict"));
        switch (kind) {
            case "read" -> {
                return readJson(given, expect);
            }
            case "condition", "value" -> {
                try (Node node = Node.read(TestJson.write(need(given, kind)), Format.JSON)) {
                    String input = TestJson.write(need(given, "input"));
                    return compare(
                            attempt(() -> TestJson.parse(kind.equals("value")
                                    ? node.evaluateValueTreeJson(input, strict, null)
                                    : node.evaluateTreeJson(input, logic, strict, null))),
                            expect,
                            "result",
                            JSON);
                }
            }
            case "summary" -> {
                try (io.github.cleanrules.ffi.RuleSet rules =
                        io.github.cleanrules.ffi.RuleSet.read(rulesetDocument(given), Format.JSON)) {
                    String issue = compare(
                            attempt(() -> content(rules.summary(Format.JSON), "summary")), expect, "summary", JSON);
                    if (!issue.isEmpty() || !expect.containsKey("text")) {
                        return issue;
                    }
                    return compareText("summarised", rules.summary(Format.TEXT), expect.get("text"));
                }
            }
            case "evaluate", "explain" -> {
                if (given.containsKey("limits")) {
                    throw new Skip("native");
                }
                try (io.github.cleanrules.ffi.RuleSet rules =
                        io.github.cleanrules.ffi.RuleSet.read(rulesetDocument(given), Format.JSON)) {
                    return evaluateJson(rules, kind, given, expect);
                }
            }
            default -> throw new VectorError("unknown kind " + kind);
        }
    }

    private static String evaluateJson(
            io.github.cleanrules.ffi.RuleSet rules, String kind, Map<?, ?> given, Map<?, ?> expect) {
        if (given.containsKey("inputs")) {
            List<io.github.cleanrules.ffi.Decision> batch = new ArrayList<>();
            String issue = compare(
                    attempt(() -> {
                        batch.addAll(rules.evaluateManyJson(TestJson.write(given.get("inputs"))));
                        List<Object> written = new ArrayList<>();
                        for (io.github.cleanrules.ffi.Decision decision : batch) {
                            written.add(content(decision.write(), "decision"));
                        }
                        return written;
                    }),
                    expect,
                    "decisions",
                    JSON);
            try {
                if (!issue.isEmpty() || batch.isEmpty()) {
                    return issue;
                }
                List<?> decisions = array(need(expect, "decisions"));
                for (int i = 0; i < decisions.size(); i++) {
                    String found = checkReaders(batch.get(i), object(decisions.get(i)), Mode.GIVEN);
                    if (!found.isEmpty()) {
                        return "decision " + i + ": " + found;
                    }
                }
                return "";
            } finally {
                batch.forEach(io.github.cleanrules.ffi.Decision::close);
            }
        }
        io.github.cleanrules.ffi.Decision[] made = new io.github.cleanrules.ffi.Decision[1];
        String issue = compare(
                attempt(() -> {
                    made[0] = rules.evaluateJson(TestJson.write(need(given, "input")));
                    return content(made[0].write(), "decision");
                }),
                expect,
                "decision",
                JSON);
        if (made[0] == null) {
            return issue;
        }
        try (io.github.cleanrules.ffi.Decision decision = made[0]) {
            if (!issue.isEmpty()) {
                return issue;
            }
            String readers = checkReaders(decision, object(need(expect, "decision")), Mode.GIVEN);
            if (!readers.isEmpty() || kind.equals("evaluate")) {
                return readers;
            }
            String explained = compare(
                    attempt(() -> content(decision.explain(Format.JSON), "explanation")), expect, "explanation", JSON);
            if (!explained.isEmpty() || !expect.containsKey("text")) {
                return explained;
            }
            return compareText("explained", decision.explain(), expect.get("text"));
        }
    }

    // ---- Java data.

    /** A data value of the format as Java data, counting the classes used. */
    private Object java(Object json) {
        return JavaValues.toJava(json, form -> forms.computeIfAbsent(form, k -> new TreeSet<>()).add(current));
    }

    private Object inputOf(Map<?, ?> given) {
        if (given.containsKey("inputs")) {
            List<Object> rows = new ArrayList<>();
            for (Object row : array(given.get("inputs"))) {
                rows.add(java(row));
            }
            return rows;
        }
        return java(need(given, "input"));
    }

    // ---- Building in code.

    /** A condition or value node of the format, built with the idiomatic API. */
    private Expr build(Object json) {
        Map<String, Object> node = object(json);
        String op = text(node, "op");
        switch (op) {
            case "field" -> {
                List<Object> path = new ArrayList<>();
                for (Object segment : array(need(node, "path"))) {
                    path.add(segment instanceof Num index ? (Object) Integer.parseInt(index.text()) : segment);
                }
                return Conditions.field(path.toArray());
            }
            case "const" -> {
                return Conditions.constant(java(need(node, "value")));
            }
            case "and", "or" -> {
                List<Expr> operands = new ArrayList<>();
                for (Object operand : array(need(node, "operands"))) {
                    operands.add(build(operand));
                }
                Expr[] all = operands.toArray(Expr[]::new);
                return op.equals("and") ? Conditions.and(all) : Conditions.or(all);
            }
            default -> {
                // Below.
            }
        }
        if (node.containsKey("left")) {
            Expr left = build(need(node, "left"));
            Expr right = build(need(node, "right"));
            return switch (op) {
                case "eq" -> left.eq(right);
                case "ne" -> left.ne(right);
                case "lt" -> left.lt(right);
                case "le" -> left.le(right);
                case "gt" -> left.gt(right);
                case "ge" -> left.ge(right);
                case "add" -> left.add(right);
                case "sub" -> left.sub(right);
                case "mul" -> left.mul(right);
                case "div" -> left.div(right);
                default -> throw new VectorError("unknown op " + op);
            };
        }
        Expr operand = build(need(node, "operand"));
        return switch (op) {
            case "not" -> Conditions.not(operand);
            case "in" -> {
                List<Object> values = new ArrayList<>();
                for (Object value : array(need(node, "values"))) {
                    values.add(java(value));
                }
                yield operand.isIn(values);
            }
            case "between" -> {
                Expr low = build(need(node, "low"));
                Expr high = build(need(node, "high"));
                yield node.get("closed") instanceof String closed
                        ? operand.between(low, high, Closed.valueOf(closed.toUpperCase(Locale.ROOT)))
                        : operand.between(low, high);
            }
            case "is_null" -> operand.isNull();
            case "matches" -> operand.matches(text(node, "pattern"));
            case "starts_with" -> operand.startsWith(text(node, "text"));
            case "ends_with" -> operand.endsWith(text(node, "text"));
            case "contains" -> operand.contains(text(node, "text"));
            case "abs" -> operand.abs();
            case "neg" -> operand.neg();
            default -> throw new VectorError("unknown op " + op);
        };
    }

    /** A rule set of the format, built with {@link Rule} and {@link RuleSet#builder()}. */
    private RuleSet buildRuleSet(Object json) {
        Map<String, Object> content = object(json);
        RuleSet.Builder builder = RuleSet.builder();
        for (Object item : array(need(content, "rules"))) {
            Map<String, Object> source = object(item);
            Rule rule = Rule.named(text(source, "name")).when(build(need(source, "when")));
            if (source.get("then") instanceof Map<?, ?> then) {
                for (Map.Entry<?, ?> entry : then.entrySet()) {
                    rule = rule.then((String) entry.getKey(), build(entry.getValue()));
                }
            }
            if (source.containsKey("priority")) {
                rule = rule.priority(((Number) java(source.get("priority"))).longValue());
            }
            if (source.containsKey("tags")) {
                rule = rule.tags(array(source.get("tags")).stream().map(String.class::cast).toList());
            }
            if (source.get("description") instanceof String description) {
                rule = rule.description(description);
            }
            if (source.containsKey("metadata")) {
                @SuppressWarnings("unchecked") // A map of the format is a map with string keys.
                Map<String, ?> metadata = (Map<String, ?>) java(source.get("metadata"));
                rule = rule.metadata(metadata);
            }
            if (source.get("do") instanceof String name) {
                rule = rule.action(name);
            }
            builder.add(rule);
        }
        if (content.get("hit_policy") instanceof String policy) {
            builder.hitPolicy(HitPolicy.fromJsonName(policy));
        }
        if (content.get("aggregate") instanceof String aggregate) {
            builder.aggregate(Aggregate.fromJsonName(aggregate));
        }
        if (content.get("logic") instanceof String logic) {
            builder.logic(logic);
        }
        if (Boolean.TRUE.equals(content.get("strict"))) {
            builder.strict(true);
        }
        return builder.build();
    }

    /**
     * The rule set built in code must be the one the core reads from the
     * vector (the same document), and its accessors must say what it holds.
     */
    private static String checkBuilt(RuleSet ruleset, Map<?, ?> given) {
        Object built = content(Documents.dumps(ruleset), "ruleset");
        Object read;
        try (io.github.cleanrules.ffi.RuleSet core =
                io.github.cleanrules.ffi.RuleSet.read(rulesetDocument(given), Format.JSON)) {
            read = content(core.write(), "ruleset");
        }
        if (!TestJson.same(built, read)) {
            return "the rule set built in code writes\n  " + TestJson.write(built) + "\nthe core reads\n  "
                    + TestJson.write(read);
        }
        Map<String, Object> source = object(read);
        if (!ruleset.hitPolicy().jsonName().equals(source.get("hit_policy"))
                || !ruleset.logic().equals(source.get("logic"))
                || ruleset.isStrict() != Boolean.TRUE.equals(source.get("strict"))
                || !ruleset.aggregate().map(Aggregate::jsonName).equals(Optional.ofNullable(source.get("aggregate")))) {
            return "the rule set built in code says " + ruleset + " " + ruleset.logic() + " " + ruleset.aggregate();
        }
        // What loads gives back writes the same document.
        Object again = content(Documents.dumps(Documents.loads(Documents.dumps(ruleset))), "ruleset");
        return TestJson.same(again, read) ? "" : "loads(dumps(ruleset)) writes " + TestJson.write(again);
    }

    /** A node built in code must be the one the core reads from the vector. */
    private static String checkBuiltNode(Expr node, Object json) {
        Object built = TestJson.parse(node.toJson());
        Object read;
        try (Node core = Node.read(TestJson.write(json), Format.JSON)) {
            read = TestJson.parse(core.write());
        }
        if (!TestJson.same(built, read)) {
            return "the node built in code writes\n  " + TestJson.write(built) + "\nthe core reads\n  "
                    + TestJson.write(read);
        }
        if (!node.tag().equals(need(read, "op"))) {
            return "tag() " + node.tag() + ", expected " + need(read, "op");
        }
        if (node.kind() == Kind.VALUE) {
            return "";
        }
        Object dumped = content(Documents.dumps(node), "condition");
        if (!TestJson.same(dumped, read)) {
            return "dumps of the condition built in code gives " + TestJson.write(dumped);
        }
        DocumentContent again = Documents.loads(Documents.dumps(node));
        Object reloaded = TestJson.parse(((Expr) again).toJson());
        return TestJson.same(reloaded, read) ? "" : "loads(dumps(node)) writes " + TestJson.write(reloaded);
    }

    // ---- Values given back, as values of the format.

    static Object truthJson(Truth truth) {
        return switch (truth) {
            case TRUE -> Boolean.TRUE;
            case FALSE -> Boolean.FALSE;
            case UNKNOWN -> null;
        };
    }

    private static Map<String, Object> reasonOf(Reason reason) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("code", reason.code());
        if (!reason.paths().isEmpty()) {
            List<Object> paths = new ArrayList<>();
            reason.paths().forEach(path -> paths.add(JavaValues.path(path)));
            out.put("paths", paths);
        }
        if (!reason.types().isEmpty()) {
            out.put("types", new ArrayList<Object>(reason.types()));
        }
        return out;
    }

    /** A {@link ResultTree} in the JSON format (spec/json-format.md, Result trees). */
    private static Map<String, Object> resultTree(ResultTree tree) {
        Map<String, Object> out = new LinkedHashMap<>();
        Expr node = tree.node();
        out.put("op", node.tag());
        if (node instanceof Field field) {
            out.put("path", JavaValues.path(field.path()));
        }
        if (tree.truth().isPresent()) {
            out.put("truth", truthJson(tree.truth().get()));
            if (tree.value() != null) {
                throw new VectorError("a result with a truth value has the value " + tree.value());
            }
        } else {
            out.put("value", JavaValues.given(tree.value()));
        }
        tree.reason().ifPresent(reason -> out.put("reason", reasonOf(reason)));
        if (!tree.children().isEmpty()) {
            List<Object> children = new ArrayList<>();
            tree.children().forEach(child -> children.add(resultTree(child)));
            out.put("children", children);
        }
        return out;
    }

    static Object gapOf(Object gap) {
        return switch (gap) {
            case Period days -> {
                if (days.getYears() != 0 || days.getMonths() != 0) {
                    throw new VectorError("a gap of " + days + ", not of days");
                }
                yield Map.of("days", JavaValues.given((long) days.getDays()));
            }
            case Duration duration -> {
                BigInteger micros = BigInteger.valueOf(duration.getSeconds()).multiply(BigInteger.valueOf(1_000_000))
                        .add(BigInteger.valueOf(duration.getNano() / 1000));
                if (duration.getNano() % 1000 != 0) {
                    throw new VectorError("a gap finer than a microsecond: " + duration);
                }
                yield Map.of("microseconds", JavaValues.given(micros.longValueExact()));
            }
            default -> JavaValues.given(gap);
        };
    }

    private static Object requirementOf(Requirement requirement) {
        if (requirement instanceof CauseGroup group) {
            List<Object> items = new ArrayList<>();
            group.items().forEach(item -> items.add(requirementOf(item)));
            return Map.of(group.kind().jsonName(), items);
        }
        Cause cause = (Cause) requirement;
        boolean holds = switch (cause.truth()) {
            case TRUE -> !cause.negated();
            case FALSE -> cause.negated();
            case UNKNOWN -> false;
        };
        if (cause.holds() != holds) {
            throw new VectorError("a cause " + cause.truth() + ", negated " + cause.negated() + " says holds "
                    + cause.holds());
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("condition", TestJson.parse(cause.node().toJson()));
        out.put("truth", truthJson(cause.truth()));
        out.put("negated", cause.negated());
        List<Object> values = new ArrayList<>();
        for (Cause.FieldValue value : cause.values()) {
            Map<String, Object> pair = new LinkedHashMap<>();
            pair.put("path", JavaValues.path(value.path()));
            pair.put("value", JavaValues.given(value.value()));
            values.add(pair);
        }
        out.put("values", values);
        cause.reason().ifPresent(reason -> out.put("reason", reasonOf(reason)));
        cause.counterfactual().ifPresent(counterfactual -> out.put("counterfactual", counterfactualOf(counterfactual)));
        return out;
    }

    private static Map<String, Object> counterfactualOf(Counterfactual counterfactual) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("path", JavaValues.path(counterfactual.path()));
        out.put("op", counterfactual.op());
        out.put("target", JavaValues.given(counterfactual.target()));
        counterfactual.gap().ifPresent(gap -> out.put("gap", gapOf(gap)));
        return out;
    }

    /** Leaves of a requirement, in order. */
    private static void leaves(Requirement requirement, List<Cause> out) {
        switch (requirement) {
            case Cause cause -> out.add(cause);
            case CauseGroup group -> group.items().forEach(item -> leaves(item, out));
        }
    }

    /**
     * An {@link Explanation} in the JSON format, without the causes' {@code at}
     * (the objects do not say it; {@code toJson()} is compared with it).
     */
    private static Map<String, Object> explanationOf(Decision decision, Explanation explanation) {
        List<Object> rules = new ArrayList<>();
        for (int i = 0; i < explanation.rules().size(); i++) {
            RuleExplanation rule = explanation.rules().get(i);
            if (explanation.forRule(rule.rule()) != rule) {
                throw new VectorError("forRule(" + rule.rule() + ") is not rules().get(" + i + ")");
            }
            Map<String, Object> out = new LinkedHashMap<>();
            out.put("rule", rule.rule());
            out.put("status", rule.status().jsonName());
            rule.note().ifPresent(note -> out.put("note", noteOf(note)));
            rule.achievable().ifPresent(achievable -> out.put("achievable", achievable));
            out.put("requirement", requirementOf(rule.requirement()));
            List<Cause> causes = new ArrayList<>();
            leaves(rule.requirement(), causes);
            if (causes.isEmpty() || causes.size() != rule.causes().size()) {
                throw new VectorError("the rule " + rule.rule() + " has the causes " + rule.causes());
            }
            for (int c = 0; c < causes.size(); c++) {
                if (causes.get(c) != rule.causes().get(c)) {
                    throw new VectorError("causes() of " + rule.rule() + " are not its requirement's, in order");
                }
            }
            rules.add(out);
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("hit_policy", decision.hitPolicy().jsonName());
        out.put("logic", decision.ruleSet().logic());
        out.put("selected", new ArrayList<Object>(explanation.selected()));
        out.put("rules", rules);
        return out;
    }

    private static Map<String, Object> noteOf(Note note) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("kind", note.kind().jsonName());
        out.put("winner", note.winner());
        if (note.priority().isPresent() != note.winnerPriority().isPresent()) {
            throw new VectorError("a note with one priority: " + note);
        }
        if (note.priority().isPresent()) {
            out.put("priority", JavaValues.given(note.priority().getAsLong()));
            out.put("winner_priority", JavaValues.given(note.winnerPriority().getAsLong()));
        }
        return out;
    }

    /** The expected explanation without the causes' {@code at}. */
    static Object withoutAt(Object value) {
        if (value instanceof List<?> items) {
            return items.stream().map(Conformance::withoutAt).toList();
        }
        if (!(value instanceof Map<?, ?> members)) {
            return value;
        }
        boolean cause = members.containsKey("condition") && members.containsKey("at");
        Map<String, Object> out = new LinkedHashMap<>();
        members.forEach((key, item) -> {
            if (cause && key.equals("at")) {
                return;
            }
            boolean data = key.equals("values") || key.equals("counterfactual") || key.equals("condition");
            out.put((String) key, data ? item : withoutAt(item));
        });
        return out;
    }

    /** A {@link Summary} in the JSON format (spec/json-format.md, Summaries). */
    private static Map<String, Object> summaryOf(Summary summary) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("hit_policy", summary.hitPolicy().jsonName());
        summary.aggregate().ifPresent(aggregate -> out.put("aggregate", aggregate.jsonName()));
        out.put("logic", summary.logic());
        if (summary.isStrict()) {
            out.put("strict", true);
        }
        List<Object> rules = new ArrayList<>();
        for (RuleSummary rule : summary.rules()) {
            Map<String, Object> item = new LinkedHashMap<>();
            item.put("name", rule.name());
            item.put("priority", JavaValues.given(rule.priority()));
            item.put("outputs", new ArrayList<Object>(rule.outputs()));
            if (rule.action()) {
                item.put("action", true);
            }
            rule.actionName().ifPresent(name -> item.put("do", name));
            rules.add(item);
        }
        out.put("rules", rules);
        List<Object> fields = new ArrayList<>();
        for (FieldUsage usage : summary.fields()) {
            Map<String, Object> item = new LinkedHashMap<>();
            item.put("path", JavaValues.path(usage.path()));
            item.put("conditions", new ArrayList<Object>(usage.conditions()));
            List<Object> payloads = new ArrayList<>();
            for (FieldUsage.Payload payload : usage.payloads()) {
                Map<String, Object> pair = new LinkedHashMap<>();
                pair.put("rule", payload.rule());
                pair.put("key", payload.key());
                payloads.add(pair);
            }
            item.put("payloads", payloads);
            fields.add(item);
        }
        out.put("fields", fields);
        out.put("outputs", new ArrayList<Object>(summary.outputs()));
        List<Object> tags = new ArrayList<>();
        summary.tags().forEach((tag, names) -> {
            Map<String, Object> item = new LinkedHashMap<>();
            item.put("tag", tag);
            item.put("rules", new ArrayList<Object>(names));
            tags.add(item);
        });
        out.put("tags", tags);
        return out;
    }

    /** A {@link Decision} of the idiomatic API against the expected decision. */
    private static String checkDecision(RuleSet ruleset, Decision d, Map<?, ?> expected, Mode mode) {
        List<?> results = array(need(expected, "results"));
        if (d.ruleSet() != ruleset) {
            return "ruleSet() is another rule set";
        }
        if (!d.hitPolicy().jsonName().equals(need(expected, "hit_policy"))) {
            return "hitPolicy() " + d.hitPolicy();
        }
        if (d.results().size() != results.size()) {
            return d.results().size() + " results, expected " + results.size();
        }
        List<Map<String, Object>> outputs = new ArrayList<>();
        for (int i = 0; i < results.size(); i++) {
            Map<String, Object> result = object(results.get(i));
            RuleResult got = d.results().get(i);
            String at = "results().get(" + i + "): ";
            if (got.rule() != ruleset.rules().get(i) || !got.rule().name().equals(need(result, "rule"))) {
                return at + "rule() " + got.rule().name();
            }
            if (got.fired() != (Boolean) need(result, "fired")) {
                return at + "fired() " + got.fired();
            }
            Truth truth = truthOf(need(need(result, "when"), "truth"));
            if (got.truth() != truth) {
                return at + "truth() " + got.truth() + ", expected " + truth;
            }
            Map<String, Object> output = outputOf(result);
            outputs.add(output);
            if (output == null ? got.output().isPresent()
                    : got.output().isEmpty() || !TestJson.same(JavaValues.given(got.output().get()), output, mode)) {
                return at + "output() " + got.output().map(JavaValues::given).map(TestJson::write) + ", expected "
                        + TestJson.write(output);
            }
        }
        List<Object> names = new ArrayList<>();
        List<Object> wantOutputs = new ArrayList<>();
        for (RuleResult selected : d.selected()) {
            int position = d.results().indexOf(selected);
            if (position < 0 || d.results().get(position) != selected) {
                return "selected() holds a result that is not in results()";
            }
            names.add(selected.rule().name());
            Map<String, Object> output = outputs.get(position);
            wantOutputs.add(output == null ? new LinkedHashMap<>() : output);
        }
        if (!TestJson.same(names, need(expected, "selected"))) {
            return "selected() " + TestJson.write(names);
        }
        List<Object> fired = new ArrayList<>();
        for (Object result : results) {
            if ((Boolean) need(result, "fired")) {
                fired.add(need(result, "rule"));
            }
        }
        if (!TestJson.same(new ArrayList<Object>(d.fired()), fired)) {
            return "fired() " + d.fired();
        }
        if (!TestJson.same(JavaValues.given(d.outputs()), wantOutputs, mode)) {
            return "outputs() " + TestJson.write(JavaValues.given(d.outputs())) + ", expected "
                    + TestJson.write(wantOutputs);
        }
        String policy = text(expected, "hit_policy");
        Object want;
        if (expected.containsKey("aggregated")) {
            want = expected.get("aggregated");
        } else if (SINGLE_HIT.contains(policy)) {
            want = wantOutputs.isEmpty() ? null : wantOutputs.getFirst();
        } else {
            if (!(attempt(d::output).error() instanceof HitPolicyException)) {
                return "output() under " + policy + ": expected a HitPolicyException";
            }
            return d.aggregated().isEmpty() ? "" : "aggregated() is not empty without an aggregate";
        }
        Object output = JavaValues.given(d.output().orElse(null));
        if (!TestJson.same(output, want, mode)) {
            return "output() " + TestJson.write(output) + ", expected " + TestJson.write(want);
        }
        Optional<Map<String, Object>> aggregated = expected.containsKey("aggregated") ? d.output() : Optional.empty();
        return d.aggregated().equals(aggregated) ? "" : "aggregated() differs from output()";
    }

    /**
     * A decision's document ({@code toJson()}) is the expected one, and it
     * reads back with {@code readDecision} as the same decision.
     */
    private static String checkRecorded(RuleSet ruleset, Decision decision, Map<?, ?> expected) {
        Object written = content(decision.toJson(), "decision");
        if (!TestJson.same(written, expected)) {
            return "toJson()\n  got  " + TestJson.write(written) + "\n  want " + TestJson.write(expected);
        }
        Attempt read = attempt(() -> ruleset.readDecision(decision.toJson()));
        if (read.error() != null) {
            return "readDecision(toJson()): " + read.error().code() + ": " + read.error().getMessage();
        }
        try (Decision again = (Decision) read.result()) {
            String issue = checkDecision(ruleset, again, expected, Mode.RECORDED);
            return issue.isEmpty() ? "" : "readDecision(toJson()): " + issue;
        }
    }

    // ---- The native route: the idiomatic API.

    private String readNative(Map<?, ?> given, Map<?, ?> expect) {
        Limits limits = limitsOf(given);
        if (given.containsKey("ruleset")) {
            Source source = sourceOf(given);
            RuleSet ruleset = buildRuleSet(need(given, "ruleset"));
            String built = checkBuilt(ruleset, given);
            if (!built.isEmpty()) {
                return built;
            }
            Attempt outcome = attempt(() -> ruleset.readDecision(source.text(), source.format(), limits));
            if (outcome.error() != null || expect.containsKey("error")) {
                return compare(outcome, expect, "document", NATIVE_READ);
            }
            try (Decision decision = (Decision) outcome.result()) {
                Map<String, Object> expected = object(need(need(expect, "document"), "decision"));
                String issue = checkDecision(ruleset, decision, expected, Mode.RECORDED);
                return issue.isEmpty() ? checkRecorded(ruleset, decision, expected) : issue;
            }
        }
        Attempt outcome;
        String text = null;
        if (given.get("table_grid") instanceof Map<?, ?> grid) {
            outcome = attempt(() -> table(grid, limits));
        } else {
            Source source = sourceOf(given);
            text = source.format() == Format.TEXT ? source.text() : null;
            outcome = attempt(() -> Documents.loads(source.text(), source.format(), limits, null));
        }
        List<String> issues = new ArrayList<>();
        if (text != null) {
            issues.add(fromText(text, limits, expect));
        }
        if (outcome.error() != null) {
            issues.add(compare(outcome, expect, "document", NATIVE_READ));
            return join(issues);
        }
        DocumentContent loaded = (DocumentContent) outcome.result();
        if (expect.containsKey("error")) {
            if (!(given.get("write") instanceof String target)) {
                return "read a " + loaded.getClass().getSimpleName() + ", expected the error "
                        + TestJson.write(expect.get("error"));
            }
            issues.add(compare(attempt(() -> Documents.dumps(loaded, format(target))), expect, "document", NATIVE_READ));
            return join(issues);
        }
        if (expect.get("document") instanceof Map<?, ?> wanted) {
            issues.add(compare(attempt(() -> TestJson.parse(Documents.dumps(loaded))), expect, "document", NATIVE_READ));
            String key = switch (loaded) {
                case Expr _ -> "condition";
                case Rule _ -> "rule";
                case RuleSet _ -> "ruleset";
                case DecisionTable _ -> "decision_table";
            };
            if (!wanted.containsKey(key)) {
                issues.add("loads gave a " + loaded.getClass().getSimpleName() + ", expected " + TestJson.write(wanted));
            }
            Object again = TestJson.parse(Documents.dumps(Documents.loads(Documents.dumps(loaded))));
            if (!TestJson.same(again, wanted)) {
                issues.add("loads(dumps(x)) writes " + TestJson.write(again));
            }
            issues.add(checkContent(loaded, wanted));
        }
        if (expect.containsKey("printed")) {
            issues.add(compareText("printed", Documents.dumps(loaded, Format.TEXT), expect.get("printed")));
        }
        if (expect.containsKey("csv")) {
            issues.add(compareText("wrote the CSV", Documents.dumps(loaded, Format.CSV), expect.get("csv")));
        }
        if (expect.containsKey("compiled")) {
            if (!(loaded instanceof DecisionTable table)) {
                return "read a " + loaded.getClass().getSimpleName() + ", expected a DecisionTable";
            }
            issues.add(compare(
                    attempt(() -> content(Documents.dumps(table.ruleSet()), "ruleset")), expect, "compiled", NATIVE_READ));
        }
        return join(issues);
    }

    /** What a document's objects say, against the document: names, priorities, constants given back. */
    private static String checkContent(DocumentContent loaded, Map<?, ?> wanted) {
        switch (loaded) {
            case Expr expr -> {
                return checkConstants(expr);
            }
            case Rule rule -> {
                return checkRule(rule, object(wanted.get("rule")));
            }
            case RuleSet rules -> {
                List<?> sources = array(need(wanted.get("ruleset"), "rules"));
                if (rules.size() != sources.size()) {
                    return "size() " + rules.size();
                }
                List<String> issues = new ArrayList<>();
                for (int i = 0; i < sources.size(); i++) {
                    issues.add(checkRule(rules.rules().get(i), object(sources.get(i))));
                }
                return join(issues);
            }
            case DecisionTable _ -> {
                return "";
            }
        }
    }

    private static String checkRule(Rule rule, Map<?, ?> source) {
        if (!rule.name().equals(source.get("name"))) {
            return "name() " + rule.name();
        }
        Object priority = source.containsKey("priority") ? source.get("priority") : Num.of(0);
        if (!TestJson.same(JavaValues.given(rule.priority()), priority, Mode.GIVEN)) {
            return rule.name() + ": priority() " + rule.priority();
        }
        if (!rule.actionName().equals(Optional.ofNullable(source.get("do")))) {
            return rule.name() + ": actionName() " + rule.actionName();
        }
        if (!rule.description().equals(Optional.ofNullable(source.get("description")))) {
            return rule.name() + ": description() " + rule.description();
        }
        // Metadata's keys are literal, even those that start with $ (spec/json-format.md, Rules).
        Map<String, Object> metadata = new LinkedHashMap<>();
        rule.metadata().forEach((key, value) -> metadata.put(key, JavaValues.given(value)));
        Object wantedMetadata = source.containsKey("metadata") ? source.get("metadata") : Map.of();
        if (!TestJson.same(metadata, wantedMetadata, Mode.GIVEN)) {
            return rule.name() + ": metadata() " + TestJson.write(metadata) + ", expected " + TestJson.write(wantedMetadata);
        }
        Object when = TestJson.parse(rule.when().toJson());
        if (!TestJson.same(when, source.get("when"))) {
            return rule.name() + ": when() " + TestJson.write(when);
        }
        List<Object> keys = new ArrayList<>(rule.then().keySet());
        List<Object> wantKeys = source.get("then") instanceof Map<?, ?> then ? new ArrayList<>(then.keySet()) : List.of();
        if (!keys.equals(wantKeys)) {
            return rule.name() + ": then() keys " + keys;
        }
        List<String> issues = new ArrayList<>();
        issues.add(checkConstants(rule.when()));
        rule.then().values().forEach(value -> issues.add(checkConstants(value)));
        return join(issues);
    }

    /** Each constant's {@code value()} is the constant the node writes, as a value given back. */
    private static String checkConstants(Expr expr) {
        for (Expr node : expr.walk()) {
            if (node instanceof Const constant) {
                Object written = need(TestJson.parse(constant.toJson()), "value");
                Object value = JavaValues.given(constant.value());
                if (!TestJson.same(value, written, Mode.GIVEN)) {
                    return "the constant " + constant.toJson() + " gives back " + TestJson.write(value);
                }
            }
        }
        return "";
    }

    /** A grid as a {@link DecisionTable}: text cells as text, other cells as Java data. */
    private DecisionTable table(Map<?, ?> grid, Limits limits) {
        DecisionTable.Builder builder = DecisionTable.builder().limits(limits);
        if (grid.get("hit_policy") instanceof String policy) {
            builder.hitPolicy(TablePolicy.fromJsonName(policy));
        }
        if (grid.get("aggregate") instanceof String aggregate) {
            builder.aggregate(Aggregate.fromJsonName(aggregate));
        }
        if (grid.get("logic") instanceof String logic) {
            builder.logic(logic);
        }
        if (Boolean.TRUE.equals(grid.get("strict"))) {
            builder.strict(true);
        }
        List<Object> header = new ArrayList<>();
        for (Object item : array(need(grid, "header"))) {
            header.add(item instanceof String text ? text : java(item));
        }
        builder.header(header);
        for (Object row : array(need(grid, "rows"))) {
            List<Object> cells = new ArrayList<>();
            for (Object cell : array(row)) {
                cells.add(cell instanceof String text ? text : java(cell));
            }
            builder.row(cells);
        }
        return builder.build();
    }

    /**
     * {@code ruleFromText} on a rule text: the rule {@code loads} reads, a
     * {@code rule_syntax_error} for a condition or a rule set (spec/text.md),
     * and an error where {@code loads} fails.
     */
    private static String fromText(String text, Limits limits, Map<?, ?> expect) {
        Attempt outcome = attempt(() -> Documents.ruleFromText(text, limits));
        if (expect.containsKey("error") || !(expect.get("document") instanceof Map<?, ?> document)) {
            if (expect.containsKey("error") && outcome.error() == null) {
                return "ruleFromText read a rule, expected the error " + TestJson.write(expect.get("error"));
            }
            return "";
        }
        if (document.containsKey("rule")) {
            if (outcome.error() != null) {
                return "ruleFromText: unexpected error " + TestJson.write(contextForm(outcome.error()));
            }
            Object written = TestJson.parse(Documents.dumps((Rule) outcome.result()));
            return TestJson.same(written, document) ? "" : "ruleFromText wrote " + TestJson.write(written);
        }
        if (outcome.error() == null || !outcome.error().code().equals("rule_syntax_error")) {
            return "ruleFromText of a " + (document.containsKey("condition") ? "condition" : "rule set")
                    + ": expected a rule_syntax_error";
        }
        checkClass(outcome.error());
        return "";
    }

    private String runNative(Map<?, ?> vector) {
        Map<String, Object> given = object(need(vector, "given"));
        Map<String, Object> expect = object(need(vector, "expect"));
        String kind = text(given, "kind");
        if (kind.equals("read")) {
            return readNative(given, expect);
        }
        if (kind.equals("summary")) {
            return summaryNative(given, expect);
        }
        if (kind.equals("condition") || kind.equals("value")) {
            return treeNative(kind, given, expect);
        }
        if (!kind.equals("evaluate") && !kind.equals("explain")) {
            throw new VectorError("unknown kind " + kind);
        }
        RuleSet ruleset = buildRuleSet(need(given, "ruleset"));
        String built = checkBuilt(ruleset, given);
        if (!built.isEmpty()) {
            return built;
        }
        Object input = inputOf(given);
        boolean failing = expect.containsKey("error");
        // `max_values`, for the idiomatic API and the native layer.
        Limits limits = limitsOf(given);
        if (given.containsKey("inputs")) {
            List<?> rows = (List<?>) input;
            Attempt outcome = attempt(() -> ruleset.evaluateMany(rows, limits));
            if (outcome.error() != null || failing) {
                String issue = compare(outcome, expect, "decisions", NATIVE);
                if (!issue.isEmpty()) {
                    return issue;
                }
            } else {
                List<?> decisions = array(need(expect, "decisions"));
                List<?> made = (List<?>) outcome.result();
                if (made.size() != decisions.size()) {
                    return made.size() + " decisions, expected " + decisions.size();
                }
                for (int i = 0; i < made.size(); i++) {
                    try (Decision decision = (Decision) made.get(i)) {
                        Map<String, Object> expected = object(decisions.get(i));
                        String issue = checkDecision(ruleset, decision, expected, Mode.GIVEN);
                        if (issue.isEmpty()) {
                            issue = checkRecorded(ruleset, decision, expected);
                        }
                        if (!issue.isEmpty()) {
                            return "decision " + i + ": " + issue;
                        }
                    }
                }
            }
            // The same Java data through the native layer, compared exactly.
            try (io.github.cleanrules.ffi.RuleSet layer =
                    io.github.cleanrules.ffi.RuleSet.read(rulesetDocument(given), Format.JSON)) {
                String exact = compare(
                        attempt(() -> {
                            List<Object> written = new ArrayList<>();
                            for (io.github.cleanrules.ffi.Decision decision : layer.evaluateMany(rows, limits)) {
                                try (decision) {
                                    written.add(content(decision.write(), "decision"));
                                }
                            }
                            return written;
                        }),
                        expect,
                        "decisions",
                        JSON);
                return exact.isEmpty() ? "" : "native layer: " + exact;
            }
        }
        Attempt outcome = attempt(() -> ruleset.evaluate(input, limits));
        if (outcome.error() != null || failing) {
            String issue = compare(outcome, expect, "decision", NATIVE);
            if (!issue.isEmpty()) {
                return issue;
            }
        } else {
            try (Decision decision = (Decision) outcome.result()) {
                Map<String, Object> expected = object(need(expect, "decision"));
                String issue = checkDecision(ruleset, decision, expected, Mode.GIVEN);
                if (issue.isEmpty()) {
                    issue = checkRecorded(ruleset, decision, expected);
                }
                if (!issue.isEmpty()) {
                    return issue;
                }
                if (kind.equals("explain")) {
                    Explanation explanation = decision.explain();
                    String explained = compare(
                            attempt(() -> explanationOf(decision, explanation)),
                            expect,
                            "explanation",
                            new How(false, Conformance::contextForm, Mode.GIVEN, Conformance::withoutAt));
                    if (!explained.isEmpty()) {
                        return explained;
                    }
                    String json = compare(
                            attempt(() -> content(explanation.toJson(), "explanation")), expect, "explanation", NATIVE);
                    if (!json.isEmpty()) {
                        return "Explanation.toJson(): " + json;
                    }
                    if (expect.containsKey("text")) {
                        String text = compareText("explained", explanation.toString(), expect.get("text"));
                        if (!text.isEmpty()) {
                            return text;
                        }
                    }
                }
            }
        }
        // The same Java data through the native layer, compared exactly.
        try (io.github.cleanrules.ffi.RuleSet layer =
                io.github.cleanrules.ffi.RuleSet.read(rulesetDocument(given), Format.JSON)) {
            io.github.cleanrules.ffi.Decision[] made = new io.github.cleanrules.ffi.Decision[1];
            String written = compare(
                    attempt(() -> {
                        made[0] = layer.evaluate(input, limits);
                        return content(made[0].write(), "decision");
                    }),
                    expect,
                    "decision",
                    JSON);
            if (made[0] == null) {
                return written.isEmpty() ? "" : "native layer: " + written;
            }
            try (io.github.cleanrules.ffi.Decision decision = made[0]) {
                if (!written.isEmpty() || kind.equals("evaluate") || failing) {
                    return written.isEmpty() ? "" : "native layer: " + written;
                }
                String explained = compare(
                        attempt(() -> content(decision.explain(Format.JSON), "explanation")), expect, "explanation", JSON);
                return explained.isEmpty() ? "" : "native layer: " + explained;
            }
        }
    }

    private String summaryNative(Map<?, ?> given, Map<?, ?> expect) {
        RuleSet ruleset = buildRuleSet(need(given, "ruleset"));
        String built = checkBuilt(ruleset, given);
        if (!built.isEmpty()) {
            return built;
        }
        Summary summary = ruleset.summary();
        String issue = compare(attempt(() -> summaryOf(summary)), expect, "summary", GIVEN);
        if (issue.isEmpty()) {
            issue = compare(attempt(() -> content(summary.toJson(), "summary")), expect, "summary", NATIVE);
        }
        if (!issue.isEmpty() || !expect.containsKey("text")) {
            return issue;
        }
        return compareText("summarised", summary.toString(), expect.get("text"));
    }

    private String treeNative(String kind, Map<?, ?> given, Map<?, ?> expect) {
        Object json = need(given, kind);
        boolean strict = Boolean.TRUE.equals(given.get("strict"));
        String logic = given.get("logic") instanceof String name ? name : "boolean";
        Expr node = build(json);
        String built = checkBuiltNode(node, json);
        if (!built.isEmpty()) {
            return built;
        }
        String constants = checkConstants(node);
        if (!constants.isEmpty()) {
            return constants;
        }
        Object input = inputOf(given);
        ResultTree[] tree = new ResultTree[1];
        String evaluated = compare(
                attempt(() -> {
                    tree[0] = kind.equals("value")
                            ? node.evaluateValueTree(input, strict)
                            : node.evaluateTree(input, logic, strict);
                    return resultTree(tree[0]);
                }),
                expect,
                "result",
                GIVEN);
        if (!evaluated.isEmpty()) {
            return evaluated;
        }
        if (tree[0] != null) {
            String json2 = compare(attempt(() -> TestJson.parse(tree[0].toJson())), expect, "result", NATIVE);
            if (!json2.isEmpty()) {
                return "ResultTree.toJson(): " + json2;
            }
        }
        // The same Java data through the native layer, compared exactly.
        try (Node layer = Node.read(TestJson.write(json), Format.JSON)) {
            String exact = compare(
                    attempt(() -> TestJson.parse(kind.equals("value")
                            ? layer.evaluateValueTree(input, strict)
                            : layer.evaluateTree(input, logic, strict))),
                    expect,
                    "result",
                    JSON);
            return exact.isEmpty() ? "" : "native layer: " + exact;
        }
    }

    // ---- The comparison itself.

    /**
     * The comparison on cases the vectors may not reach (as the C++ and
     * JavaScript runners check theirs): the cases it gets wrong.
     */
    static List<String> checkComparison() {
        Object[][] cases = {
            {"18446744073709551617", "18446744073709551617", true}, // beyond 64 bits
            {"18446744073709551617", "18446744073709551616", false},
            {"9007199254740993", "9007199254740992", false}, // beyond 2^53, one apart
            {"-0", "0", true}, // the int 0
            {"1", "1.0", false},
            {"100.0", "1e2", true},
            {"0.1", "0.10000000000000001", true}, // the same double
            {"-0.0", "0.0", false},
            {"[1,2]", "[2,1]", false},
            {"{\"a\":1,\"b\":2}", "{\"b\":2,\"a\":1}", false},
            {"{\"a\":1}", "{\"a\":1,\"b\":2}", false},
            {"{\"$opaque\":\"socket\"}", "{\"$opaque\":\"bytes\"}", true},
            {"{\"$opaque\":\"socket\"}", "null", false},
            {"\"é😀\"", "\"\\u00e9\\ud83d\\ude00\"", true},
            {"true", "1", false},
            {"null", "false", false},
            {"{\"code\":\"null\",\"detail\":\"a\"}", "{\"code\":\"null\"}", false}, // detail only inside a reason
            {"{\"reason\":{\"code\":\"null\",\"detail\":\"a\"}}", "{\"reason\":{\"code\":\"null\"}}", true},
            {"{\"detail\":1}", "{\"detail\":2}", false}, // a map's member named detail
            {"{\"$float\":\"inf\"}", "{\"$decimal\":\"inf\"}", false}, // only for values given back
        };
        List<String> wrong = new ArrayList<>();
        for (Object[] c : cases) {
            if (TestJson.same(TestJson.parse((String) c[0]), TestJson.parse((String) c[1])) != (Boolean) c[2]) {
                wrong.add("comparison: " + c[0] + ((Boolean) c[2] ? " == " : " != ") + c[1] + " does not hold");
            }
        }
        // What Java gives back for what it cannot hold, in that mode only.
        Object[][] given = {
            {"{\"$float\":\"inf\"}", "{\"$decimal\":\"inf\"}", true},
            {"{\"$float\":\"-inf\"}", "{\"$decimal\":\"inf\"}", false},
            {"{\"$decimal\":\"inf\"}", "{\"$decimal\":\"inf\"}", false},
            {"{\"$datetime\":\"2026-10-31T09:30:00+01:00\"}", "{\"$datetime\":\"2026-10-31T09:30:00+01:00[Mars/Olympus]\"}",
                true},
            {"{\"$datetime\":\"2026-10-31T09:30:00+01:00\"}", "{\"$datetime\":\"2026-10-31T09:30:00+01:00[Europe/Madrid]\"}",
                false},
            {"1", "1.0", false},
            {"null", "{\"$opaque\":\"x\"}", false},
        };
        for (Object[] c : given) {
            if (TestJson.same(TestJson.parse((String) c[0]), TestJson.parse((String) c[1]), Mode.GIVEN) != (Boolean) c[2]) {
                wrong.add("given: " + c[0] + ((Boolean) c[2] ? " == " : " != ") + c[1] + " does not hold");
            }
        }
        if (!TestJson.same(null, TestJson.parse("{\"$opaque\":\"x\"}"), Mode.RECORDED)) {
            wrong.add("recorded: null != {\"$opaque\":\"x\"}");
        }
        return wrong;
    }
}
