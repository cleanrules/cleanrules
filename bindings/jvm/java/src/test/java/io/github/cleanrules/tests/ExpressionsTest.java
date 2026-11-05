package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.and;
import static io.github.cleanrules.Conditions.constant;
import static io.github.cleanrules.Conditions.field;
import static io.github.cleanrules.Conditions.not;
import static io.github.cleanrules.Conditions.or;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Closed;
import io.github.cleanrules.Const;
import io.github.cleanrules.Expr;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Field;
import io.github.cleanrules.Kind;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.time.DayOfWeek;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.ZonedDateTime;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.function.Supplier;
import org.junit.jupiter.api.Test;

/** Conditions built with Conditions and Expr: every operator, its text, and the core's checks. */
class ExpressionsTest {
    @Test
    void everyOperatorHasItsTagAndText() {
        Field x = field("x");
        Map<String, Expr> nodes = new java.util.LinkedHashMap<>();
        nodes.put("x = 1", x.eq(1));
        nodes.put("x != 1", x.ne(1));
        nodes.put("x < 1", x.lt(1));
        nodes.put("x <= 1", x.le(1));
        nodes.put("x > 1", x.gt(1));
        nodes.put("x >= 1", x.ge(1));
        nodes.put("x > 1 AND y", x.gt(1).and(field("y")));
        nodes.put("x > 1 OR y", x.gt(1).or(field("y")));
        nodes.put("NOT y", field("y").not());
        nodes.put("country IN (\"ES\", \"PT\")", field("country").isIn("PT", "ES", "PT"));
        nodes.put("x BETWEEN 1 AND 2", x.between(1, 2));
        nodes.put("x IS NULL", x.isNull());
        nodes.put("email MATCHES \".+@.+\"", field("email").matches(".+@.+"));
        nodes.put("name STARTS WITH \"Jo\"", field("name").startsWith("Jo"));
        nodes.put("name ENDS WITH \"hn\"", field("name").endsWith("hn"));
        nodes.put("name CONTAINS \"oh\"", field("name").contains("oh"));
        nodes.put("x + 1", x.add(1));
        nodes.put("x - 1", x.sub(1));
        nodes.put("x * 1.5e0", x.mul(1.5));
        nodes.put("x / 2", x.div(2));
        nodes.put("-x", x.neg());
        nodes.put("ABS(x)", x.abs());
        List<String> tags = List.of("eq", "ne", "lt", "le", "gt", "ge", "and", "or", "not", "in", "between",
                "is_null", "matches", "starts_with", "ends_with", "contains", "add", "sub", "mul", "div", "neg", "abs");
        int i = 0;
        for (Map.Entry<String, Expr> entry : nodes.entrySet()) {
            assertEquals(entry.getKey(), entry.getValue().toString());
            assertEquals(tags.get(i++), entry.getValue().tag());
        }
        assertEquals("field", x.tag());
        assertEquals("const", constant(1).tag());
        // The closed ends of between.
        assertTrue(x.between(1, 2, Closed.LEFT).toJson().contains("\"closed\":\"left\""));
        assertTrue(x.between(field("lo"), field("hi"), Closed.NONE).toJson().contains("\"none\""));
    }

    @Test
    void staticConnectivesAndTheDecisionsStyle() {
        Expr adult = field("customer", "age").ge(18).and(field("country").isIn("ES", "PT"));
        assertEquals("customer.age >= 18 AND country IN (\"ES\", \"PT\")", adult.toString());
        Expr both = and(field("a"), field("b"), field("c"));
        assertEquals(3, both.children().size());
        // Flattened: an and inside an and is spliced in.
        assertEquals(4, and(both, field("d")).children().size());
        assertEquals("a OR b", or(field("a"), field("b")).toString());
        assertEquals("NOT a", not(field("a")).toString());
        assertThrows(ExpressionException.class, () -> and(field("a")));
    }

    @Test
    void fieldsKeepTheirPath() {
        Field item = field("items", 0, "a.b");
        assertEquals(List.of("items", 0, "a.b"), item.path());
        assertEquals("items[0].`a.b`", item.toString());
        assertEquals(List.of("items", 0, "price"), field("items").at(0, "price").path());
        assertEquals(List.of(List.of("items", 0, "a.b")), item.fieldPaths());
        assertEquals(Kind.ANY, item.kind());
        for (Object[] path : List.of(new Object[] {}, new Object[] {""}, new Object[] {-1},
                new Object[] {true}, new Object[] {1.5}, new Object[] {List.of("a")})) {
            ExpressionException error = assertThrows(ExpressionException.class, () -> field(path));
            assertEquals("expression_error", error.code());
        }
        assertEquals("a field path segment cannot be the negative index -1",
                assertThrows(ExpressionException.class, () -> field("a", -1)).getMessage());
    }

    @Test
    void constantsAreConvertedAndGivenBack() {
        ZonedDateTime madrid = ZonedDateTime.of(2026, 11, 5, 9, 30, 0, 0, ZoneId.of("Europe/Madrid"));
        Map<Object, Object> cases = new java.util.LinkedHashMap<>();
        cases.put(5, 5L);
        cases.put(5L, 5L);
        cases.put(BigInteger.TEN, 10L);
        cases.put(1.5f, 1.5);
        cases.put(new BigDecimal("0.10"), new BigDecimal("0.10"));
        cases.put("text", "text");
        cases.put(DayOfWeek.MONDAY, "MONDAY");
        cases.put(true, true);
        cases.put(LocalDate.of(2026, 11, 5), LocalDate.of(2026, 11, 5));
        cases.put(LocalDateTime.of(2026, 11, 5, 9, 30, 0, 123_000), LocalDateTime.of(2026, 11, 5, 9, 30, 0, 123_000));
        cases.put(madrid, madrid);
        for (Map.Entry<Object, Object> entry : cases.entrySet()) {
            Const made = constant(entry.getKey());
            assertEquals(entry.getValue(), made.value(), String.valueOf(entry.getKey()));
        }
        assertNull(constant(null).value());
        assertNull(constant(Optional.empty()).value());
        assertEquals(Kind.ANY, constant(true).kind());
        assertEquals(Kind.VALUE, constant(1).kind());
    }

    @Test
    void constantsThatAreErrors() {
        List<Supplier<Object>> bad = List.of(
                () -> constant(Double.NaN),
                () -> constant(Float.NaN),
                () -> constant(BigInteger.ONE.shiftLeft(70)),
                () -> constant(new BigDecimal("1" + "0".repeat(40) + ".5")),
                () -> constant(LocalDateTime.of(2026, 1, 1, 0, 0, 0, 1)),
                () -> constant(Instant.ofEpochSecond(0, 1)),
                () -> constant(List.of(1)),
                () -> constant(Map.of("a", 1)),
                () -> constant(Thread.currentThread()),
                () -> field("x").eq(null),
                () -> field("x").gt(Double.NaN),
                () -> field("x").isIn(),
                () -> field("x").isIn(1, null),
                () -> field("x").isIn(1, "a"),
                () -> field("x").isIn(Set.of(Double.NaN)),
                () -> field("x").gt(1).eq(1),
                () -> field("x").gt(1).add(1),
                () -> field("x").add("a"),
                () -> field("x").and(field("y").add(1)),
                () -> field("x").between(1, "z"),
                () -> field("x").matches("(?=a)"),
                () -> constant(1).not());
        for (Supplier<Object> make : bad) {
            ExpressionException error = assertThrows(ExpressionException.class, make::get);
            assertEquals("expression_error", error.code());
            assertTrue(error.json().startsWith("{\"code\":\"expression_error\""), error.json());
        }
        assertEquals("a constant cannot be NaN (NaN)",
                assertThrows(ExpressionException.class, () -> constant(Double.NaN)).getMessage());
        assertEquals("the integer constant does not fit in 64 bits",
                assertThrows(ExpressionException.class, () -> constant(BigInteger.ONE.shiftLeft(70))).getMessage());
        assertEquals("a constant must be a scalar",
                assertThrows(ExpressionException.class, () -> constant(List.of(1))).getMessage());
        assertTrue(assertThrows(ExpressionException.class, () -> field("x").matches("(?=a)"))
                .hint().orElseThrow().contains("regex"));
    }

    @Test
    void structuralIdentityIsEquality() {
        Expr a = field("x").gt(1).and(field("y").isIn(2, 1));
        Expr b = field("x").gt(1).and(field("y").isIn(1, 2));
        assertTrue(a.sameAs(b));
        assertEquals(a, b);
        assertEquals(a.hashCode(), b.hashCode());
        assertNotEquals(constant(1), constant(1.0));
        assertNotEquals(constant(1), constant(true));
        assertEquals(constant(0.0), constant(-0.0));
        assertEquals(constant(0.0).hashCode(), constant(-0.0).hashCode());
        assertEquals(constant(new BigDecimal("1.0")), constant(new BigDecimal("1.00")));
        assertFalse(a.sameAs(null));
        assertFalse(a.equals("x > 1"));
        Set<Expr> set = new HashSet<>(List.of(a, b, field("x")));
        assertEquals(2, set.size());
    }

    @Test
    void nodesDescribeThemselves() {
        Expr price = field("order", "total").mul(field("rate")).abs();
        assertEquals(Kind.VALUE, price.kind());
        assertEquals(Kind.CONDITION, price.gt(1).kind());
        List<String> walked = price.walk().stream().map(Expr::tag).toList();
        assertEquals(List.of("abs", "mul", "field", "field"), walked);
        assertEquals(List.of(List.of("order", "total"), List.of("rate")), price.fieldPaths());
        Expr child = price.children().getFirst();
        assertInstanceOf(Expr.class, child);
        assertEquals("mul", child.tag());
        assertInstanceOf(Field.class, child.children().getFirst());
        assertInstanceOf(Const.class, field("x").gt(1).children().get(1));
        assertEquals(
                "{\"op\":\"gt\",\"left\":{\"op\":\"field\",\"path\":[\"x\"]},\"right\":{\"op\":\"const\",\"value\":1}}",
                field("x").gt(1).toJson());
        assertEquals(List.of(), constant(1).children());
    }
}
