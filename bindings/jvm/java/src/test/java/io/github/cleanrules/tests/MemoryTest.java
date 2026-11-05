package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Format;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.Document;
import io.github.cleanrules.ffi.Node;
import io.github.cleanrules.ffi.RuleSet;
import io.github.cleanrules.internal.Handle;
import java.lang.foreign.MemorySegment;
import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;

/** Ownership, closing and threads. */
class MemoryTest {
    private static final Map<String, Object> ORDER =
            Map.of("customer", Map.of("years", 7), "order", Map.of("total", 1500));

    @Test
    void closingTwiceIsHarmlessAndUseAfterCloseIsClear() {
        RuleSet rules = Fixtures.discounts();
        Decision decision = rules.evaluate(ORDER);
        Document document = Document.read("a > 1", Format.TEXT);
        Node node = Node.read("a > 1", Format.TEXT);
        rules.close();
        rules.close();
        IllegalStateException error = assertThrows(IllegalStateException.class, rules::size);
        assertEquals("this RuleSet is closed", error.getMessage());
        assertThrows(IllegalStateException.class, () -> rules.evaluate(ORDER));
        // A decision keeps what it needs of its rule set.
        assertEquals(Map.of("discount", new BigDecimal("0.15")), decision.output());
        assertTrue(decision.explain().contains("vip"));
        decision.close();
        decision.close();
        assertThrows(IllegalStateException.class, decision::output);
        document.close();
        document.close();
        assertThrows(IllegalStateException.class, document::kind);
        assertThrows(IllegalStateException.class, () -> RuleSet.fromDocument(document));
        node.close();
        assertThrows(IllegalStateException.class, () -> node.evaluateTree(Map.of()));
    }

    @Test
    void severalThreadsEvaluateOneRuleSet() throws Exception {
        int threads = 8;
        int rounds = 200;
        ExecutorService pool = Executors.newFixedThreadPool(threads);
        try (RuleSet rules = Fixtures.discounts()) {
            CountDownLatch start = new CountDownLatch(1);
            List<Future<Integer>> results = new ArrayList<>();
            for (int t = 0; t < threads; t++) {
                int years = t;
                results.add(pool.submit(() -> {
                    start.await();
                    int vip = 0;
                    for (int i = 0; i < rounds; i++) {
                        Map<String, Object> order =
                                Map.of("customer", Map.of("years", years), "order", Map.of("total", 1500));
                        try (Decision decision = rules.evaluate(order)) {
                            if (decision.selected().equals(List.of(1))) {
                                vip++;
                            }
                        }
                    }
                    return vip;
                }));
            }
            start.countDown();
            for (int t = 0; t < threads; t++) {
                assertEquals(t > 5 ? rounds : 0, results.get(t).get(1, TimeUnit.MINUTES));
            }
        } finally {
            pool.shutdownNow();
        }
    }

    @Test
    void closingWhileAnotherThreadEvaluatesWaitsForIt() throws Exception {
        RuleSet rules = Fixtures.discounts();
        CountDownLatch reading = new CountDownLatch(1);
        CountDownLatch closed = new CountDownLatch(1);
        Object slow = new Object() {
            @SuppressWarnings("unused")
            public Object getCustomer() throws InterruptedException {
                reading.countDown();
                closed.await();
                return Map.of("years", 7);
            }

            @SuppressWarnings("unused")
            public Object getOrder() {
                return Map.of("total", 1500);
            }
        };
        ExecutorService pool = Executors.newSingleThreadExecutor();
        try {
            Future<List<Integer>> evaluation = pool.submit(() -> {
                try (Decision decision = rules.evaluate(slow)) {
                    return decision.selected();
                }
            });
            reading.await();
            // The evaluation holds the rule set: closing it waits for the end.
            rules.close();
            closed.countDown();
            assertEquals(List.of(1), evaluation.get(1, TimeUnit.MINUTES));
            assertThrows(IllegalStateException.class, rules::size);
        } finally {
            pool.shutdownNow();
        }
    }

    @Test
    void theCleanerFreesWhatIsNotClosed() throws Exception {
        AtomicInteger freed = new AtomicInteger();
        for (int i = 0; i < 10; i++) {
            Handle.own(new Object(), MemorySegment.ofAddress(8), pointer -> freed.incrementAndGet(), "Test");
        }
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(20);
        while (freed.get() < 10 && System.nanoTime() < deadline) {
            System.gc();
            Thread.sleep(10);
        }
        assertEquals(10, freed.get());
    }

    @Test
    void aHandleIsFreedOnceByTheLastUser() {
        AtomicInteger freed = new AtomicInteger();
        Object owner = new Object();
        Handle handle = Handle.own(owner, MemorySegment.ofAddress(8), pointer -> freed.incrementAndGet(), "Test");
        handle.acquire();
        handle.acquire();
        handle.closeFromOwner();
        assertTrue(handle.isClosed());
        assertEquals(0, freed.get());
        handle.release();
        assertEquals(0, freed.get());
        handle.release();
        assertEquals(1, freed.get());
        handle.closeFromOwner();
        handle.close();
        assertEquals(1, freed.get());
        assertThrows(IllegalStateException.class, handle::acquire);
        java.lang.ref.Reference.reachabilityFence(owner);
    }
}
