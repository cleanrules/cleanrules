package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.ActionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.ffi.Action;
import io.github.cleanrules.ffi.ActionCall;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.RuleSet;
import java.io.IOException;
import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;
import org.junit.jupiter.api.Test;

/** Actions as upcalls (spec/rules.md, Actions). */
class ActionsTest {
    private static final String RULES = """
            HIT POLICY rule_order

            RULE first
            IF x > 1
            THEN level = 1
            DO note

            RULE skipped
            IF x > 100
            DO note

            RULE second
            IF x > 2
            THEN level = 2, thing = thing
            DO alarm

            RULE silent
            IF true
            """;

    @Test
    void actionsRunForTheSelectedRulesInRuleOrder() {
        List<String> calls = new ArrayList<>();
        Object thing = new Object();
        Map<String, Object> data = Map.of("x", 5, "thing", thing);
        AtomicReference<Decision> kept = new AtomicReference<>();
        Map<String, Action> actions = new LinkedHashMap<>();
        actions.put("note", call -> calls.add("note " + call.ruleName() + " " + call.position() + " " + call.output()));
        actions.put("alarm", call -> {
            calls.add("alarm " + call.ruleName() + " " + call.truth());
            assertSame(data, call.data());
            assertSame(thing, call.output().get("thing"));
            kept.set(call.decision());
        });
        try (RuleSet rules = RuleSet.read(RULES, Format.TEXT);
                Decision decision = rules.run(data, actions)) {
            assertEquals(List.of("note first 0 {level=1}", "alarm second TRUE"), calls);
            assertEquals(List.of(0, 2, 3), decision.selected());
            try (Decision copy = kept.get()) {
                // The copy the action kept is the same decision, valid after the run.
                assertEquals(decision.write(), copy.write());
            }
        }
    }

    @Test
    void aThrowingActionStopsTheRun() {
        List<String> calls = new ArrayList<>();
        IOException failure = new IOException("disk full");
        Map<String, Action> actions = Map.of(
                "note", call -> calls.add(call.ruleName()),
                "alarm", call -> {
                    throw failure;
                });
        try (RuleSet rules = RuleSet.read(RULES, Format.TEXT)) {
            ActionException error = assertThrows(ActionException.class, () -> rules.run(Map.of("x", 5), actions));
            assertEquals("action_error", error.code());
            assertSame(failure, error.getCause());
            assertEquals(Optional.of("second"), error.rule());
            assertEquals(List.of("first"), error.executed());
            assertTrue(error.json().contains("disk full"), error.json());
            assertEquals(List.of("first"), calls);
            try (Decision decision = error.nativeDecision().orElseThrow()) {
                assertEquals(List.of(0, 2, 3), decision.selected());
                assertEquals(Map.of("level", 2L, "thing", Map.of()), withoutNull(decision.ruleOutput(2)));
            }
        }
        // Errors of the virtual machine are caught before the native frames,
        // then thrown as they are, never wrapped.
        Map<String, Action> overflowing = Map.of("note", ActionsTest::recurse, "alarm", call -> {});
        try (RuleSet rules = RuleSet.read(RULES, Format.TEXT)) {
            assertThrows(StackOverflowError.class, () -> rules.run(Map.of("x", 5), overflowing));
            // The rule set still runs afterwards.
            rules.run(Map.of("x", 0), overflowing).close();
        }
    }

    private static Map<String, Object> withoutNull(Optional<Map<String, Object>> output) {
        Map<String, Object> copy = new LinkedHashMap<>();
        output.orElseThrow().forEach((key, value) -> copy.put(key, value == null ? Map.of() : value));
        return copy;
    }

    private static void recurse(ActionCall call) {
        recurse(call);
    }

    @Test
    void unknownActionsFailBeforeTheDataIsRead() {
        AtomicInteger reads = new AtomicInteger();
        Object data = new Object() {
            @SuppressWarnings("unused")
            public int getX() {
                reads.incrementAndGet();
                return 5;
            }
        };
        try (RuleSet rules = RuleSet.read(RULES, Format.TEXT)) {
            RegistryException error =
                    assertThrows(RegistryException.class, () -> rules.run(data, Map.of("note", call -> {})));
            assertEquals(Optional.of("alarm"), error.name());
            assertEquals(Optional.of("second"), error.rule());
            assertEquals(0, reads.get());
            assertThrows(IllegalArgumentException.class, () -> rules.run(data, Map.of("", call -> {})));
        }
    }

    @Test
    void anActionCallIsValidDuringItsCallOnly() {
        AtomicReference<ActionCall> kept = new AtomicReference<>();
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.run(
                        Map.of("customer", Map.of("years", 7), "order", Map.of("total", 1500)),
                        Map.of("notify_sales", kept::set))) {
            assertEquals(Map.of("discount", new BigDecimal("0.15")), decision.output());
        }
        IllegalStateException error = assertThrows(IllegalStateException.class, () -> kept.get().ruleName());
        assertTrue(error.getMessage().contains("during its action"));
        assertThrows(IllegalStateException.class, () -> kept.get().data());
    }

    @Test
    void anActionCallUsedFromAnotherThreadAfterItsCallIsRefused() throws Exception {
        CountDownLatch returned = new CountDownLatch(1);
        AtomicReference<Throwable> late = new AtomicReference<>();
        AtomicReference<String> during = new AtomicReference<>();
        Thread[] helper = new Thread[1];
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.run(
                        Map.of("customer", Map.of("years", 7), "order", Map.of("total", 1500)),
                        Map.of("notify_sales", call -> {
                            // During the call, another thread may use it.
                            Thread reader = new Thread(() -> during.set(call.ruleName()));
                            reader.start();
                            reader.join();
                            // After it, a thread that kept it is refused.
                            helper[0] = new Thread(() -> {
                                try {
                                    returned.await();
                                    call.output();
                                } catch (Throwable thrown) {
                                    late.set(thrown);
                                }
                            });
                            helper[0].start();
                        }))) {
            assertEquals(List.of(1), decision.selected());
        }
        returned.countDown();
        helper[0].join(60_000);
        assertEquals("vip", during.get());
        assertInstanceOf(IllegalStateException.class, late.get());
        assertTrue(late.get().getMessage().contains("during its action"));
    }

    @Test
    void theActionsOfARunShareOneCopyOfItsDecision() {
        List<Decision> seen = new ArrayList<>();
        Map<String, Action> actions = Map.of(
                "note", call -> seen.add(call.decision()),
                "alarm", call -> seen.add(call.decision()));
        try (RuleSet rules = RuleSet.read(RULES, Format.TEXT);
                Decision decision = rules.run(Map.of("x", 5), actions)) {
            assertEquals(2, seen.size());
            assertSame(seen.get(0), seen.get(1));
            assertTrue(seen.get(0) != decision);
            assertEquals(decision.write(), seen.get(0).write());
            seen.get(0).close();
        }
    }

    @Test
    void anActionMayRunARuleSetAgain() {
        List<String> calls = new ArrayList<>();
        try (RuleSet rules = Fixtures.discounts()) {
            Map<String, Object> order = Map.of("customer", Map.of("years", 7), "order", Map.of("total", 1500));
            Map<String, Action> inner = Map.of("notify_sales", call -> calls.add("inner " + call.ruleName()));
            Map<String, Action> outer = Map.of("notify_sales", call -> {
                calls.add("outer before");
                try (Decision nested = rules.run(order, inner)) {
                    calls.add("nested " + nested.selected());
                }
                calls.add("outer after " + call.ruleName());
            });
            try (Decision decision = rules.run(order, outer)) {
                assertEquals(List.of(1), decision.selected());
            }
        }
        assertEquals(List.of("outer before", "inner vip", "nested [1]", "outer after vip"), calls);
    }

    @Test
    void evaluateNeverRunsActions() {
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.evaluate(Map.of("customer", Map.of("years", 7), "order", Map.of("total", 1500)))) {
            assertEquals(List.of(1), decision.selected());
        }
    }
}
