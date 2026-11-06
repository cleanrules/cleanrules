package io.github.cleanrules.scaladsl.tests

import io.github.cleanrules.{Closed, ExpressionException, Kind, Truth}
import io.github.cleanrules.scaladsl.*
import java.time.{DayOfWeek, LocalDate, ZoneId, ZonedDateTime}

enum Level:
  case Low, High

/** Conditions with fuzzytool's operators: every operator and its text, F paths, constants, and the core's checks. */
class ExprSuite extends munit.FunSuite:
  test("every operator builds its node, with the core's text"):
    val x = F.x
    val cases = Seq(
      (x === 1, "x = 1", "eq"),
      (x =!= 1, "x != 1", "ne"),
      (x < 1, "x < 1", "lt"),
      (x <= 1, "x <= 1", "le"),
      (x > 1, "x > 1", "gt"),
      (x >= 1, "x >= 1", "ge"),
      (x > 1 & F.y, "x > 1 AND y", "and"),
      (x > 1 | F.y, "x > 1 OR y", "or"),
      (~F.y, "NOT y", "not"),
      (F.country.isIn("PT", "ES", "PT"), "country IN (\"ES\", \"PT\")", "in"),
      (x.between(1, 2), "x BETWEEN 1 AND 2", "between"),
      (x.isNull, "x IS NULL", "is_null"),
      (F.email.matches(".+@.+"), "email MATCHES \".+@.+\"", "matches"),
      (F.name.startsWith("Jo"), "name STARTS WITH \"Jo\"", "starts_with"),
      (F.name.endsWith("hn"), "name ENDS WITH \"hn\"", "ends_with"),
      (F.name.contains("oh"), "name CONTAINS \"oh\"", "contains"),
      (x + 1, "x + 1", "add"),
      (x - 1, "x - 1", "sub"),
      (x * 1.5, "x * 1.5e0", "mul"),
      (x / 2, "x / 2", "div"),
      (-x, "-x", "neg"),
      (x.abs, "ABS(x)", "abs"),
    )
    for (node, text, tag) <- cases do
      assertEquals(node.toString, text)
      assertEquals(node.asJava.tag(), tag)
    assertEquals(x.asJava.tag(), "field")
    assertEquals(Const(1).asJava.tag(), "const")
    assert(x.between(1, 2, Closed.LEFT).toJson.contains("\"closed\":\"left\""))
    assert(x.between(F.lo, F.hi, Closed.NONE).toJson.contains("\"none\""))
    assertEquals(F.country.isIn(Set("ES", "PT")).toString, "country IN (\"ES\", \"PT\")")
    assertEquals(F.country.isIn(Seq("PT", "ES")), F.country.isIn("ES", "PT"))

  test("Scala's precedence: comparisons, then &, then |"):
    assertEquals((F.a > 1 & F.b < 2 | F.c).toString, "a > 1 AND b < 2 OR c")
    assertEquals((F.a > 1 & (F.b < 2 | F.c)).toString, "a > 1 AND (b < 2 OR c)")
    assertEquals((F.a === 1 & F.b =!= 2).toString, "a = 1 AND b != 2")
    assertEquals((F.a + 1 > F.b * 2).toString, "a + 1 > b * 2")
    assertEquals((~(F.a & F.b)).toString, "NOT (a AND b)")
    // Flattened, as fuzzytool's operators chain.
    assertEquals((F.a & F.b & F.c).asJava.children().size, 3)
    assertEquals((Const(1000) < F.total).toString, "1000 < total")

  test("F paths: keys, indexes, apply and backquotes"):
    assertEquals(F.customer.age.toString, "customer.age")
    assertEquals(F.items(0).price.fieldPaths, Seq(Seq("items", 0, "price")))
    assertEquals(F("items", 0, "price"), F.items(0).price)
    assertEquals(F.items(0, "price"), F.items(0).price)
    assertEquals(F("a.b").toString, "`a.b`")
    assertEquals(F.`type`.fieldPaths, Seq(Seq("type")))
    // Keys that clash with a member of a field are reached with apply.
    assertEquals(F.customer("contains").fieldPaths, Seq(Seq("customer", "contains")))
    assertEquals(F.customer("isNull").fieldPaths, Seq(Seq("customer", "isNull")))
    assertEquals(F("apply").fieldPaths, Seq(Seq("apply")))
    // At the top, only apply and the methods of every object clash.
    assertEquals(F.contains.fieldPaths, Seq(Seq("contains")))
    assertEquals(F.isNull.fieldPaths, Seq(Seq("isNull")))
    // A member of a field is the member: F.name.contains is the operator.
    assertEquals(F.name.contains("x").asJava.tag(), "contains")
    assertEquals(F.big(3000000000L).fieldPaths, Seq(Seq("big", 3000000000L)))
    for bad <- Seq(() => F(), () => F(""), () => F.items(-1)) do
      assertEquals(intercept[ExpressionException](bad()).code(), "expression_error")

  test("constants are converted and given back as Scala values"):
    val madrid = ZonedDateTime.of(2026, 11, 5, 9, 30, 0, 0, ZoneId.of("Europe/Madrid"))
    val cases: Seq[(Constant, Any)] = Seq(
      5 -> 5L,
      5L -> 5L,
      (3: Short) -> 3L,
      BigInt(10) -> 10L,
      java.math.BigInteger.TEN -> 10L,
      1.5f -> 1.5,
      2.5 -> 2.5,
      BigDecimal("0.10") -> BigDecimal("0.10"),
      new java.math.BigDecimal("0.10") -> BigDecimal("0.10"),
      "text" -> "text",
      true -> true,
      DayOfWeek.MONDAY -> "MONDAY",
      Level.High -> "High",
      LocalDate.of(2026, 11, 5) -> LocalDate.of(2026, 11, 5),
      madrid -> madrid,
    )
    for (constant, back) <- cases do assertEquals(Const(constant).value, back, constant)
    // A decimal keeps its scale, exactly.
    assertEquals(Const(BigDecimal("0.10")).value.asInstanceOf[BigDecimal].scale, 2)
    assertEquals(Const(BigDecimal("12345678901234567890.123456789")).value, BigDecimal("12345678901234567890.123456789"))
    assertEquals(Const(null).value, None)
    assertEquals(Const(1).asJava.kind(), Kind.VALUE)

  test("constants and nodes that are errors"):
    val bad: Seq[() => Any] = Seq(
      () => Const(Double.NaN),
      () => Const(BigInt(2).pow(70)),
      () => F.x > Double.NaN,
      () => (F.a > 1) > 2,
      () => F.a & (F.b + 1),
      () => F.a.matches("("),
      () => F.a.isIn(),
      () => F.a.isIn(1, "a"),
      () => F.b.between(1, "z"),
    )
    for make <- bad do assertEquals(intercept[ExpressionException](make()).code(), "expression_error")

  test("== is structural identity, as Java's equals"):
    assertEquals(F.a > 1, F.a > 1)
    assertNotEquals(F.a > 1, F.a > 1.0)
    assertEquals((F.a > 1).hashCode, (F.a > 1).hashCode)
    assertEquals(Set(F.a > 1, F.a > 1, F.b).size, 2)
    assertNotEquals[Any, Any](F.a, "a")
    assertEquals(Expr.fromJava((F.a > 1).asJava), F.a > 1)
    assert(Expr.fromJava(F.a.asJava).isInstanceOf[Field])
    assert(Expr.fromJava(Const(1).asJava).isInstanceOf[Const])

  test("one node evaluated on Scala data, with its result tree"):
    case class Person(age: Int, nick: Option[String])
    val tree = (F.age >= 18 & F.nick.isNull).evaluateTree(Person(20, None))
    assertEquals(tree.truth, Some(Truth.TRUE))
    assertEquals(tree.children.map(_.node.toString), Seq("age >= 18", "nick IS NULL"))
    assertEquals(tree.children.head.children.map(_.value), Seq(20L, 18L))
    assert(tree.toJson.contains("\"truth\""))
    assert(tree.toString.startsWith("ResultTree("))
    val value = (F.age * 2).evaluateValueTree(Person(21, Some("x")))
    assertEquals(value.value, 42L)
    assertEquals(value.truth, None)
    val unknown = (F.missing > 1).evaluateTree(Person(1, None), logic = "boolean")
    assertEquals(unknown.truth, Some(Truth.UNKNOWN))
    assertEquals(unknown.reason.map(_.code), Some("missing_field"))
    intercept[io.github.cleanrules.EvaluationException]((F.missing > 1).evaluateTree(Person(1, None), strict = true))
