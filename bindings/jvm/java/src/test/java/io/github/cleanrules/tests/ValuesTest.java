package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.EvaluationException;
import io.github.cleanrules.Format;
import io.github.cleanrules.Limits;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.RuleSyntaxException;
import io.github.cleanrules.Truth;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.RuleSet;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.OffsetDateTime;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.util.AbstractList;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.SequencedMap;
import java.util.Set;
import java.util.TreeMap;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;

/** Java data read by the binding, and values given back (spec/data-model.md, Java and Scala). */
class ValuesTest {
    /** The value the echo rule set gives back for {@code value}. */
    static Object echo(Object value) {
        Map<String, Object> data = new HashMap<>();
        data.put("value", value);
        try (RuleSet rules = Fixtures.echo();
                Decision decision = rules.evaluate(data)) {
            return decision.outputs().getFirst().get("value");
        }
    }

    /** The JSON of what the core read for {@code value}. */
    static String echoJson(Object value) {
        Map<String, Object> data = new HashMap<>();
        data.put("value", value);
        try (RuleSet rules = Fixtures.echo();
                Decision decision = rules.evaluate(data)) {
            return decision.outputsJson();
        }
    }

    /** The decision's JSON for {@code value}, with the reasons of nulls. */
    static String decisionJson(Object value) {
        Map<String, Object> data = new HashMap<>();
        data.put("value", value);
        try (RuleSet rules = Fixtures.echo();
                Decision decision = rules.evaluate(data)) {
            return decision.write();
        }
    }

    @Test
    void nullsAndOptionals() {
        assertNull(echo(null));
        assertNull(echo(Optional.empty()));
        assertEquals(5L, echo(Optional.of(5)));
        assertEquals("[{\"value\":null}]", echoJson(null));
    }

    @Test
    void booleansAndIntegers() {
        assertEquals(true, echo(true));
        assertEquals(false, echo(Boolean.FALSE));
        assertEquals(7L, echo((byte) 7));
        assertEquals(-7L, echo((short) -7));
        assertEquals(7L, echo(7));
        assertEquals(Long.MIN_VALUE, echo(Long.MIN_VALUE));
        assertEquals(Long.MAX_VALUE, echo(BigInteger.valueOf(Long.MAX_VALUE)));
        assertEquals(Long.MIN_VALUE, echo(BigInteger.valueOf(Long.MIN_VALUE)));
        assertEquals("[{\"value\":7}]", echoJson(7));
        // Beyond 64 bits: a null with reason int_overflow.
        BigInteger big = BigInteger.ONE.shiftLeft(63);
        assertNull(echo(big));
        assertTrue(decisionJson(big).contains("int_overflow"), decisionJson(big));
    }

    @Test
    void floats() {
        assertEquals(1.5, echo(1.5f));
        assertEquals(0.1f, ((Double) echo(0.1f)).floatValue());
        assertEquals((double) 0.1f, echo(0.1f));
        assertEquals(1.0, echo(1.0));
        assertEquals("[{\"value\":1.0}]", echoJson(1.0));
        assertEquals(Double.doubleToRawLongBits(-0.0), Double.doubleToRawLongBits((Double) echo(-0.0)));
        assertEquals(Double.POSITIVE_INFINITY, echo(Double.POSITIVE_INFINITY));
        assertEquals(Double.NEGATIVE_INFINITY, echo(Float.NEGATIVE_INFINITY));
        assertNull(echo(Double.NaN));
        assertNull(echo(Float.NaN));
    }

    @Test
    void decimals() {
        assertEquals(new BigDecimal("0.10"), echo(new BigDecimal("0.10")));
        assertEquals("[{\"value\":{\"$decimal\":\"0.10\"}}]", echoJson(new BigDecimal("0.10")));
        // A positive exponent has scale 0 (spec/data-model.md, Decimals).
        assertEquals(new BigDecimal("1000"), echo(new BigDecimal("1E+3")));
        assertEquals("[{\"value\":{\"$decimal\":\"1000\"}}]", echoJson(new BigDecimal("1E+3")));
        assertEquals(new BigDecimal("-0.0015"), echo(new BigDecimal("-1.5E-3")));
        BigDecimal wide = new BigDecimal("1" + "0".repeat(40));
        assertNull(echo(wide));
        assertTrue(decisionJson(wide).contains("decimal_overflow"), decisionJson(wide));
    }

    @Test
    void decimalInfinitiesComeBackAsDoubles() {
        try (RuleSet rules = Fixtures.echo();
                Decision decision = rules.evaluateJson("{\"value\": {\"$decimal\": \"-inf\"}}")) {
            assertEquals(Double.NEGATIVE_INFINITY, decision.outputs().getFirst().get("value"));
        }
    }

    @Test
    void strings() {
        assertEquals("héllo 😀", echo("héllo 😀"));
        assertEquals("built", echo(new StringBuilder("built")));
        // A lone surrogate has no Unicode form: opaque, and the same object back.
        String lone = "a\uD800";
        assertSame(lone, echo(lone));
        assertEquals("[{\"value\":{\"$opaque\":\"java.lang.String\"}}]", echoJson(lone));
    }

    @Test
    void datesAndNaiveDatetimes() {
        LocalDate date = LocalDate.of(2026, 11, 5);
        assertEquals(date, echo(date));
        assertEquals("[{\"value\":{\"$date\":\"2026-11-05\"}}]", echoJson(date));
        LocalDate tooLate = LocalDate.of(10000, 1, 1);
        assertSame(tooLate, echo(tooLate));
        LocalDate yearZero = LocalDate.of(0, 1, 1);
        assertSame(yearZero, echo(yearZero));
        // Nanoseconds are truncated to the microsecond.
        LocalDateTime local = LocalDateTime.of(2026, 11, 5, 9, 30, 1, 123_456_789);
        assertEquals(LocalDateTime.of(2026, 11, 5, 9, 30, 1, 123_456_000), echo(local));
        assertEquals("[{\"value\":{\"$datetime\":\"2026-11-05T09:30:01.123456\"}}]", echoJson(local));
        @SuppressWarnings("deprecation")
        java.sql.Date sqlDate = new java.sql.Date(126, 10, 5);
        assertEquals(date, echo(sqlDate));
        java.sql.Time time = java.sql.Time.valueOf("09:30:00");
        assertSame(time, echo(time));
    }

    @Test
    void awareDatetimes() {
        Instant instant = Instant.parse("2026-11-05T09:30:00.123456789Z");
        ZonedDateTime utc = ZonedDateTime.of(2026, 11, 5, 9, 30, 0, 123_456_000, ZoneOffset.UTC);
        assertEquals(utc, echo(instant));
        assertEquals("[{\"value\":{\"$datetime\":\"2026-11-05T09:30:00.123456Z\"}}]", echoJson(instant));
        assertEquals(utc.withNano(123_000_000), echo(java.util.Date.from(instant)));
        java.sql.Timestamp stamp = java.sql.Timestamp.from(instant);
        assertEquals(utc, echo(stamp));

        OffsetDateTime offset = OffsetDateTime.of(2026, 11, 5, 15, 0, 0, 0, ZoneOffset.ofHoursMinutes(5, 30));
        ZonedDateTime back = (ZonedDateTime) echo(offset);
        assertEquals(offset.toInstant(), back.toInstant());
        assertEquals(ZoneOffset.ofHoursMinutes(5, 30), back.getZone());
        assertEquals("[{\"value\":{\"$datetime\":\"2026-11-05T15:00:00+05:30\"}}]", echoJson(offset));

        ZonedDateTime madrid = ZonedDateTime.of(2026, 7, 1, 12, 0, 0, 0, ZoneId.of("Europe/Madrid"));
        assertEquals(madrid, echo(madrid));
        assertEquals(
                "[{\"value\":{\"$datetime\":\"2026-07-01T12:00:00+02:00[Europe/Madrid]\"}}]", echoJson(madrid));
        // A ZoneOffset is a fixed offset, never a zone.
        ZonedDateTime fixed = ZonedDateTime.of(2026, 7, 1, 12, 0, 0, 0, ZoneOffset.UTC);
        assertEquals(fixed, echo(fixed));
        assertEquals("[{\"value\":{\"$datetime\":\"2026-07-01T12:00:00Z\"}}]", echoJson(fixed));
        // A zone the core does not know keeps the offset.
        ZonedDateTime custom = ZonedDateTime.of(2026, 7, 1, 12, 0, 0, 0, ZoneId.of("UTC+01:00"));
        ZonedDateTime customBack = (ZonedDateTime) echo(custom);
        assertEquals(custom.toInstant(), customBack.toInstant());
        assertEquals(ZoneOffset.ofHours(1), customBack.getZone());
        // Outside years 1 to 9999, in UTC or locally: opaque.
        Instant early = Instant.parse("0000-12-31T23:00:00Z");
        assertSame(early, echo(early));
        OffsetDateTime localTooLate = OffsetDateTime.of(9999, 12, 31, 23, 0, 0, 0, ZoneOffset.ofHours(-2));
        assertSame(localTooLate, echo(localTooLate));
        // What the core refuses is opaque too, and comes back as the object.
        Instant lastHour = Instant.parse("9999-12-31T23:00:00Z");
        assertSame(lastHour, echo(lastHour));
    }

    @Test
    void listsAndArrays() {
        assertEquals(List.of(1L, 2L), echo(List.of(1, 2)));
        assertEquals(List.of(1L, 2L), echo(new int[] {1, 2}));
        assertEquals(List.of("a", "b"), echo(new String[] {"a", "b"}));
        List<Object> withNull = new ArrayList<>();
        withNull.add(null);
        withNull.add(true);
        assertEquals(withNull, echo(withNull));
        Object list = echo(List.of(1));
        assertThrows(UnsupportedOperationException.class, () -> ((List<?>) list).clear());
        // Bytes and sets are opaque.
        byte[] bytes = {1, 2};
        assertSame(bytes, echo(bytes));
        Set<Integer> set = Set.of(1);
        assertSame(set, echo(set));
        // A list that holds itself is opaque where it repeats.
        List<Object> self = new ArrayList<>();
        self.add(1);
        self.add(self);
        List<?> back = (List<?>) echo(self);
        assertEquals(1L, back.get(0));
        assertSame(self, back.get(1));
    }

    @Test
    void mapsKeepTheirOrderOrAreSorted() {
        SequencedMap<String, Object> ordered = new LinkedHashMap<>();
        ordered.put("b", 1);
        ordered.put("a", 2);
        assertEquals("[{\"value\":{\"b\":1,\"a\":2}}]", echoJson(ordered));
        Object back = echo(ordered);
        assertInstanceOf(SequencedMap.class, back);
        assertEquals(List.of("b", "a"), List.copyOf(((Map<?, ?>) back).keySet()));
        assertThrows(UnsupportedOperationException.class, () -> ((Map<?, ?>) back).clear());

        Map<String, Object> unordered = new HashMap<>();
        unordered.put("b", 1);
        unordered.put("a", 2);
        unordered.put("～", 3);
        unordered.put("😀", 4);
        // By code point: U+FF5E before U+1F600, unlike UTF-16 order.
        assertEquals("[{\"value\":{\"a\":2,\"b\":1,\"～\":3,\"😀\":4}}]", echoJson(unordered));

        TreeMap<String, Object> sorted = new TreeMap<>(java.util.Comparator.reverseOrder());
        sorted.put("a", 1);
        sorted.put("b", 2);
        assertEquals("[{\"value\":{\"b\":2,\"a\":1}}]", echoJson(sorted));

        // Keys that are not strings are left out; a null value is a null.
        Map<Object, Object> mixed = new LinkedHashMap<>();
        mixed.put(1, "one");
        mixed.put("k", null);
        assertEquals("[{\"value\":{\"k\":null}}]", echoJson(mixed));
    }

    public record Point(int y, int x) {}

    private record Hidden(String name) {}

    public static class Bean {
        public int field = 1;
        public static int constant = 2;
        private int secret = 3;

        public String getName() {
            return "bean";
        }

        public boolean isActive() {
            return true;
        }

        public String getURL() {
            return "u";
        }

        public int getSecret(int key) {
            return secret + key;
        }

        public void getNothing() {}

        public static int getStatic() {
            return 4;
        }

        int getPackagePrivate() {
            return 5;
        }
    }

    @Test
    void recordsAndBeansAreMapsOfTheirPublicData() {
        assertEquals("[{\"value\":{\"y\":2,\"x\":1}}]", echoJson(new Point(2, 1)));
        assertEquals("[{\"value\":{\"name\":\"n\"}}]", echoJson(new Hidden("n")));
        assertEquals(
                "[{\"value\":{\"URL\":\"u\",\"active\":true,\"field\":1,\"name\":\"bean\"}}]",
                echoJson(new Bean()));
    }

    @Test
    void objectsOfThePlatformAreOpaque() {
        Thread thread = Thread.currentThread();
        assertSame(thread, echo(thread));
        assertSame(String.class, echo(String.class));
        AtomicInteger atomic = new AtomicInteger(1);
        assertSame(atomic, echo(atomic));
        Object object = new Object();
        assertSame(object, echo(object));
        Runnable lambda = () -> {};
        assertSame(lambda, echo(lambda));
        ClassLoader loader = getClass().getClassLoader();
        assertSame(loader, echo(loader));
    }

    public static class Holder {
        public Object getThread() {
            return Thread.currentThread();
        }
    }

    @Test
    void nothingReachesIntoTheProcess() {
        try (RuleSet rules = RuleSet.read(
                        "HIT POLICY collect\nIF true THEN a = value.class, b = value.thread.contextClassLoader",
                        Format.TEXT);
                Decision decision = rules.evaluate(Map.of("value", new Holder()))) {
            Map<String, Object> output = decision.outputs().getFirst();
            assertNull(output.get("a"));
            assertNull(output.get("b"));
            assertTrue(decision.write().contains("missing_field"), decision.write());
        }
    }

    @Test
    void opaqueObjectsComeBackByIdentity() {
        Object thing = new Object();
        try (RuleSet rules = RuleSet.read("HIT POLICY collect\nIF true THEN a = x, b = y", Format.TEXT);
                Decision decision = rules.evaluate(Map.of("x", thing, "y", thing))) {
            Map<String, Object> output = decision.outputs().getFirst();
            assertSame(thing, output.get("a"));
            assertSame(thing, output.get("b"));
        }
        // A recorded decision keeps only the type name.
        try (RuleSet rules = Fixtures.echo();
                Decision decision = rules.evaluate(Map.of("value", thing));
                Decision read = rules.readDecision(decision.write())) {
            assertNull(read.outputs().getFirst().get("value"));
        }
    }

    public static class Customer {
        final AtomicInteger reads = new AtomicInteger();

        public Customer getCustomer() {
            reads.incrementAndGet();
            return this;
        }

        public int getYears() {
            return 7;
        }

        public boolean isNewsletter() {
            return false;
        }

        public boolean isBlocked() {
            return false;
        }

        public String getUnread() {
            throw new IllegalStateException("never read");
        }
    }

    @Test
    void onlyThePathsReadAreReadAndEachObjectOnce() {
        Customer customer = new Customer();
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.evaluate(customer)) {
            assertEquals(1, customer.reads.get());
            assertEquals(List.of(2), decision.selected());
        }
    }

    @Test
    void aSparseListKeepsItsLength() {
        AtomicInteger gets = new AtomicInteger();
        List<Integer> items = new AbstractList<>() {
            @Override
            public Integer get(int index) {
                gets.incrementAndGet();
                return index * 10;
            }

            @Override
            public int size() {
                return 5;
            }
        };
        try (RuleSet rules = RuleSet.read("HIT POLICY collect\nIF items[3] = 30 THEN n = items[3], m = items[9]",
                        Format.TEXT);
                Decision decision = rules.evaluate(Map.of("items", items))) {
            assertEquals(1, gets.get());
            Map<String, Object> output = decision.outputs().getFirst();
            assertEquals(30L, output.get("n"));
            assertNull(output.get("m"));
        }
    }

    public static class Failing {
        public int getAge() {
            throw new IllegalStateException("no age");
        }
    }

    @Test
    void aGetterThatThrowsIsAReadErrorAndStrictModeChainsIt() {
        try (RuleSet rules = RuleSet.read("RULE adult IF person.age >= 18", Format.TEXT);
                Decision decision = rules.evaluate(Map.of("person", new Failing()))) {
            assertEquals(Truth.UNKNOWN, decision.truth(0));
            assertTrue(decision.write().contains("read_error"), decision.write());
        }
        try (RuleSet strict = RuleSet.read("STRICT\nRULE adult IF person.age >= 18", Format.TEXT)) {
            EvaluationException error = assertThrows(
                    EvaluationException.class, () -> strict.evaluate(Map.of("person", new Failing())));
            assertEquals("evaluation_error", error.code());
            assertEquals(Optional.of(List.of("person", "age")), error.path());
            assertInstanceOf(IllegalStateException.class, error.getCause());
            assertEquals("no age", error.getCause().getMessage());
            // In a batch, the cause of the row that failed.
            EvaluationException batch = assertThrows(
                    EvaluationException.class,
                    () -> strict.evaluateMany(List.of(Map.of("person", Map.of("age", 20)), Map.of("person", new Failing()))));
            assertEquals(java.util.OptionalInt.of(1), batch.row());
            assertEquals("no age", batch.getCause().getMessage());
        }
    }

    @Test
    void absentIsMissingAndNullIsNull() {
        try (RuleSet strict = RuleSet.read("STRICT\nRULE a IF x IS NULL", Format.TEXT)) {
            Map<String, Object> withNull = new HashMap<>();
            withNull.put("x", null);
            try (Decision decision = strict.evaluate(withNull)) {
                assertEquals(Truth.TRUE, decision.truth(0));
            }
            MissingFieldException missing =
                    assertThrows(MissingFieldException.class, () -> strict.evaluate(Map.of()));
            assertEquals(Optional.of(List.of("x")), missing.path());
            // An integer segment never reads a map, a string segment never a list.
            assertThrows(MissingFieldException.class, () -> strict.evaluate(List.of(1)));
        }
    }

    @Test
    void anIndexBeyondAnyJavaListIsMissing() {
        try (RuleSet rules = RuleSet.read("RULE big IF a[3000000000] > 1", Format.TEXT);
                RuleSet strict = RuleSet.read("STRICT\nRULE big IF a[3000000000] > 1", Format.TEXT)) {
            assertEquals(List.of(List.of("a", 3_000_000_000L)), rules.fieldPaths());
            try (Decision decision = rules.evaluate(Map.of("a", List.of(1, 2)))) {
                assertEquals(Truth.UNKNOWN, decision.truth(0));
            }
            MissingFieldException error =
                    assertThrows(MissingFieldException.class, () -> strict.evaluate(Map.of("a", List.of(1, 2))));
            assertEquals(Optional.of(List.of("a", 3_000_000_000L)), error.path());
        }
    }

    enum Level {
        LOW,
        HIGH {
            @Override
            public String toString() {
                return "high";
            }
        }
    }

    @Test
    void enumsAreTheirNames() {
        assertEquals("LOW", echo(Level.LOW));
        // A constant with a body is a subclass: still its name.
        assertEquals("HIGH", echo(Level.HIGH));
        assertEquals("MONDAY", echo(java.time.DayOfWeek.MONDAY));
        assertEquals("SECONDS", echo(java.util.concurrent.TimeUnit.SECONDS));
        try (RuleSet rules = RuleSet.read("RULE high IF level = 'HIGH'", Format.TEXT);
                Decision decision = rules.evaluate(Map.of("level", Level.HIGH))) {
            assertEquals(Truth.TRUE, decision.truth(0));
        }
    }

    @Test
    void theBindingsOwnObjectsAreOpaque() {
        try (RuleSet rules = Fixtures.echo()) {
            assertSame(rules, echo(rules));
            Limits limits = Limits.defaults();
            assertSame(limits, echo(limits));
            // Its enums too, unlike other enums, and its exceptions.
            assertSame(Format.JSON, echo(Format.JSON));
            RuleSyntaxException error = assertThrows(RuleSyntaxException.class, () -> RuleSet.read("IF", Format.TEXT));
            assertSame(error, echo(error));
        }
    }

    public static class Overflowing {
        public int getAge() {
            throw new StackOverflowError();
        }
    }

    @Test
    void errorsOfTheVirtualMachineAreNeverReadErrors() {
        try (RuleSet rules = RuleSet.read("RULE adult IF person.age >= 18", Format.TEXT)) {
            assertThrows(StackOverflowError.class, () -> rules.evaluate(Map.of("person", new Overflowing())));
            // The rule set still evaluates afterwards.
            rules.evaluate(Map.of()).close();
        }
    }

    @Test
    void aGivenBackValueReadsBackAsTheSameValue() {
        Map<String, Object> data = new LinkedHashMap<>();
        data.put("i", 1L);
        data.put("f", 1.0);
        data.put("d", new BigDecimal("1.50"));
        data.put("s", "s");
        data.put("date", LocalDate.of(2026, 1, 2));
        data.put("naive", LocalDateTime.of(2026, 1, 2, 3, 4, 5, 6000));
        data.put("aware", ZonedDateTime.of(2026, 1, 2, 3, 4, 5, 0, ZoneId.of("America/New_York")));
        data.put("list", List.of(1L, "x"));
        try (RuleSet rules = Fixtures.echo();
                Decision first = rules.evaluate(Map.of("value", data));
                Decision second = rules.evaluate(Map.of("value", first.outputs().getFirst().get("value")))) {
            assertEquals(first.outputsJson(), second.outputsJson());
            assertEquals(data, first.outputs().getFirst().get("value"));
        }
    }
}
