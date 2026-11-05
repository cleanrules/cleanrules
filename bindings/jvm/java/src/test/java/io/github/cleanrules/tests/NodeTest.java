package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.EvaluationException;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.ffi.Node;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import org.junit.jupiter.api.Test;

/** Single nodes (spec/ast.md, Evaluating one node). */
class NodeTest {
    @Test
    void aNodeIsReadWrittenAndSaysWhatItReads() {
        try (Node node = Node.read("years > 5 AND tier = 'gold'", Format.TEXT)) {
            assertEquals("years > 5 AND tier = \"gold\"", node.write(Format.TEXT));
            String json = node.write();
            assertTrue(json.startsWith("{\"op\":\"and\""), json);
            try (Node again = Node.read(json, Format.JSON);
                    Node yaml = Node.read(node.write(Format.YAML), Format.YAML);
                    Node bytes = Node.read(json.getBytes(StandardCharsets.UTF_8), Format.JSON, null)) {
                assertEquals(json, again.write());
                assertEquals(json, yaml.write());
                assertEquals(json, bytes.write());
            }
            assertEquals(List.of(List.of("years"), List.of("tier")), node.fieldPaths());
            assertThrows(UnsupportedException.class, () -> node.write(Format.CSV));
        }
        try (Node twice = Node.read("a.b[1] > 5 OR (x = 1 AND a.b[1] < 2)", Format.TEXT)) {
            assertEquals(List.of(List.of("a", "b", 1), List.of("x")), twice.fieldPaths());
        }
        assertThrows(UnsupportedException.class, () -> Node.read("a", Format.CSV));
    }

    @Test
    void aConditionGivesItsResultTree() {
        try (Node node = Node.read("years > 5 AND tier = 'gold'", Format.TEXT)) {
            String tree = node.evaluateTree(Map.of("years", 7, "tier", "gold"));
            assertTrue(tree.startsWith("{\"op\":\"and\",\"truth\":true"), tree);
            String json = node.evaluateTreeJson("{\"years\": 7, \"tier\": \"gold\"}", "boolean", false, null);
            assertEquals(tree, json);
            assertEquals(tree, node.evaluateTreeJson("{\"years\": 7, \"tier\": \"gold\"}"));
            String unknown = node.evaluateTree(Map.of());
            assertTrue(unknown.startsWith("{\"op\":\"and\",\"truth\":null"), unknown);
            MissingFieldException strict =
                    assertThrows(MissingFieldException.class, () -> node.evaluateTree(Map.of(), "boolean", true));
            assertEquals(Optional.empty(), strict.rule());
            assertEquals(Optional.of(List.of("years")), strict.path());
            RegistryException logic =
                    assertThrows(RegistryException.class, () -> node.evaluateTree(Map.of(), "nope", false));
            assertEquals(Optional.of("nope"), logic.name());
            assertThrows(RegistryException.class, () -> node.evaluateTree(Map.of(), "", false));
            ExpressionException value = assertThrows(ExpressionException.class, () -> node.evaluateValueTree(Map.of()));
            assertEquals("expression_error", value.code());
        }
    }

    public static class Order {
        public int getTotal() {
            return 1000;
        }

        public int getBroken() {
            throw new IllegalStateException("broken");
        }
    }

    @Test
    void aValueGivesItsResultTree() {
        try (Node node = Node.read("total * 0.9", Format.TEXT)) {
            String tree = node.evaluateValueTree(new Order());
            assertTrue(tree.contains("\"value\":900.0"), tree);
            assertEquals(tree, node.evaluateValueTreeJson("{\"total\": 1000}", false, null));
            assertEquals(tree, node.evaluateValueTreeJson("{\"total\": 1000}"));
            assertThrows(ExpressionException.class, () -> node.evaluateTree(new Order()));
            assertThrows(MissingFieldException.class, () -> node.evaluateValueTree(Map.of(), true));
        }
        try (Node broken = Node.read("broken + 1", Format.TEXT)) {
            EvaluationException error =
                    assertThrows(EvaluationException.class, () -> broken.evaluateValueTree(new Order(), true));
            assertInstanceOf(IllegalStateException.class, error.getCause());
            assertTrue(broken.evaluateValueTree(new Order()).contains("read_error"));
        }
    }

    @Test
    void jsonInputsAreChecked() {
        try (Node node = Node.read("a > 1", Format.TEXT)) {
            FormatException error = assertThrows(
                    FormatException.class, () -> node.evaluateTreeJson("{\"a\": ", "boolean", false, null));
            assertTrue(error.line().isPresent());
            FormatException deep = assertThrows(
                    FormatException.class,
                    () -> node.evaluateValueTreeJson("[[[[1]]]]", false, Limits.defaults().withMaxNesting(2)));
            assertEquals("limit_exceeded", deep.code());
        }
    }
}
