package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.Format;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.Node;
import io.github.cleanrules.ffi.RuleSet;
import java.io.IOException;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.OffsetDateTime;
import java.time.ZonedDateTime;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Supplier;
import org.junit.jupiter.api.Test;

/**
 * A smoke pass of the conformance vectors (conformance/README.md) over the
 * hand-written evaluation files: every vector through the JSON entry points
 * ({@code json:<id>}), and through Java data converted from the vector's input
 * ({@code native:<id>}). The full runner, with the documents read in every
 * format, is task G.3.
 */
class ConformanceSmokeTest {
    private static final List<String> FILES = List.of(
            "ast/missing.json",
            "ast/nodes.json",
            "ast/reading.json",
            "ast/reasons.json",
            "ast/truth.json",
            "ast/types.json",
            "explain/counterfactuals.json",
            "explain/notes.json",
            "explain/requirements.json",
            "explain/summaries.json",
            "rules/aggregates.json",
            "rules/batch.json",
            "rules/payloads.json",
            "rules/policies.json",
            "rules/strict.json");

    /** The compared fields of an evaluation error, as the C ABI's runner compares them. */
    private static final List<String> ERROR_FIELDS = List.of("code", "rule", "path", "types", "rules", "row");

    /** An input that Java data cannot hold (a decimal infinity). */
    private static final class Skip extends Exception {
        private static final long serialVersionUID = 1L;
    }

    private final List<String> failures = new ArrayList<>();
    private int json;
    private int nativeRuns;
    private int skipped;

    @Test
    void theVectorsPassThroughJsonAndJavaData() throws IOException {
        Path root = Path.of(System.getProperty("cleanrules.conformance.vectors", "../../../conformance/vectors"));
        for (String file : FILES) {
            Map<?, ?> document = (Map<?, ?>) TestJson.parse(Files.readString(root.resolve(file)));
            for (Object vector : (List<?>) document.get("vectors")) {
                run((Map<?, ?>) vector);
            }
        }
        assertTrue(failures.isEmpty(), failures.size() + " failed:\n" + String.join("\n", failures));
        // Every vector of these files through JSON, and all but the summaries
        // through Java data: none of their inputs is beyond Java's values.
        assertEquals(131, json, "vectors through JSON");
        assertEquals(127, nativeRuns, "vectors through Java data");
        assertEquals(0, skipped, "vectors whose input Java data cannot hold");
    }

    private void run(Map<?, ?> vector) {
        String id = (String) vector.get("id");
        Map<?, ?> given = (Map<?, ?>) vector.get("given");
        Map<?, ?> expect = (Map<?, ?>) vector.get("expect");
        String kind = (String) given.get("kind");
        check("json:" + id, expect, () -> viaJson(kind, given));
        json++;
        if (kind.equals("summary")) {
            return;
        }
        Object data;
        try {
            data = given.containsKey("inputs") ? toJava(given.get("inputs")) : toJava(given.get("input"));
        } catch (Skip skip) {
            skipped++;
            return;
        }
        check("native:" + id, expect, () -> viaJava(kind, given, data));
        nativeRuns++;
    }

    /** What the vector expects: its key and the value, or the error. */
    private void check(String id, Map<?, ?> expect, Supplier<Map.Entry<String, Object>> run) {
        Map<?, ?> error = (Map<?, ?>) expect.get("error");
        Map.Entry<String, Object> actual;
        try {
            actual = run.get();
        } catch (CleanRulesException thrown) {
            if (error == null) {
                failures.add(id + ": unexpected error " + thrown.json());
                return;
            }
            Map<String, Object> fields = new LinkedHashMap<>();
            ((Map<?, ?>) TestJson.parse(thrown.json())).forEach((key, value) -> {
                if (ERROR_FIELDS.contains(key)) {
                    fields.put((String) key, value);
                }
            });
            if (!sameMembers(fields, error)) {
                failures.add(id + ": error " + TestJson.write(fields) + ", expected " + TestJson.write(error));
            }
            return;
        } catch (RuntimeException thrown) {
            failures.add(id + ": " + thrown);
            return;
        }
        if (error != null) {
            failures.add(id + ": got " + TestJson.write(actual.getValue()) + ", expected " + TestJson.write(error));
            return;
        }
        Object wanted = TestJson.withoutDetail(expect.get(actual.getKey()));
        Object got = TestJson.withoutDetail(actual.getValue());
        if (!TestJson.same(got, wanted)) {
            failures.add(id + ": got " + TestJson.write(got) + "\n  want " + TestJson.write(wanted));
        }
    }

    private static boolean sameMembers(Map<?, ?> actual, Map<?, ?> wanted) {
        if (actual.size() != wanted.size()) {
            return false;
        }
        for (Map.Entry<?, ?> member : wanted.entrySet()) {
            if (!actual.containsKey(member.getKey()) || !TestJson.same(actual.get(member.getKey()), member.getValue())) {
                return false;
            }
        }
        return true;
    }

    private static RuleSet ruleset(Map<?, ?> given) {
        return RuleSet.read("{\"version\":1,\"ruleset\":" + TestJson.write(given.get("ruleset")) + "}", Format.JSON);
    }

    /** The member {@code key} of a document the binding wrote. */
    private static Object content(String text, String key) {
        return ((Map<?, ?>) TestJson.parse(text)).get(key);
    }

    private static Map.Entry<String, Object> viaJson(String kind, Map<?, ?> given) {
        String logic = given.get("logic") instanceof String name ? name : "boolean";
        boolean strict = Boolean.TRUE.equals(given.get("strict"));
        String input = TestJson.write(given.get("input"));
        return switch (kind) {
            case "condition", "value" -> {
                try (Node node = Node.read(TestJson.write(given.get(kind)), Format.JSON)) {
                    String tree = kind.equals("value")
                            ? node.evaluateValueTreeJson(input, strict, null)
                            : node.evaluateTreeJson(input, logic, strict, null);
                    yield Map.entry("result", TestJson.parse(tree));
                }
            }
            case "summary" -> {
                try (RuleSet rules = ruleset(given)) {
                    yield Map.entry("summary", content(rules.summary(Format.JSON), "summary"));
                }
            }
            case "evaluate", "explain" -> {
                try (RuleSet rules = ruleset(given)) {
                    if (given.containsKey("inputs")) {
                        List<Decision> decisions = rules.evaluateManyJson(TestJson.write(given.get("inputs")), null);
                        yield Map.entry("decisions", written(decisions));
                    }
                    try (Decision decision = rules.evaluateJson(input)) {
                        yield decided(kind, decision);
                    }
                }
            }
            default -> throw new IllegalStateException("unknown kind " + kind);
        };
    }

    private static Map.Entry<String, Object> viaJava(String kind, Map<?, ?> given, Object data) {
        String logic = given.get("logic") instanceof String name ? name : "boolean";
        boolean strict = Boolean.TRUE.equals(given.get("strict"));
        return switch (kind) {
            case "condition", "value" -> {
                try (Node node = Node.read(TestJson.write(given.get(kind)), Format.JSON)) {
                    String tree = kind.equals("value")
                            ? node.evaluateValueTree(data, strict)
                            : node.evaluateTree(data, logic, strict);
                    yield Map.entry("result", TestJson.parse(tree));
                }
            }
            case "evaluate", "explain" -> {
                try (RuleSet rules = ruleset(given)) {
                    if (given.containsKey("inputs")) {
                        yield Map.entry("decisions", written(rules.evaluateMany((List<?>) data)));
                    }
                    try (Decision decision = rules.evaluate(data)) {
                        yield decided(kind, decision);
                    }
                }
            }
            default -> throw new IllegalStateException("unknown kind " + kind);
        };
    }

    private static List<Object> written(List<Decision> decisions) {
        List<Object> written = new ArrayList<>();
        for (Decision decision : decisions) {
            written.add(content(decision.write(), "decision"));
            decision.close();
        }
        return written;
    }

    private static Map.Entry<String, Object> decided(String kind, Decision decision) {
        Object written = content(decision.write(), "decision");
        if (kind.equals("explain")) {
            return Map.entry("explanation", content(decision.explain(Format.JSON), "explanation"));
        }
        return Map.entry("decision", written);
    }

    /** A vector's data value as Java data (spec/data-model.md, Java and Scala). */
    private static Object toJava(Object json) throws Skip {
        switch (json) {
            case null -> {
                return null;
            }
            case TestJson.Num number -> {
                if (number.integral()) {
                    return integer(new BigInteger(number.text()));
                }
                return Double.parseDouble(number.text());
            }
            case List<?> items -> {
                List<Object> list = new ArrayList<>();
                for (Object item : items) {
                    list.add(toJava(item));
                }
                return list;
            }
            case Map<?, ?> members -> {
                if (members.size() == 1) {
                    Map.Entry<?, ?> only = members.entrySet().iterator().next();
                    String key = (String) only.getKey();
                    Object value = only.getValue();
                    switch (key) {
                        case "$int" -> {
                            return integer(new BigInteger(value instanceof TestJson.Num n ? n.text() : (String) value));
                        }
                        case "$float" -> {
                            if (value instanceof TestJson.Num n) {
                                return Double.parseDouble(n.text());
                            }
                            return value.equals("inf") ? Double.POSITIVE_INFINITY : Double.NEGATIVE_INFINITY;
                        }
                        case "$decimal" -> {
                            if (value.equals("inf") || value.equals("-inf")) {
                                throw new Skip();
                            }
                            return new BigDecimal((String) value);
                        }
                        case "$date" -> {
                            return LocalDate.parse((String) value);
                        }
                        case "$datetime" -> {
                            return datetime((String) value);
                        }
                        case "$map" -> {
                            return map((Map<?, ?>) value);
                        }
                        default -> {
                            // An ordinary map with one member.
                        }
                    }
                }
                return map(members);
            }
            default -> {
                return json;
            }
        }
    }

    private static Map<String, Object> map(Map<?, ?> members) throws Skip {
        Map<String, Object> map = new LinkedHashMap<>();
        for (Map.Entry<?, ?> member : members.entrySet()) {
            map.put((String) member.getKey(), toJava(member.getValue()));
        }
        return map;
    }

    private static Object integer(BigInteger value) {
        return value.bitLength() < 64 ? (Object) value.longValueExact() : value;
    }

    private static Object datetime(String text) throws Skip {
        if (text.contains("[")) {
            ZonedDateTime zoned = ZonedDateTime.parse(text, DateTimeFormatter.ISO_ZONED_DATE_TIME);
            OffsetDateTime written = OffsetDateTime.parse(text.substring(0, text.indexOf('[')));
            if (!zoned.getOffset().equals(written.getOffset())) {
                // Java's zone rules disagree with the offset written: no Java value holds both.
                throw new Skip();
            }
            return zoned;
        }
        if (text.endsWith("Z") || text.matches(".*[+-]\\d\\d:\\d\\d(:\\d\\d)?$")) {
            return OffsetDateTime.parse(text);
        }
        return LocalDateTime.parse(text);
    }
}
