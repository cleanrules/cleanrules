package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.constant;
import static io.github.cleanrules.Conditions.field;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Action;
import io.github.cleanrules.ActionContext;
import io.github.cleanrules.ActionException;
import io.github.cleanrules.Decision;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import org.junit.jupiter.api.Test;

/** Actions given as code or by name, bound and run (spec/rules.md, *Actions*). */
class RunTest {
    @Test
    void actionsRunForTheSelectedRulesInOrder() {
        List<ActionContext> seen = new ArrayList<>();
        Action record = seen::add;
        Rule a = Rule.named("a").when(field("x").gt(1)).then("n", field("x")).action(record);
        Rule b = Rule.named("b").when(field("x").gt(5)).action(record);
        Rule c = Rule.named("c").when(constant(true)).then("n", 3).action("log");
        RuleSet rules = RuleSet.builder().add(a, b, c).hitPolicy(HitPolicy.RULE_ORDER).build();
        // Named actions must be bound first.
        RegistryException unbound = assertThrows(RegistryException.class, () -> rules.run(Map.of("x", 2)));
        assertEquals(Optional.of("c"), unbound.rule());
        RegistryException missing = assertThrows(RegistryException.class, () -> rules.bind(Map.of("other", record)));
        assertEquals(Optional.of("log"), missing.name());
        assertEquals(List.of("other"), missing.available());

        RuleSet bound = rules.bind(Map.of("log", record));
        assertTrue(bound.rule("c").action().isPresent());
        assertTrue(rules.rule("c").action().isEmpty());
        Map<String, Object> data = Map.of("x", 2);
        Decision decision = bound.run(data);
        assertEquals(List.of("a", "c"), seen.stream().map(context -> context.rule().name()).toList());
        ActionContext first = seen.getFirst();
        assertSame(data, first.data());
        assertSame(decision, first.decision());
        assertSame(decision.selected().getFirst(), first.result());
        assertEquals(Map.of("n", 2L), first.output());
        assertSame(bound.rule("a"), first.rule());
        // evaluate never runs actions.
        seen.clear();
        bound.evaluate(data);
        assertTrue(seen.isEmpty());
        // A rule without payload gives an empty output.
        bound.run(Map.of("x", 9));
        assertEquals(Map.of(), seen.get(1).output());
    }

    @Test
    void anActionThatThrowsStopsTheRun() {
        List<String> ran = new ArrayList<>();
        Rule first = Rule.named("first").when(constant(true)).action(context -> ran.add("first"));
        Rule failing = Rule.named("failing").when(constant(true)).action(context -> {
            throw new IOException("the service is down");
        });
        Rule last = Rule.named("last").when(constant(true)).action(context -> ran.add("last"));
        RuleSet rules = RuleSet.builder().add(first, failing, last).hitPolicy(HitPolicy.COLLECT).build();
        ActionException error = assertThrows(ActionException.class, () -> rules.run(Map.of()));
        assertEquals("action_error", error.code());
        assertEquals(Optional.of("failing"), error.rule());
        assertEquals(List.of("first"), error.executed());
        assertEquals(List.of("first"), ran);
        assertTrue(error.getCause() instanceof IOException);
        assertEquals(
                "the action of rule \"failing\" threw java.io.IOException: the service is down", error.getMessage());
        Decision decision = error.decision().orElseThrow();
        assertEquals(List.of("first", "failing", "last"), decision.fired());
        assertTrue(error.nativeDecision().isEmpty());
        assertTrue(error.json().contains("\"executed\":[\"first\"]"), error.json());
        // The core makes the error; its message names a named action.
        RuleSet named = RuleSet.of(Rule.named("n").when(constant(true)).action("log"))
                .bind(Map.of("log", context -> {
                    throw new IllegalStateException("full");
                }));
        ActionException fromName = assertThrows(ActionException.class, () -> named.run(Map.of()));
        assertEquals("the action \"log\" of rule \"n\" threw java.lang.IllegalStateException: full",
                fromName.getMessage());
        assertEquals(Optional.of("later actions did not run; the error carries the decision"), fromName.hint());
        assertTrue(fromName.json().contains("\"cause\":\"java.lang.IllegalStateException: full\""), fromName.json());
        // Within limits: an input of more values than maxValues is a format error before any action.
        List<String> calls = new ArrayList<>();
        RuleSet limited = RuleSet.of(Rule.named("l").when(field("a").isNull().not()).action(context -> calls.add("l")));
        assertThrows(io.github.cleanrules.FormatException.class, () -> limited.run(
                Map.of("a", List.of(List.of(List.of(1)))), io.github.cleanrules.Limits.defaults().withMaxValues(3)));
        assertTrue(calls.isEmpty());
        limited.run(Map.of("a", 1), io.github.cleanrules.Limits.defaults());
        assertEquals(List.of("l"), calls);
        // An error of the virtual machine goes on as it is.
        RuleSet deep = RuleSet.of(Rule.named("deep").when(constant(true)).action(context -> {
            throw new StackOverflowError("deep");
        }));
        assertThrows(StackOverflowError.class, () -> deep.run(Map.of()));
    }
}
