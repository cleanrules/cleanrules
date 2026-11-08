package io.github.cleanrules.scaladsl.tests

import io.github.cleanrules.{
  Aggregate,
  CleanRulesException,
  ExpressionException,
  Format,
  FormatException,
  HitPolicy,
  HitPolicyException,
  Limits,
  RegistryException,
  RuleDefinitionException,
  RuleSyntaxException,
  TablePolicy,
  UnsupportedException,
}
import io.github.cleanrules.scaladsl.*
import java.time.LocalDate
import scala.collection.immutable.VectorMap

/** Decision tables with Scala cells over the Java binding's DecisionTable: cells, errors with pointers, policies. */
class TablesSuite extends munit.FunSuite:
  import Discounts.order

  private def customer(years: Int, total: Int): Map[String, Any] =
    Map("customer" -> Map("years" -> years), "order" -> Map("total" -> total))

  test("text cells are read as the table's CSV, and the table evaluates Scala data"):
    val table = DecisionTable(
      Seq("RULE", "customer.years", "order.total", "THEN discount", "THEN label"),
      Seq(
        Seq("vip", "> 5", "> 1000", BigDecimal("0.15"), "'VIP'"),
        Seq("loyal", "> 2", "-", BigDecimal("0.05"), None),
      ),
    )
    assertEquals(table.hitPolicy, TablePolicy.FIRST)
    assertEquals(table.aggregate, None)
    assertEquals(table.logic, "boolean")
    assertEquals(table.strict, false)
    assertEquals(table.inputs, Seq(F.customer.years, F.order.total))
    assertEquals(table.outputs, Seq(TableOutput("discount"), TableOutput("label")))
    assertEquals(table.toString, "DecisionTable(first: 2 rows, 4 columns)")
    assertEquals(
      dumps(table, Format.CSV),
      "HIT POLICY,first\nRULE,customer.years,order.total,THEN discount,THEN label\n"
        + "vip,> 5,> 1000,0.15,'VIP'\nloyal,> 2,-,0.05,\n",
    )
    // Case classes as data, Scala values back.
    val decision = table.evaluate(order(9, "1500"))
    assertEquals(decision.output, Some(VectorMap("discount" -> BigDecimal("0.15"), "label" -> "VIP")))
    assertEquals(decision.fired, Seq("vip", "loyal"))
    val many = table.evaluateMany(Seq(order(9, "1500"), customer(3, 0), customer(1, 0)))
    assertEquals(many.map(_.selected.map(_.rule.name)), Seq(Seq("vip"), Seq("loyal"), Nil))
    assertEquals(table.ruleSet.rules.map(_.name), Seq("vip", "loyal"))
    assertEquals(table.ruleSet.hitPolicy, HitPolicy.FIRST)
    assertEquals(table.ruleSet.asJava, table.asJava.ruleSet())
    assertEquals(table.summary.rules.map(_.name), Seq("vip", "loyal"))
    assertEquals(table.summary.toString, table.ruleSet.summary.toString)
    assertEquals(table.toJson, dumps(table))

  test("evaluate and evaluateMany within limits"):
    val table = DecisionTable(Seq("RULE", "a.b.c", "THEN n"), Seq(Seq("deep", "> 1", 1)))
    val data = Map("a" -> Map("b" -> Map("c" -> 2)))
    assertEquals(table.evaluate(data).fired, Seq("deep"))
    val tight = Limits.defaults().withMaxNesting(1)
    assertEquals(intercept[FormatException](table.evaluate(data, tight)).code(), "limit_exceeded")
    val batch = intercept[FormatException](table.evaluateMany(Seq(Map("a" -> 1), data), tight))
    assertEquals(batch.rowOption, Some(1))
    assertEquals(table.evaluateMany(Seq(data), Limits.defaults()).map(_.fired), Seq(Seq("deep")))

  test("rows are the core's, with Scala types"):
    val table = DecisionTable(
      Seq("RULE", "PRIORITY", "DESCRIPTION", "age", "THEN band"),
      Seq(
        Seq("minor", 3, "under age", "< 18", "'minor'"),
        Seq(None, None, None, "-", "'adult'"),
      ),
    )
    table.rows match
      case Seq(TableRow(Some("minor"), Some(3L), Some("under age"), Seq(Some(input)), Seq(Some(output))), adult) =>
        assertEquals(input, F.age < 18)
        assertEquals(output, Const("minor"))
        assertEquals(adult.name, None)
        assertEquals(adult.priority, None)
        assertEquals(adult.description, None)
        assertEquals(adult.inputs, Seq(None))
        assertEquals(adult.outputs, Seq(Some(Const("adult"))))
        assert(adult.asJava eq table.asJava.rows().get(1))
      case other => fail(s"unexpected rows $other")
    assertEquals(table.ruleSet.rules(1).name, "row_2")
    assertEquals(table.rows, table.rows)

  test("each kind of cell: text, node, constant, None and Some"):
    val table = DecisionTable(
      Seq("RULE", F.age, "day", "units", TableOutput("risk", "high", "low"), "THEN tier", "THEN rate"),
      Seq(
        Seq("minor", F.age < 18, None, None, "'high'", "'High'", BigDecimal("0.5")),
        Seq("dated", Some("> 60"), LocalDate.of(2026, 1, 2), None, Some("'low'"), Some("'Low'"), 0.25),
        Seq("exact", 40, None, BigInt(7), "'low'", None, F.age * 2),
        Seq("adult", Some(None), None, None, "'low'"),
      ),
      hitPolicy = TablePolicy.PRIORITY,
    )
    assertEquals(
      dumps(table, Format.CSV),
      "HIT POLICY,priority\n"
        + "RULE,age,day,units,\"THEN risk IN ('high', 'low')\",THEN tier,THEN rate\n"
        + "minor,< 18,-,-,'high','High',0.5\n"
        + "dated,> 60,\"DATE \"\"2026-01-02\"\"\",-,'low','Low',0.25e0\n"
        + "exact,40,-,7,'low',,age * 2\n"
        + "adult,-,-,-,'low',,\n",
    )
    assertEquals(table.rows(2).inputs, Seq(Some(F.age === 40), None, Some(F.units === 7)))
    assertEquals(
      table.evaluate(Map("age" -> 10, "day" -> LocalDate.of(2026, 1, 2))).output,
      Some(VectorMap("risk" -> "high", "tier" -> "High", "rate" -> BigDecimal("0.5"))),
    )
    assertEquals(table.evaluate(Map("age" -> 40, "units" -> 7)).output, Some(VectorMap("risk" -> "low", "rate" -> 80L)))
    // Output values come back as Scala values, equal to the ones given.
    val numbers = DecisionTable(Seq("x", TableOutput("n", 1, 2.5, BigDecimal("0.1"), Level.Low)), Seq(Seq("> 1", 1)))
    assertEquals(numbers.outputs, Seq(TableOutput("n", 1, 2.5, BigDecimal("0.1"), "Low")))
    numbers.outputs.head match
      case TableOutput("n", one, half, tenth, low) =>
        assertEquals[Any, Any](one.getClass, classOf[java.lang.Long])
        assertEquals(half, 2.5)
        assertEquals[Any, Any](tenth.getClass, classOf[BigDecimal])
        assertEquals(low, "Low")
      case other => fail(s"unexpected output $other")
    // An enum case without parameters is a constant, its name as a string (not
    // text, which only strings are), in inputs and outputs, as in a TableOutput.
    val named = DecisionTable(
      Seq("RULE", "x", "THEN t"),
      Seq(Seq("a", Level.High, Level.High), Seq("b", Some(Tier.Gold), Some(Level.Low))),
    )
    assertEquals(named.rows.map(_.inputs), Seq(Seq(Some(F.x === Level.High)), Seq(Some(F.x === "Gold"))))
    assertEquals(named.rows.map(_.outputs), Seq(Seq(Some(Const("High"))), Seq(Some(Const(Level.Low)))))
    assertEquals(named.evaluate(Map("x" -> Level.High)).output, Some(VectorMap("t" -> "High")))
    assertEquals(named.evaluate(Map("x" -> Tier.Gold)).output, Some(VectorMap("t" -> "Low")))
    val listed = DecisionTable(Seq("x", TableOutput("t", Level.High, Level.Low)), Seq(Seq("-", Level.Low)))
    assertEquals(listed.outputs, Seq(TableOutput("t", "High", "Low")))
    assertEquals(listed.evaluate(Map("x" -> 1)).output, Some(VectorMap("t" -> "Low")))
    // A case object as well (not a Cell type, so given as Any at run time, as from Java collections).
    val silver = DecisionTable(Seq("x", "THEN t"), Seq(Seq[Any]("1", Tiers.Silver).asInstanceOf[Seq[DecisionTable.Cell]]))
    assertEquals(silver.rows.head.outputs, Seq(Some(Const("Silver"))))
    // Under RULE and DESCRIPTION, the name or description, unquoted.
    val labels = DecisionTable(Seq("RULE", "x", "THEN t", "DESCRIPTION"), Seq(Seq(Level.High, "> 1", 1, Level.Low)))
    assertEquals((labels.rows.head.name, labels.rows.head.description), (Some("High"), Some("Low")))
    // Rows may be shorter than the header.
    assertEquals(DecisionTable(Seq("a", "b", "THEN n"), Seq(Seq("> 1"))).rows.head.inputs, Seq(Some(F.a > 1), None))

  test("tables of Scala collections: Vector, List, Iterable and rows built in code"):
    val header: Seq[DecisionTable.HeaderItem] = Vector("RULE", "x", "THEN n")
    val rows: List[List[DecisionTable.Cell]] = (1 to 3).map(n => List(s"r$n", s"> $n", n)).toList
    val table = DecisionTable(header, rows, hitPolicy = TablePolicy.COLLECT, aggregate = Some(Aggregate.SUM))
    assertEquals(table.rows.flatMap(_.name), Seq("r1", "r2", "r3"))
    assertEquals(table.evaluate(Map("x" -> 3)).output, Some(VectorMap("n" -> 3L)))
    // Without rows, a table with its header only.
    assertEquals(DecisionTable(Seq("x", "THEN n")).rows, Nil)

  test("hit policies, aggregates, logic and strict mode"):
    val header = Seq("RULE", "x", "THEN n")
    val rows = Seq(Seq("a", "> 0", 1), Seq("b", "> 1", 2))
    val sum = DecisionTable(header, rows, hitPolicy = TablePolicy.COLLECT, aggregate = Some(Aggregate.SUM))
    assertEquals(sum.aggregate, Some(Aggregate.SUM))
    assertEquals(sum.evaluate(Map("x" -> 5)).output, Some(VectorMap("n" -> 3L)))
    val unique = DecisionTable(header, rows, hitPolicy = TablePolicy.UNIQUE)
    intercept[HitPolicyException](unique.evaluate(Map("x" -> 5)))
    val ordered = DecisionTable(
      Seq("RULE", "x", TableOutput("n", 2, 1)),
      rows,
      hitPolicy = TablePolicy.OUTPUT_ORDER,
      strict = true,
    )
    assertEquals(ordered.hitPolicy, TablePolicy.OUTPUT_ORDER)
    assert(ordered.strict)
    assertEquals(ordered.evaluate(Map("x" -> 5)).selected.map(_.rule.name), Seq("b", "a"))
    intercept[CleanRulesException](ordered.evaluate(Map.empty[String, Any]))
    assertEquals(DecisionTable(header, rows, logic = "boolean").logic, "boolean")
    intercept[RegistryException](DecisionTable(header, logic = "nope"))
    val noCollect = intercept[RuleDefinitionException](DecisionTable(header, rows, aggregate = Some(Aggregate.SUM)))
    assertEquals(noCollect.pointerOption, Some("/aggregate"))

  test("errors point into the grid"):
    val header = intercept[RuleSyntaxException](DecisionTable(Seq("RULE", "age >")))
    assertEquals(header.pointerOption, Some("/header/1"))
    val cell = intercept[RuleSyntaxException](DecisionTable(Seq("RULE", "age"), Seq(Seq("a", "< 1"), Seq("b", "<"))))
    assertEquals(cell.pointerOption, Some("/rows/1/1"))
    val tooMany = intercept[FormatException](DecisionTable(Seq("age"), Seq(Seq("1", "2"))))
    assert(tooMany.pointerOption.exists(_.startsWith("/rows/0")), tooMany.pointerOption)
    val nan = intercept[ExpressionException](DecisionTable(Seq("RULE", "age"), Seq(Seq("a", Double.NaN))))
    assertEquals(nan.pointerOption, Some("/rows/0/1"))
    val nanInSome = intercept[ExpressionException](DecisionTable(Seq("RULE", "age"), Seq(Seq("a", Some(Double.NaN)))))
    assertEquals(nanInSome.pointerOption, Some("/rows/0/1"))
    val nanOutput = intercept[ExpressionException](DecisionTable(Seq("age", TableOutput("n", Double.NaN))))
    assertEquals(nanOutput.pointerOption, Some("/header/1"))
    // An enum case with parameters is not a constant: this layer's error, at its cell.
    val shape = intercept[ExpressionException](DecisionTable(Seq("RULE", "shape"), Seq(Seq("a", Shape.Square(2)))))
    assertEquals(shape.code(), "expression_error")
    assertEquals(shape.pointerOption, Some("/rows/0/1"))
    assert(shape.json().contains("\"pointer\":\"/rows/0/1\""), shape.json())
    assert(shape.getMessage.contains("Square(2)"), shape.getMessage)
    val shapeOutput = intercept[ExpressionException](DecisionTable(Seq("age", TableOutput("s", Shape.Square(2)))))
    assertEquals(shapeOutput.pointerOption, Some("/header/1"))
    assertEquals(intercept[ExpressionException](TableOutput("s", Shape.Square(2)).asJava).pointerOption, None)
    val other = intercept[UnsupportedException](DecisionTable(Seq("age"), Seq(Seq(F.x > 1))))
    assertEquals(other.pointerOption, Some("/rows/0/0"))
    val condition = intercept[ExpressionException](DecisionTable(Seq(F.x > 1)))
    assertEquals(condition.pointerOption, Some("/header/0"))
    val notListed = intercept[RuleDefinitionException](
      DecisionTable(Seq("age", TableOutput("r", "a")), Seq(Seq("1", "'b'")))
    )
    assert(notListed.pointerOption.exists(_.startsWith("/rows/0")), notListed.pointerOption)
    val big = intercept[FormatException](
      DecisionTable(Seq("age"), Seq(Seq("< 1"), Seq("< 2")), limits = Limits.defaults().withMaxTableSize(3))
    )
    assertEquals(big.code(), "limit_exceeded")
    val longText = intercept[FormatException](
      DecisionTable(Seq("age"), Seq(Seq("< 100000")), limits = Limits.defaults().withMaxTextSize(3))
    )
    assertEquals(longText.code(), "limit_exceeded")

  test("tables are read and written in each format"):
    val csv = "HIT POLICY,C+\nRULE,age,THEN n\na,> 1,1\nb,> 2,2\n"
    val table = loads(csv, Format.CSV) match
      case read: DecisionTable => read
      case other => fail(s"not a table: $other")
    assertEquals(table.hitPolicy, TablePolicy.COLLECT)
    assertEquals(table.aggregate, Some(Aggregate.SUM))
    for format <- Seq(Format.CSV, Format.JSON, Format.YAML) do
      loads(dumps(table, format), format) match
        case again: DecisionTable =>
          assertEquals(dumps(again), dumps(table))
          assertEquals(again.rows, table.rows)
          assertEquals(again.outputs, table.outputs)
        case other => fail(s"not a table: $other")
    intercept[UnsupportedException](dumps(table, Format.TEXT))
    assertEquals(table.evaluate(Map("age" -> 5)).output, Some(VectorMap("n" -> 3L)))

  test("to and from the Java binding, and identity"):
    val table = DecisionTable(Seq("x", TableOutput("n", 1, 2)), Seq(Seq("> 1", 1)))
    val again = DecisionTable.fromJava(table.asJava)
    assertEquals(again, table)
    assertEquals(again.hashCode, table.hashCode)
    assertNotEquals(DecisionTable(Seq("x", TableOutput("n", 1, 2)), Seq(Seq("> 1", 1))), table)
    assertEquals(TableOutput.fromJava(TableOutput("n", 1, BigDecimal("2.5")).asJava), TableOutput("n", 1L, BigDecimal("2.5")))
    assertEquals(table.outputs.head.asJava, io.github.cleanrules.TableOutput.of("n", 1L, 2L))
    assertEquals(table.rows.head, TableRow.fromJava(table.asJava.rows().getFirst))
