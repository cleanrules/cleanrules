package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.field;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Decision;
import io.github.cleanrules.DecisionTable;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import io.github.cleanrules.Truth;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Supplier;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.function.Executable;

/**
 * The {@code max_values} limit on Java data ({@code spec/data-model.md},
 * <i>Reading native data</i>, <i>Limits</i>): an input beyond it is refused
 * with the core's {@code limit_exceeded}, in every mode.
 */
class MaxValuesTest {
    /** A list holding the same list twice, {@code depth} times over: 2^depth references. */
    static Object diamond(int depth) {
        Object node = 1;
        for (int i = 0; i < depth; i++) {
            node = List.of(node, node);
        }
        return node;
    }

    /** Run {@code call} on a thread with 512 KiB of stack, as a host thread may have. */
    static <T> T onSmallStack(Supplier<T> call) throws InterruptedException {
        AtomicReference<T> result = new AtomicReference<>();
        AtomicReference<Throwable> failure = new AtomicReference<>();
        Thread thread = new Thread(null, () -> {
            try {
                result.set(call.get());
            } catch (Throwable thrown) {
                failure.set(thrown);
            }
        }, "small stack", 512 * 1024);
        thread.start();
        thread.join();
        if (failure.get() instanceof RuntimeException unchecked) {
            throw unchecked;
        }
        if (failure.get() instanceof Error error) {
            throw error;
        }
        return result.get();
    }

    /** The core's {@code limit_exceeded} for {@code max_values} that {@code call} throws. */
    static FormatException beyond(Executable call) {
        FormatException error = assertThrows(FormatException.class, call);
        assertEquals("limit_exceeded", error.code());
        assertEquals("max_values", error.name().orElseThrow());
        assertTrue(error.getMessage().contains("max_values"), error.getMessage());
        assertFalse(error.getMessage().contains("cr_"), error.getMessage());
        assertTrue(error.hint().isPresent());
        return error;
    }

    private static Limits at(long maxValues) {
        return Limits.defaults().withMaxValues(maxValues);
    }

    private static List<Truth> truths(Decision decision) {
        return decision.results().stream().map(r -> r.truth()).toList();
    }

    @Test
    void theDefaultIsAMillionAndItCanBeChanged() {
        assertEquals(1_000_000, Limits.defaults().maxValues());
        Limits few = at(5);
        assertEquals(5, few.maxValues());
        assertTrue(few.toString().endsWith("maxValues=5]"), few.toString());
        assertThrows(IllegalArgumentException.class, () -> few.withMaxValues(-1));
    }

    @Test
    void aDiamondOfDepth30IsRefusedInEveryMode() {
        RuleSet rules = RuleSet.of(
                Rule.named("r").when(field("d").isNull()), Rule.named("s").when(field("x").eq(1)));
        Map<String, Object> data = Map.of("d", diamond(30), "x", 1);
        long started = System.nanoTime();
        FormatException error = beyond(() -> onSmallStack(() -> rules.evaluate(data)));
        // A million values, not a thousand million.
        assertTrue(System.nanoTime() - started < 30_000_000_000L);
        assertTrue(error.rule().isEmpty() && error.path().isEmpty() && error.row().isEmpty());
        RuleSet strict = RuleSet.builder().add(Rule.named("r").when(field("d").isNull())).strict(true).build();
        beyond(() -> strict.evaluate(data));
    }

    @Test
    void theCountIsExact() {
        // diamond(3): 8 ints and 7 lists, plus the int at `x`: 16 values.
        RuleSet rules = RuleSet.of(
                Rule.named("r").when(field("d").isNull()), Rule.named("s").when(field("x").eq(1)));
        Map<String, Object> data = Map.of("d", diamond(3), "x", 1);
        assertEquals(List.of(Truth.FALSE, Truth.TRUE), truths(rules.evaluate(data, at(16))));
        beyond(() -> rules.evaluate(data, at(15)));
        // A shared object counts each time it is converted.
        Object shared = diamond(3);
        RuleSet both = RuleSet.of(
                Rule.named("a").when(field("a").isNull()), Rule.named("b").when(field("b").isNull()));
        Map<String, Object> twice = Map.of("a", shared, "b", shared);
        assertEquals(Truth.FALSE, both.evaluate(twice, at(30)).results().get(1).truth());
        beyond(() -> both.evaluate(twice, at(29)));
        // A list along the paths counts its size, and what is read at a position.
        RuleSet second = RuleSet.of(Rule.named("r").when(field("xs", 1).eq(20)));
        assertEquals(Truth.TRUE, second.evaluate(Map.of("xs", List.of(10, 20, 30)), at(4)).results().get(0).truth());
        beyond(() -> second.evaluate(Map.of("xs", List.of(10, 20, 30)), at(3)));
        // A record is a map of its components: only those read count.
        RuleSet point = RuleSet.of(Rule.named("r").when(field("p", "x").eq(1)));
        assertEquals(Truth.TRUE, point.evaluate(Map.of("p", new Point(1, 2)), at(1)).results().get(0).truth());
        beyond(() -> point.evaluate(Map.of("p", new Point(1, 2)), at(0)));
        // A read error from a getter is a value placed in the tree.
        RuleSet failing = RuleSet.of(Rule.named("r").when(field("f", "broken").gt(1)));
        assertEquals(Truth.UNKNOWN, failing.evaluate(Map.of("f", new Failing()), at(1)).results().get(0).truth());
        beyond(() -> failing.evaluate(Map.of("f", new Failing()), at(0)));
        // The default allows a million values: 2^19 - 1, not 2^20 - 1.
        RuleSet d = RuleSet.of(Rule.named("r").when(field("d").isNull()));
        assertEquals(Truth.FALSE, d.evaluate(Map.of("d", diamond(18))).results().get(0).truth());
        beyond(() -> d.evaluate(Map.of("d", diamond(19))));
    }

    /** A record, read by its components. */
    public record Point(int x, int y) {}

    /** A JavaBean whose getter throws. */
    public static final class Failing {
        public int getBroken() {
            throw new IllegalStateException("no");
        }
    }

    @Test
    void aHugeDeclaredSizeFailsFast() {
        List<Integer> huge = Collections.nCopies(Integer.MAX_VALUE, 1);
        // Along the paths, before the list is allocated.
        beyond(() -> RuleSet.of(Rule.named("r").when(field("a", 0).isNull())).evaluate(Map.of("a", huge)));
        // Where a path ends, before the list is copied.
        beyond(() -> RuleSet.of(Rule.named("r").when(field("a").isNull())).evaluate(Map.of("a", huge)));
    }

    @Test
    void aBatchReportsTheRowOfTheFirstInputBeyondTheLimit() {
        RuleSet one = RuleSet.of(Rule.named("s").when(field("x").eq(1)));
        List<Map<String, Object>> rows = List.of(Map.of("x", 1), Map.of("x", List.of(1, 1, 1)), Map.of("x", 1));
        assertEquals(1, beyond(() -> one.evaluateMany(rows, at(2))).row().orElseThrow());
        List<Truth> batch = new ArrayList<>();
        for (Decision decision : one.evaluateMany(List.of(Map.of("x", 1), Map.of("x", List.of(1))), at(2))) {
            batch.add(decision.results().get(0).truth());
        }
        assertEquals(List.of(Truth.TRUE, Truth.UNKNOWN), batch);
        // Read before evaluating: the limit comes before an evaluation error.
        RuleSet strict = RuleSet.builder().add(Rule.named("s").when(field("y").eq(1))).strict(true).build();
        List<Map<String, Object>> missing = List.of(Map.of(), Map.of("y", List.of(1, 1, 1)));
        assertEquals(1, beyond(() -> strict.evaluateMany(missing, at(2))).row().orElseThrow());
    }

    @Test
    void runsTablesTreesAndMetadataTakeTheLimitToo() {
        List<String> ran = new ArrayList<>();
        RuleSet rules = RuleSet.of(Rule.named("r").when(field("x").isNull()).action(context -> ran.add("r")));
        beyond(() -> rules.run(Map.of("x", diamond(30))));
        // diamond(2): 4 ints and 3 lists.
        rules.run(Map.of("x", diamond(2)), at(7));
        beyond(() -> rules.run(Map.of("x", diamond(2)), at(6)));
        assertEquals(List.of(), ran);
        DecisionTable table = DecisionTable.builder().header("x").row("> 0").build();
        beyond(() -> table.evaluate(Map.of("x", List.of(1)), at(1)));
        beyond(() -> table.evaluateMany(List.of(Map.of("x", List.of(1))), at(1)));
        beyond(() -> field("x").eq(1).evaluateTree(Map.of("x", diamond(30))));
        beyond(() -> field("x").eq(1).evaluateTree(Map.of("x", diamond(2)), "boolean", false, at(6)));
        assertTrue(field("x").eq(1).evaluateTree(Map.of("x", diamond(2)), "boolean", false, at(7))
                .reason().isPresent());
        beyond(() -> field("x").evaluateValueTree(Map.of("x", diamond(2)), false, at(6)));
        try (io.github.cleanrules.ffi.Node node =
                io.github.cleanrules.ffi.Node.read("x", io.github.cleanrules.Format.TEXT, null)) {
            assertTrue(node.evaluateValueTree(Map.of("x", diamond(2)), false, at(7)).contains("value"));
            beyond(() -> node.evaluateValueTree(Map.of("x", diamond(2)), false, at(6)));
            beyond(() -> node.evaluateTree(Map.of("x", diamond(2)), "boolean", false, at(6)));
        }
        // Metadata, under the default limit, naming the rule.
        FormatException metadata = beyond(
                () -> Rule.named("big").when(field("x").eq(1)).metadata(Map.of("m", diamond(20))));
        assertEquals("big", metadata.rule().orElseThrow());
        assertEquals(
                1, Rule.named("small").when(field("x").eq(1)).metadata(Map.of("m", 1)).metadata().size());
    }
}
