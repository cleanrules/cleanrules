package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.field;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.DecisionTable;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.Rule;
import io.github.cleanrules.Truth;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.Node;
import io.github.cleanrules.ffi.RuleSet;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

class LimitsTest {
    @Test
    void theDefaultsComeFromTheCore() {
        Limits defaults = Limits.defaults();
        assertTrue(defaults.maxSize() > 0);
        assertEquals(1024, defaults.maxNesting());
        assertTrue(defaults.maxDepth() > 0);
        assertTrue(defaults.maxTableSize() > 0);
        assertEquals(1 << 20, defaults.maxYamlSize());
        assertTrue(defaults.maxTextSize() > 0);
        assertTrue(defaults.toString().startsWith("Limits[maxSize="), defaults.toString());
    }

    @Test
    void limitsAreImmutableValues() {
        Limits defaults = Limits.defaults();
        Limits size = defaults.withMaxSize(100);
        assertEquals(100, size.maxSize());
        assertEquals(100, size.maxYamlSize());
        assertEquals(100, size.maxTextSize());
        assertNotEquals(defaults, size);
        assertEquals(defaults, Limits.defaults());
        assertEquals(size, defaults.withMaxSize(100));
        assertEquals(size.hashCode(), defaults.withMaxSize(100).hashCode());
        Limits yaml = size.withMaxYamlSize(5).withMaxTextSize(6).withMaxNesting(7).withMaxDepth(8).withMaxTableSize(9);
        assertEquals(List.of(100L, 7L, 8L, 9L, 5L, 6L), List.of(
                yaml.maxSize(), yaml.maxNesting(), yaml.maxDepth(), yaml.maxTableSize(), yaml.maxYamlSize(),
                yaml.maxTextSize()));
        assertThrows(IllegalArgumentException.class, () -> defaults.withMaxSize(-1));
    }

    @Test
    void eachLimitIsApplied() {
        Limits text = Limits.defaults().withMaxTextSize(10);
        FormatException size = assertThrows(
                FormatException.class, () -> RuleSet.read(Fixtures.DISCOUNTS, Format.TEXT, text));
        assertEquals("limit_exceeded", size.code());
        // The JSON limit stays.
        try (RuleSet rules = RuleSet.read(Fixtures.discounts().write(), Format.JSON, text)) {
            assertEquals(3, rules.size());
        }
        Limits depth = Limits.defaults().withMaxDepth(2);
        FormatException deep = assertThrows(
                FormatException.class, () -> Node.read("a + (b + (c + d)) > 1", Format.TEXT, depth));
        assertEquals("limit_exceeded", deep.code());
        // JSON input nested deeper than the JSON nesting limit.
        Limits nesting = Limits.defaults().withMaxNesting(2);
        try (RuleSet rules = Fixtures.echo()) {
            FormatException json =
                    assertThrows(FormatException.class, () -> rules.evaluateJson("{\"value\": [[[1]]]}", nesting));
            assertEquals("limit_exceeded", json.code());
            FormatException many = assertThrows(FormatException.class,
                    () -> rules.evaluateManyJson("[{\"value\": 1}, {\"value\": [[[1]]]}]", nesting.withMaxNesting(3)));
            assertEquals("limit_exceeded", many.code());
            for (FormatException error : List.of(json, many)) {
                String said = error.getMessage() + " " + error.hint().orElse("") + " " + error.json();
                assertFalse(said.contains("CR_") || said.contains("cr_"), said);
            }
        }
    }

    /**
     * Only {@code maxValues} bounds Java data ({@code spec/json-format.md},
     * <i>Errors and limits</i>): the other limits bound documents and JSON.
     */
    @Test
    void onlyMaxValuesBoundsJavaData() {
        Limits others = Limits.defaults()
                .withMaxSize(1)
                .withMaxNesting(1)
                .withMaxDepth(1)
                .withMaxTableSize(1)
                .withMaxYamlSize(1)
                .withMaxTextSize(1);
        Object deep = Map.of("value", List.of(List.of(List.of(1))));
        Object expected = List.of(List.of(List.of(1L)));
        try (RuleSet rules = Fixtures.echo()) {
            try (Decision one = rules.evaluate(deep, others)) {
                assertEquals(List.of(Map.of("value", expected)), one.outputs());
            }
            List<Decision> batch = rules.evaluateMany(List.of(deep, Map.of("value", 1)), others);
            assertEquals(2, batch.size());
            batch.forEach(Decision::close);
            try (Decision run = rules.run(deep, Map.of(), others)) {
                assertEquals(List.of(Map.of("value", expected)), run.outputs());
            }
        }
        List<String> ran = new ArrayList<>();
        io.github.cleanrules.RuleSet idiomatic = io.github.cleanrules.RuleSet.of(
                Rule.named("r").when(field("value").isNull().not()).action(context -> ran.add("r")));
        assertEquals(Truth.TRUE, idiomatic.evaluate(deep, others).results().get(0).truth());
        assertEquals(Truth.TRUE, idiomatic.evaluateMany(List.of(deep), others).get(0).results().get(0).truth());
        idiomatic.run(deep, others);
        assertEquals(List.of("r"), ran);
        DecisionTable table = DecisionTable.builder().header("value").row("IS NOT NULL").build();
        assertEquals(Truth.TRUE, table.evaluate(deep, others).results().get(0).truth());
        assertEquals(Truth.TRUE, table.evaluateMany(List.of(deep), others).get(0).results().get(0).truth());
        // maxValues still does: the list and its three values are four.
        assertEquals("max_values", assertThrows(FormatException.class,
                () -> idiomatic.evaluate(deep, others.withMaxValues(3))).name().orElseThrow());
        idiomatic.evaluate(deep, others.withMaxValues(4));
    }
}
