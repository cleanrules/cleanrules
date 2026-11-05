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
import io.github.cleanrules.ExpressionException;
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

        // Each step is a new rule; a repeated key keeps its place.
        Rule twice = loyal().then("rate", field("rate").mul(2)).then("discount", 0);
        assertEquals(List.of("discount", "rate"), List.copyOf(twice.then().keySet()));
        assertEquals(0L, ((io.github.cleanrules.Const) twice.then().get("discount")).value());
        assertEquals(1, loyal().then().size());
        // A payload from a map: in its order when it has one, else sorted.
        Map<String, Object> sequenced = new LinkedHashMap<>();
        sequenced.put("z", 1);
        sequenced.put("a", 2);
        assertEquals(List.of("z", "a"), List.copyOf(loyal().then(sequenced).then().keySet()).subList(1, 3));
        Map<String, Object> plain = new HashMap<>(sequenced);
        assertEquals(List.of("a", "z"), List.copyOf(loyal().then(plain).then().keySet()).subList(1, 3));
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
}
