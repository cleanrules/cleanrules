package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Aggregate;
import io.github.cleanrules.DocumentKind;
import io.github.cleanrules.EvaluationException;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.HitPolicyException;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.Truth;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.Document;
import io.github.cleanrules.ffi.RuleSet;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalInt;
import org.junit.jupiter.api.Test;

class RuleSetTest {
    private static final String ORDER =
            "{\"customer\": {\"years\": 7, \"newsletter\": false}, \"order\": {\"total\": 1500}}";

    @Test
    void aRuleSetDescribesItself() {
        try (RuleSet rules = Fixtures.discounts()) {
            assertEquals(3, rules.size());
            assertEquals(List.of("blocked", "vip", "loyal"), rules.ruleNames());
            assertEquals(HitPolicy.PRIORITY, rules.hitPolicy());
            assertEquals("priority", rules.hitPolicy().jsonName());
            assertEquals(Optional.empty(), rules.aggregate());
            assertEquals("boolean", rules.logic());
            assertFalse(rules.isStrict());
            assertEquals(
                    List.of(
                            List.of("customer", "blocked"),
                            List.of("customer", "years"),
                            List.of("order", "total"),
                            List.of("customer", "newsletter")),
                    rules.fieldPaths());
            assertThrows(IndexOutOfBoundsException.class, () -> rules.ruleName(3));
            assertThrows(IndexOutOfBoundsException.class, () -> rules.ruleName(-1));
        }
        try (RuleSet sum = RuleSet.read("HIT POLICY collect\nAGGREGATE sum\nSTRICT\nIF true THEN x = 1", Format.TEXT)) {
            assertEquals(HitPolicy.COLLECT, sum.hitPolicy());
            assertEquals(Optional.of(Aggregate.SUM), sum.aggregate());
            assertEquals("sum", Aggregate.SUM.jsonName());
            assertTrue(sum.isStrict());
        }
    }

    @Test
    void aRuleSetIsReadAndWrittenInEveryFormat() {
        try (RuleSet rules = Fixtures.discounts()) {
            String json = rules.write();
            try (RuleSet fromJson = RuleSet.read(json, Format.JSON);
                    RuleSet fromYaml = RuleSet.read(rules.write(Format.YAML), Format.YAML);
                    RuleSet fromBytes = RuleSet.read(json.getBytes(StandardCharsets.UTF_8), Format.JSON);
                    Document document = Document.read(json, Format.JSON);
                    RuleSet fromDocument = RuleSet.fromDocument(document)) {
                assertEquals(json, fromJson.write());
                assertEquals(json, fromYaml.write(Format.JSON));
                assertEquals(json, fromBytes.write());
                assertEquals(json, fromDocument.write());
                assertEquals(rules.write(Format.TEXT), fromJson.write(Format.TEXT));
            }
        }
        // One rule is a rule set of that rule.
        try (RuleSet one = RuleSet.read("IF a > 1 THEN x = 1", Format.TEXT)) {
            assertEquals(List.of("rule_1"), one.ruleNames());
        }
    }

    @Test
    void aDecisionSaysWhatFiredAndWhatItGives() {
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.evaluateJson(ORDER)) {
            assertEquals(3, decision.size());
            assertEquals("vip", decision.ruleName(1));
            assertFalse(decision.firedAt(0));
            assertTrue(decision.firedAt(1));
            assertTrue(decision.firedAt(2));
            assertEquals(Truth.UNKNOWN, decision.truth(0));
            assertEquals(Truth.TRUE, decision.truth(1));
            assertEquals(List.of(1), decision.selected());
            assertEquals(Map.of("discount", new BigDecimal("0.15")), decision.output());
            assertEquals(List.of(Map.of("discount", new BigDecimal("0.15"))), decision.outputs());
            assertEquals(Optional.of(Map.of("discount", new BigDecimal("0.15"))), decision.ruleOutput(1));
            // The payload of a rule that was not selected is not evaluated.
            assertEquals(Optional.empty(), decision.ruleOutput(2));
            assertEquals("[{\"discount\":{\"$decimal\":\"0.15\"}}]", decision.outputsJson());
            assertThrows(IndexOutOfBoundsException.class, () -> decision.firedAt(3));
            String text = decision.explain();
            assertTrue(text.contains("vip"), text);
            assertTrue(decision.explain(Format.JSON).startsWith("{"), decision.explain(Format.JSON));
            assertTrue(decision.explain(Format.YAML).contains("vip"));
        }
    }

    @Test
    void noSelectedRuleGivesANullOutput() {
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.evaluateJson("{}")) {
            assertEquals(List.of(), decision.selected());
            assertEquals(null, decision.output());
            assertEquals("null", decision.outputJson());
            assertEquals(List.of(), decision.outputs());
        }
    }

    @Test
    void severalOutputsAreAHitPolicyErrorForOutput() {
        try (RuleSet rules = RuleSet.read("HIT POLICY rule_order\nIF true THEN x = 1\nIF true THEN x = 2", Format.TEXT);
                Decision decision = rules.evaluateJson("{}")) {
            HitPolicyException error = assertThrows(HitPolicyException.class, decision::output);
            assertEquals("hit_policy_error", error.code());
            assertEquals(List.of(), error.rules());
            assertThrows(HitPolicyException.class, decision::outputJson);
            assertEquals(List.of(Map.of("x", 1L), Map.of("x", 2L)), decision.outputs());
        }
        try (RuleSet sum = RuleSet.read("HIT POLICY collect\nAGGREGATE sum\nIF true THEN x = 1\nIF true THEN x = 2",
                        Format.TEXT);
                Decision decision = sum.evaluateJson("{}")) {
            assertEquals(Map.of("x", 3L), decision.output());
        }
    }

    @Test
    void aDecisionIsWrittenAndReadBack() {
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.evaluateJson(ORDER)) {
            String json = decision.write();
            try (Decision again = rules.readDecision(json);
                    Decision yaml = rules.readDecision(decision.write(Format.YAML), Format.YAML, null);
                    Decision bytes = rules.readDecision(json.getBytes(StandardCharsets.UTF_8), Format.JSON, null)) {
                assertEquals(json, again.write());
                assertEquals(json, yaml.write());
                assertEquals(json, bytes.write());
                assertEquals(decision.selected(), again.selected());
                assertEquals(decision.explain(), again.explain());
            }
            assertThrows(UnsupportedException.class, () -> decision.write(Format.TEXT));
            assertThrows(UnsupportedException.class, () -> rules.readDecision(json, Format.TEXT, null));
            try (RuleSet other = RuleSet.read("IF a > 1 THEN x = 1", Format.TEXT)) {
                FormatException error = assertThrows(FormatException.class, () -> other.readDecision(json));
                assertEquals("decision_mismatch", error.code());
                assertTrue(error.pointer().isPresent());
            }
        }
    }

    @Test
    void theSummarySaysWhatTheRulesRead() {
        try (RuleSet rules = Fixtures.discounts()) {
            String text = rules.summary();
            assertTrue(text.contains("customer.years"), text);
            assertTrue(rules.summary(Format.JSON).startsWith("{"), rules.summary(Format.JSON));
            assertTrue(rules.summary(Format.YAML).contains("customer"));
        }
    }

    @Test
    void aBatchGivesOneDecisionPerInput() {
        try (RuleSet rules = Fixtures.discounts()) {
            List<Decision> decisions = rules.evaluateManyJson(
                    "[" + ORDER + ", {\"customer\": {\"blocked\": true}}, {}]", null);
            assertEquals(3, decisions.size());
            assertEquals(List.of(1), decisions.get(0).selected());
            assertEquals(List.of(0), decisions.get(1).selected());
            assertEquals(List.of(), decisions.get(2).selected());
            decisions.forEach(Decision::close);

            List<Decision> native_ = rules.evaluateMany(List.of(
                    Map.of("customer", Map.of("years", 7), "order", Map.of("total", 1500)), Map.of()));
            assertEquals(2, native_.size());
            assertEquals(List.of(1), native_.get(0).selected());
            native_.forEach(Decision::close);

            EvaluationException notAList =
                    assertThrows(EvaluationException.class, () -> rules.evaluateManyJson("{}", null));
            assertEquals("evaluation_error", notAList.code());
        }
        try (RuleSet strict = RuleSet.read("STRICT\nRULE a IF x > 1", Format.TEXT)) {
            MissingFieldException error = assertThrows(
                    MissingFieldException.class, () -> strict.evaluateMany(List.of(Map.of("x", 2), Map.of())));
            assertEquals(OptionalInt.of(1), error.row());
            assertEquals(Optional.of(List.of("x")), error.path());
            assertEquals(Optional.of("a"), error.rule());
            MissingFieldException json =
                    assertThrows(MissingFieldException.class, () -> strict.evaluateManyJson("[{}, {}]", null));
            assertEquals(OptionalInt.of(0), json.row());
        }
    }

    @Test
    void theEnumsHaveTheirNamesInDocuments() {
        assertEquals(HitPolicy.RULE_ORDER, HitPolicy.fromJsonName("rule_order"));
        assertEquals("count", Aggregate.COUNT.jsonName());
        assertEquals(Aggregate.MAX, Aggregate.fromJsonName("max"));
        assertEquals("unknown", Truth.UNKNOWN.jsonName());
        assertEquals(Truth.FALSE, Truth.fromJsonName("false"));
        assertEquals("decision_table", DocumentKind.DECISION_TABLE.jsonName());
        assertEquals(DocumentKind.RULESET, DocumentKind.fromJsonName("ruleset"));
        IllegalArgumentException unknown =
                assertThrows(IllegalArgumentException.class, () -> HitPolicy.fromJsonName("FIRST"));
        assertEquals("no HitPolicy is named FIRST", unknown.getMessage());
    }

    @Test
    void aBatchIsAnyIterable() {
        java.util.ArrayDeque<Object> inputs = new java.util.ArrayDeque<>();
        inputs.add(Map.of("customer", Map.of("blocked", true)));
        inputs.add(Map.of());
        try (RuleSet rules = Fixtures.discounts()) {
            List<Decision> decisions = rules.evaluateMany(inputs);
            assertEquals(List.of(0), decisions.get(0).selected());
            assertEquals(List.of(), decisions.get(1).selected());
            decisions.forEach(Decision::close);
            List<Decision> json = rules.evaluateManyJson("[{}]".getBytes(StandardCharsets.UTF_8), null);
            assertEquals(1, json.size());
            json.forEach(Decision::close);
        }
    }

    @Test
    void jsonInputMayBeBytes() {
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.evaluateJson(ORDER.getBytes(StandardCharsets.UTF_8), null);
                Decision plain = rules.evaluateJson(ORDER.getBytes(StandardCharsets.UTF_8));
                Decision read = rules.readDecision(decision.write(Format.YAML), Format.YAML)) {
            assertEquals(List.of(1), decision.selected());
            assertEquals(decision.write(), plain.write());
            assertEquals(decision.write(), read.write());
        }
    }
}
