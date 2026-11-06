package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.field;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Decision;
import io.github.cleanrules.Expr;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.ResultTree;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import io.github.cleanrules.Truth;
import io.github.cleanrules.ffi.Node;
import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import org.junit.jupiter.api.Test;

/** Result trees of one node, and of a rule's condition in an action. */
class TreesTest {
    private static final Expr ADULT_IN_SPAIN = field("age").ge(18).and(field("country").isIn("ES"));

    @Test
    void aConditionGivesItsTree() {
        ResultTree tree = ADULT_IN_SPAIN.evaluateTree(Map.of("age", 20));
        assertEquals(Optional.of(Truth.UNKNOWN), tree.truth());
        assertEquals(ADULT_IN_SPAIN, tree.node());
        assertEquals(2, tree.children().size());
        ResultTree age = tree.children().get(0);
        assertEquals(Optional.of(Truth.TRUE), age.truth());
        assertEquals(field("age").ge(18), age.node());
        assertEquals(20L, age.children().get(0).value());
        assertEquals(Optional.empty(), age.children().get(0).truth());
        assertEquals(field("age"), age.children().get(0).node());
        ResultTree country = tree.children().get(1);
        assertEquals("missing_field", country.reason().orElseThrow().code());
        assertEquals(List.of(List.of("country")), country.reason().orElseThrow().paths());
        assertEquals(Optional.empty(), age.reason());
        // The JSON form is the core's, the native layer's, for the root and for each subtree.
        try (Node node = Node.read(ADULT_IN_SPAIN.toJson(), io.github.cleanrules.Format.JSON)) {
            assertEquals(node.evaluateTree(Map.of("age", 20)), tree.toJson());
        }
        assertTrue(country.toJson().startsWith("{\"op\":\"in\""), country.toJson());
        assertTrue(tree.toJson().contains(country.toJson()));
        assertEquals("ResultTree(age >= 18: true)", age.toString());
    }

    @Test
    void aValueGivesItsTree() {
        ResultTree tree = field("price").mul(field("quantity")).evaluateValueTree(Map.of("price", 2.5, "quantity", 4));
        assertEquals(10.0, tree.value());
        assertEquals(Optional.empty(), tree.truth());
        assertEquals(List.of(2.5, 4L), tree.children().stream().map(ResultTree::value).toList());
        ResultTree decimal = field("price").mul(2).evaluateValueTree(Map.of("price", new BigDecimal("1.25")));
        // Arithmetic converts decimals to floats (spec/ast.md).
        assertEquals(2.5, decimal.value());
        ResultTree missing = field("price").abs().evaluateValueTree(Map.of());
        assertEquals(null, missing.value());
        assertEquals("missing_field", missing.reason().orElseThrow().code());
        assertEquals("ResultTree(ABS(price): null)", missing.toString());
        // An opaque value comes back as the very object.
        Object thing = new Object();
        assertSame(thing, field("x").evaluateValueTree(Map.of("x", thing)).value());
        // Between has two halves the evaluation made.
        ResultTree between = field("x").between(1, 3).evaluateTree(Map.of("x", 2));
        assertEquals(List.of("x >= 1", "x <= 3"), between.children().stream()
                .map(child -> child.node().toString())
                .toList());
    }

    @Test
    void wrongNodesLogicsAndStrictMode() {
        assertThrows(ExpressionException.class, () -> field("a").add(1).evaluateTree(Map.of()));
        assertThrows(ExpressionException.class, () -> field("a").gt(1).evaluateValueTree(Map.of()));
        assertThrows(RegistryException.class, () -> ADULT_IN_SPAIN.evaluateTree(Map.of(), "nope", false));
        assertThrows(RegistryException.class, () -> ADULT_IN_SPAIN.evaluateTree(Map.of(), "", false));
        assertThrows(MissingFieldException.class, () -> ADULT_IN_SPAIN.evaluateTree(Map.of(), "boolean", true));
        assertThrows(MissingFieldException.class, () -> field("a").neg().evaluateValueTree(Map.of(), true));
        // A field is a condition too.
        assertEquals(Optional.of(Truth.TRUE), field("ok").evaluateTree(Map.of("ok", true)).truth());
    }

    @Test
    void anActionReadsTheTreeOfItsRule() {
        List<ResultTree> trees = new ArrayList<>();
        Rule vip = Rule.named("vip")
                .when(field("customer", "years").gt(5).and(field("order", "total").gt(1000)))
                .then("discount", 0.15)
                .action(context -> trees.add(context.tree()));
        Rule loyal = Rule.named("loyal").when(field("customer", "years").gt(2)).action(context -> {
            ResultTree tree = context.tree();
            assertSame(tree, context.tree());
            trees.add(tree);
        });
        RuleSet rules = RuleSet.builder()
                .add(vip, loyal)
                .hitPolicy(io.github.cleanrules.HitPolicy.RULE_ORDER)
                .build();
        Decision decision = rules.run(Map.of("customer", Map.of("years", 7), "order", Map.of("total", 2000)));
        assertEquals(List.of("vip", "loyal"), decision.fired());
        assertEquals(2, trees.size());
        assertEquals(vip.when(), trees.get(0).node());
        assertEquals(Optional.of(Truth.TRUE), trees.get(0).truth());
        assertEquals(7L, trees.get(1).children().get(0).value());
        assertEquals(
                vip.when().evaluateTree(Map.of("customer", Map.of("years", 7), "order", Map.of("total", 2000)))
                        .toJson(),
                trees.get(0).toJson());
    }
}
