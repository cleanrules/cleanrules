package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.tests.Conformance.Outcome;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestFactory;

/**
 * The conformance vectors (conformance/README.md) on the JVM: every vector of
 * conformance/vectors, through JSON ({@code json:<id>}, the native layer) and
 * through Java data ({@code native:<id>}, the idiomatic API).
 * {@link Conformance} says what each route calls and compares. Every failure
 * is reported with its id, what came back and what the vector expects; the
 * vectors a route cannot run are counted and listed with their reason, in the
 * report (printed, and written to the file that the system property
 * {@code cleanrules.conformance.report} names).
 */
class ConformanceTest {
    private static final List<String> ROUTES = List.of("json", "native");

    /** The skips each route may have; any other is a failure. */
    private static final Map<String, Set<String>> ALLOWED_SKIPS =
            Map.of("json", Set.of("grid"), "native", Set.of("decimal_infinity", "zone_unknown_to_java"));

    /** What a route did with the vectors. */
    private static final class Counts {
        int passed;
        int failed;
        final Map<String, List<String>> skipped = new TreeMap<>();
    }

    @TestFactory
    List<DynamicTest> everyVectorBothWays() {
        Map<String, List<?>> files = Conformance.files();
        Conformance runner = new Conformance();
        Map<String, Counts> counts = new LinkedHashMap<>();
        ROUTES.forEach(route -> counts.put(route, new Counts()));
        Map<String, Map<?, ?>> vectors = new LinkedHashMap<>();
        long[] started = {0};
        List<DynamicTest> tests = new ArrayList<>();
        tests.add(DynamicTest.dynamicTest("start", () -> started[0] = System.nanoTime()));
        for (Map.Entry<String, List<?>> file : files.entrySet()) {
            tests.add(DynamicTest.dynamicTest("vectors in " + file.getKey(), () -> {
                List<String> failures = new ArrayList<>();
                for (Object item : file.getValue()) {
                    Map<?, ?> vector = (Map<?, ?>) item;
                    String id = (String) vector.get("id");
                    vectors.put(id, vector);
                    for (String route : ROUTES) {
                        Outcome outcome = runner.run(vector, route);
                        Counts route0 = counts.get(route);
                        if (outcome.passed()) {
                            route0.passed++;
                        } else if (outcome.skip() != null) {
                            route0.skipped.computeIfAbsent(outcome.skip(), k -> new ArrayList<>()).add(id);
                        } else {
                            route0.failed++;
                            failures.add(route + ":" + id + ": " + outcome.failure());
                        }
                    }
                }
                assertTrue(failures.isEmpty(), failures.size() + " failed:\n" + String.join("\n", failures));
            }));
        }
        tests.add(DynamicTest.dynamicTest("every vector ran both ways, but those listed with their reason", () -> {
            double seconds = (System.nanoTime() - started[0]) / 1e9;
            int total = files.values().stream().mapToInt(List::size).sum();
            assertTrue(total >= Conformance.MINIMUM_VECTORS,
                    "only " + total + " vectors, expected at least " + Conformance.MINIMUM_VECTORS);
            assertEquals(total, vectors.size(), "every vector ran once, and ids do not repeat");
            List<String> report = new ArrayList<>();
            report.add(String.format(Locale.ROOT, "conformance on Java %s (%s %s): %d vectors in %d files, %.1f s",
                    System.getProperty("java.version"), System.getProperty("os.name"), System.getProperty("os.arch"),
                    total, files.size(), seconds));
            for (String route : ROUTES) {
                Counts c = counts.get(route);
                int skips = c.skipped.values().stream().mapToInt(List::size).sum();
                report.add(String.format(Locale.ROOT, "%s: %d of %d passed, %d failed, %d skipped",
                        route, c.passed, total, c.failed, skips));
                c.skipped.forEach((why, ids) -> report.add("  " + ids.size() + " skipped: "
                        + Conformance.SKIPS.get(why) + ": " + String.join(", ", ids)));
                assertEquals(total, c.passed + c.failed + skips, route + ": every vector is counted once");
                for (String why : c.skipped.keySet()) {
                    assertTrue(ALLOWED_SKIPS.get(route).contains(why), route + " skips for " + why);
                    assertNotNull(Conformance.SKIPS.get(why), why);
                }
            }
            List<String> forms = new ArrayList<>();
            runner.forms.forEach((form, ids) -> forms.add(form + " " + ids.size()));
            report.add("native: vectors using each Java class (inputs, constants, metadata, grid cells): "
                    + String.join(", ", forms));
            String text = String.join("\n", report);
            System.out.println(text);
            String file = System.getProperty("cleanrules.conformance.report");
            if (file != null) {
                Path path = Path.of(file);
                Files.createDirectories(path.getParent());
                Files.writeString(path, text + "\n");
            }
            // The only vectors the JSON route skips: grids have no JSON route.
            List<String> grids = vectors.entrySet().stream()
                    .filter(v -> ((Map<?, ?>) v.getValue().get("given")).containsKey("table_grid"))
                    .map(Map.Entry::getKey)
                    .toList();
            assertEquals(grids, counts.get("json").skipped.getOrDefault("grid", List.of()));
        }));
        return tests;
    }

    @Test
    void theComparisonHoldsOnItsOwnCases() {
        assertEquals(List.of(), Conformance.checkComparison());
    }

    /** The vector {@code id} with {@code from} replaced by {@code to} in its expected output only. */
    private static Map<?, ?> mutated(Map<String, Map<?, ?>> vectors, String id, String from, String to) {
        Map<?, ?> vector = vectors.get(id);
        assertNotNull(vector, "no vector " + id);
        String expect = TestJson.write(vector.get("expect"));
        assertTrue(expect.contains(from), id + ": no " + from + " in " + expect);
        Map<Object, Object> copy = new LinkedHashMap<>(vector);
        copy.put("expect", TestJson.parse(expect.replace(from, to)));
        return copy;
    }

    @Test
    void aWrongExpectedOutputFailsBothWays() {
        Map<String, Map<?, ?>> vectors = new LinkedHashMap<>();
        Conformance.files().values().forEach(list -> list.forEach(item -> {
            Map<?, ?> vector = (Map<?, ?>) item;
            vectors.put((String) vector.get("id"), vector);
        }));
        Conformance runner = new Conformance();
        for (String[] change : Conformance.WRONG) {
            Map<?, ?> vector = mutated(vectors, change[0], change[1], change[2]);
            for (String route : ROUTES) {
                Outcome outcome = runner.run(vector, route);
                // Grids have no JSON route.
                boolean grid = route.equals("json") && "grid".equals(outcome.skip());
                assertTrue(outcome.failure() != null || grid,
                        route + ":" + change[0] + " passes with " + change[2] + " for " + change[1]);
            }
        }
    }
}
