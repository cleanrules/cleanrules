package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.constant;
import static io.github.cleanrules.Conditions.field;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Aggregate;
import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.Decision;
import io.github.cleanrules.Documents;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleDefinitionException;
import io.github.cleanrules.RuleNotFoundException;
import io.github.cleanrules.RuleSet;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import org.junit.jupiter.api.Test;

/** Rules and rule sets built in code, with the core's checks. */
class RulesTest {
    static Rule vip() {
        return Rule.named("vip")
                .when(field("customer", "years").gt(5).and(field("order", "total").gt(1000)))
                .then("discount", new BigDecimal("0.15"))
                .priority(10)
                .tags("loyalty", "b", "loyalty")
                .description("big spenders")
                .metadata(Map.of("owner", "sales", "ticket", 42, "since", LocalDate.of(2026, 1, 1)))
                .action("notify_sales");
    }

    static Rule loyal() {
        return Rule.named("loyal")
                .when(field("customer", "years").gt(2))
                .then("discount", new BigDecimal("0.05"));
    }

    @Test
    void rulesAreBuiltStepByStep() {
        Rule rule = vip();
        assertEquals("vip", rule.name());
        assertEquals("customer.years > 5 AND order.total > 1000", rule.when().toString());
        assertEquals(List.of("discount"), List.copyOf(rule.then().keySet()));
        assertEquals(new BigDecimal("0.15"), ((io.github.cleanrules.Const) rule.then().get("discount")).value());
        assertEquals(10, rule.priority());
        assertEquals(List.of("b", "loyalty"), rule.tags());
        assertEquals(Optional.of("big spenders"), rule.description());
        // Sorted (a Map.of has no order), as the core keeps it, and given back.
        assertEquals(List.of("owner", "since", "ticket"), List.copyOf(rule.metadata().keySet()));
        assertEquals(42L, rule.metadata().get("ticket"));
        assertEquals(LocalDate.of(2026, 1, 1), rule.metadata().get("since"));
        assertEquals(Optional.of("notify_sales"), rule.actionName());
        assertTrue(rule.action().isEmpty());
        assertEquals("Rule(vip: customer.years > 5 AND order.total > 1000)", rule.toString());

        // Each step is a new rule; a repeated key takes the new value.
        Rule twice = loyal().then("rate", field("rate").mul(2)).then("discount", 0);
        assertEquals(List.of("discount", "rate"), List.copyOf(twice.then().keySet()));
        assertEquals(0L, ((io.github.cleanrules.Const) twice.then().get("discount")).value());
        assertEquals(1, loyal().then().size());
        // A payload has no order: its keys are kept sorted by code point,
        // whatever the map (U+FF5A before U+1F600, unlike UTF-16 order).
        Map<String, Object> sequenced = new LinkedHashMap<>();
        sequenced.put("z", 1);
        sequenced.put("\uD83D\uDE00", 3);
        sequenced.put("\uFF5A", 4);
        sequenced.put("a", 2);
        List<String> sorted = List.of("a", "discount", "z", "\uFF5A", "\uD83D\uDE00");
        assertEquals(sorted, List.copyOf(loyal().then(sequenced).then().keySet()));
        Map<String, Object> plain = new HashMap<>(sequenced);
        assertEquals(sorted, List.copyOf(loyal().then(plain).then().keySet()));
        // Of two conditions, the first by key is reported.
        Map<String, Object> conditions = new LinkedHashMap<>();
        conditions.put("b", field("x").gt(1));
        conditions.put("a", field("y").gt(1));
        RuleDefinitionException error = assertThrows(RuleDefinitionException.class, () -> loyal().then(conditions));
        assertTrue(error.getMessage().contains("\"a\""), error.getMessage());
        // Code as the action; a named one keeps its name.
        Rule coded = loyal().action(context -> {});
        assertTrue(coded.action().isPresent());
        assertTrue(coded.actionName().isEmpty());
        assertEquals(Optional.of("notify_sales"), vip().action(context -> {}).actionName());
        assertTrue(vip().action(context -> {}).action("other").action().isEmpty());
    }

    @Test
    void rulesThatAreErrors() {
        RuleDefinitionException empty = assertThrows(
                RuleDefinitionException.class, () -> Rule.named("").when(field("x")));
        assertEquals("rule_definition_error", empty.code());
        RuleDefinitionException value = assertThrows(
                RuleDefinitionException.class, () -> Rule.named("r").when(field("x").add(1)));
        assertEquals(Optional.of("r"), value.rule());
        assertThrows(ExpressionException.class, () -> Rule.named("r").when(constant(null)));
        RuleDefinitionException condition = assertThrows(
                RuleDefinitionException.class, () -> loyal().then("ok", field("x").gt(1)));
        assertEquals(Optional.of("loyal"), condition.rule());
        assertThrows(ExpressionException.class, () -> loyal().then("bad", Double.NaN));
        assertThrows(RuleDefinitionException.class, () -> loyal().action(""));
        RuleDefinitionException metadata = assertThrows(
                RuleDefinitionException.class, () -> loyal().metadata(Map.of("who", List.of(new Object()))));
        assertEquals("metadata[\"who\"][0] is not a data value: a java.lang.Object", metadata.getMessage());
    }

    @Test
    void ruleSetsAreListsOfRules() {
        RuleSet rules = RuleSet.builder()
                .add(vip(), loyal())
                .hitPolicy(HitPolicy.PRIORITY)
                .build();
        assertEquals(2, rules.size());
        assertEquals(HitPolicy.PRIORITY, rules.hitPolicy());
        assertEquals(Optional.empty(), rules.aggregate());
        assertEquals("boolean", rules.logic());
        assertFalse(rules.isStrict());
        List<String> names = new ArrayList<>();
        rules.forEach(rule -> names.add(rule.name()));
        assertEquals(List.of("vip", "loyal"), names);
        assertSame(rules.rules().get(1), rules.rule("loyal"));
        assertSame(rules.rules().get(0), rules.get("vip").orElseThrow());
        assertEquals(Optional.empty(), rules.get("nope"));
        assertTrue(rules.contains("vip"));
        assertFalse(rules.contains("nope"));
        assertEquals(
                List.of(List.of("customer", "years"), List.of("order", "total")), rules.fieldPaths());
        assertEquals("RuleSet([vip, loyal], priority)", rules.toString());

        RuleNotFoundException missing = assertThrows(RuleNotFoundException.class, () -> rules.rule("nope"));
        assertEquals("rule_not_found", missing.code());
        assertEquals("no rule named \"nope\"", missing.getMessage());
        assertEquals(Optional.of("nope"), missing.name());
        assertEquals(List.of("loyal", "vip"), missing.available());
        assertEquals(Optional.of("the rules are: loyal, vip"), missing.hint());

        RuleSet collect = RuleSet.builder()
                .add(List.of(loyal()))
                .hitPolicy(HitPolicy.COLLECT)
                .aggregate(Aggregate.SUM)
                .strict(true)
                .build();
        assertEquals(Optional.of(Aggregate.SUM), collect.aggregate());
        assertTrue(collect.isStrict());
        assertEquals(HitPolicy.FIRST, RuleSet.of(loyal()).hitPolicy());
    }

    @Test
    void ruleSetsAndNodesAreSharedBetweenThreads() throws Exception {
        RuleSet rules = RuleSet.of(vip(), loyal());
        io.github.cleanrules.Expr shared = field("customer", "years").gt(2);
        List<Thread> threads = new ArrayList<>();
        List<Throwable> failures = java.util.Collections.synchronizedList(new ArrayList<>());
        for (int t = 0; t < 8; t++) {
            int years = t;
            threads.add(Thread.ofPlatform().start(() -> {
                try {
                    for (int i = 0; i < 50; i++) {
                        var decision = rules.evaluate(Map.of("customer", Map.of("years", years)));
                        String wanted = years > 2 ? "loyal" : "";
                        assertEquals(wanted, String.join("", decision.fired()));
                        assertEquals(shared, field("customer", "years").gt(2));
                        assertEquals(rules.rule("loyal").when().hashCode(), shared.hashCode());
                    }
                } catch (Throwable failure) {
                    failures.add(failure);
                }
            }));
        }
        for (Thread thread : threads) {
            thread.join();
        }
        assertEquals(List.of(), failures);
    }

    @Test
    void ruleSetsThatAreErrors() {
        RuleDefinitionException twice = assertThrows(RuleDefinitionException.class, () -> RuleSet.of(loyal(), loyal()));
        assertEquals(Optional.of("loyal"), twice.rule());
        assertThrows(RuleDefinitionException.class,
                () -> RuleSet.builder().add(loyal()).aggregate(Aggregate.SUM).build());
        RegistryException logic = assertThrows(
                RegistryException.class, () -> RuleSet.builder().add(loyal()).logic("fuzzy").build());
        assertEquals(Optional.of("fuzzy"), logic.name());
        assertThrows(NullPointerException.class, () -> RuleSet.of(loyal(), null));
        CleanRulesException any = assertThrows(CleanRulesException.class, () -> RuleSet.of(vip(), vip()));
        assertEquals("rule_definition_error", any.code());
    }

    @Test
    void rulesHaveAnIdOrTheirName() {
        assertEquals(Optional.empty(), loyal().id());
        assertEquals("loyal", loyal().effectiveId());
        Rule rule = loyal().id("PRICING-1");
        assertEquals(Optional.of("PRICING-1"), rule.id());
        assertEquals("PRICING-1", rule.effectiveId());
        assertEquals("loyal", rule.name());
        // The id is kept through the other steps, and a later id replaces it.
        Rule more = rule.priority(3).description("loyal customers").action("notify").then("tier", "silver");
        assertEquals(Optional.of("PRICING-1"), more.id());
        assertEquals("PRICING-1", more.effectiveId());
        assertEquals(Optional.of("PRICING-2"), more.id("PRICING-2").id());
        // Any non-empty string, also the rule's own name.
        assertEquals(Optional.of("loyal"), loyal().id("loyal").id());
        assertEquals(" a/b.c ~ é ", loyal().id(" a/b.c ~ é ").effectiveId());
        // A rule set keeps the ids, and still finds and names rules by name.
        RuleSet rules = RuleSet.of(vip().id("PRICING-1"), loyal());
        assertEquals(Optional.of("PRICING-1"), rules.rule("vip").id());
        assertEquals("PRICING-1", rules.rules().get(0).effectiveId());
        assertEquals(Optional.empty(), rules.rule("loyal").id());
        assertEquals("loyal", rules.rules().get(1).effectiveId());
        Decision decision = rules.evaluate(Map.of("customer", Map.of("years", 3)));
        assertEquals(List.of("loyal"), decision.fired());
    }

    @Test
    void idsThatAreErrors() {
        RuleDefinitionException empty = assertThrows(RuleDefinitionException.class, () -> loyal().id(""));
        assertEquals("rule_definition_error", empty.code());
        assertThrows(NullPointerException.class, () -> loyal().id(null));
        // Two equal ids: the error names the second rule.
        RuleDefinitionException twice = assertThrows(RuleDefinitionException.class,
                () -> RuleSet.of(vip().id("same"), loyal().id("same")));
        assertEquals(Optional.of("loyal"), twice.rule());
        // An id equal to the name of a rule without one, either way round.
        RuleDefinitionException name = assertThrows(RuleDefinitionException.class,
                () -> RuleSet.of(vip(), loyal().id("vip")));
        assertEquals(Optional.of("loyal"), name.rule());
        RuleDefinitionException earlier = assertThrows(RuleDefinitionException.class,
                () -> RuleSet.of(vip().id("loyal"), loyal()));
        assertEquals(Optional.of("loyal"), earlier.rule());
        // Swapped ids are unique, and so is an id equal to the rule's own name.
        Rule a = Rule.named("a").when(field("x")).id("b");
        Rule b = Rule.named("b").when(field("y")).id("a");
        RuleSet swapped = RuleSet.of(a, b);
        assertEquals(List.of("b", "a"), swapped.rules().stream().map(Rule::effectiveId).toList());
        assertEquals(2, RuleSet.of(vip().id("vip"), loyal()).size());
    }

    @Test
    void idsAreWrittenAndReadInEachFormat() {
        Rule rule = loyal().id("PRICING 12");
        String json = Documents.dumps(rule, Format.JSON);
        assertTrue(json.contains("\"id\""), json);
        assertTrue(json.indexOf("\"name\"") < json.indexOf("\"id\""), json);
        String text = Documents.dumps(rule, Format.TEXT);
        assertTrue(text.startsWith("RULE loyal ID \"PRICING 12\""), text);
        for (Format format : List.of(Format.JSON, Format.YAML, Format.TEXT)) {
            Rule back = (Rule) Documents.loads(Documents.dumps(rule, format), format);
            assertEquals(Optional.of("PRICING 12"), back.id(), format.toString());
            assertEquals("PRICING 12", back.effectiveId(), format.toString());
            assertEquals("loyal", back.name(), format.toString());
            assertEquals(Documents.dumps(rule, format), Documents.dumps(back, format));
        }
        Rule none = (Rule) Documents.loads(Documents.dumps(loyal(), Format.JSON), Format.JSON);
        assertEquals(Optional.empty(), none.id());
        assertFalse(Documents.dumps(loyal(), Format.JSON).contains("\"id\""));
        assertEquals(Optional.of("vip"), Documents.ruleFromText("RULE gold_customer ID vip\nIF vip").id());
        // A rule set's rules, through JSON and text, keep their ids.
        RuleSet rules = RuleSet.of(vip().metadata(Map.of()).id("PRICING-1"), loyal());
        for (Format format : List.of(Format.JSON, Format.TEXT)) {
            RuleSet back = (RuleSet) Documents.loads(Documents.dumps(rules, format), format);
            assertEquals(Optional.of("PRICING-1"), back.rule("vip").id());
            assertEquals(Optional.empty(), back.rule("loyal").id());
        }
        // A document with a repeated effective id is an error.
        String repeated = "RULE a ID x\nIF p\n\nRULE b ID x\nIF q";
        assertThrows(RuleDefinitionException.class, () -> Documents.loads(repeated, Format.TEXT));
    }
}
