package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.ffi.Node;
import io.github.cleanrules.ffi.RuleSet;
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
        // Java data nested deeper than the JSON nesting limit.
        Limits nesting = Limits.defaults().withMaxNesting(2);
        try (RuleSet rules = Fixtures.echo()) {
            Object deepData = Map.of("value", List.of(List.of(List.of(1))));
            FormatException input =
                    assertThrows(FormatException.class, () -> rules.evaluate(deepData, nesting));
            assertEquals("limit_exceeded", input.code());
            FormatException json =
                    assertThrows(FormatException.class, () -> rules.evaluateJson("{\"value\": [[[1]]]}", nesting));
            assertEquals("limit_exceeded", json.code());
            rules.evaluate(deepData).close();
        }
    }
}
