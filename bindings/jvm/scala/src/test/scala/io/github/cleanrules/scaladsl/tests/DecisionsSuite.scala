package io.github.cleanrules.scaladsl.tests

import io.github.cleanrules.{
  Aggregate,
  Format,
  FormatException,
  GroupKind,
  HitPolicy,
  HitPolicyException,
  MissingFieldException,
  NoteKind,
  RuleDefinitionException,
  RuleNotFoundException,
  Status,
  Truth,
}
import io.github.cleanrules.scaladsl.*
import java.time.{Duration, LocalDate, LocalDateTime, Period, ZoneId, ZonedDateTime}
import scala.collection.immutable.{SeqMap, VectorMap}
import scala.jdk.CollectionConverters.*

final case class Client(years: Int, newsletter: Boolean, blocked: Option[Boolean] = None)
final case class Purchase(total: BigDecimal)
final case class Order(customer: Client, order: Purchase)

object Discounts:
  val blocked: Rule = Rule(
    "blocked",
    F.customer.blocked,
    payload = Map("discount" -> 0),
    priority = 99,
    description = Some("blocked customers never get a discount"),
  )
  val vip: Rule = Rule(
    "vip",
    F.customer.years > 5 & F.order.total > 1000,
    payload = Map("discount" -> BigDecimal("0.15")),
    priority = 10,
    tags = Seq("loyalty"),
    actionName = Some("notify_sales"),
  )
  val loyal: Rule = Rule(
    "loyal",
    F.customer.years > 2 | F.customer.newsletter,
    payload = Map("discount" -> BigDecimal("0.05")),
    priority = 5,
    tags = Seq("loyalty"),
  )
  val rules: RuleSet = RuleSet(Seq(blocked, vip, loyal), hitPolicy = HitPolicy.PRIORITY)

  def order(years: Int, total: String): Order = Order(Client(years, newsletter = false), Purchase(BigDecimal(total)))

/** Rules, rule sets, decisions, explanations and summaries with Scala types. */
class DecisionsSuite extends munit.FunSuite:
  import Discounts.*

  test("rules: named arguments, steps and accessors"):
    assertEquals(vip.name, "vip")
    assertEquals(vip.when.toString, "customer.years > 5 AND order.total > 1000")
    assertEquals(vip.payload, VectorMap[String, Expr]("discount" -> Const(BigDecimal("0.15"))))
    assertEquals(vip.priority, 10L)
    assertEquals(vip.tags, Seq("loyalty"))
    assertEquals(vip.actionName, Some("notify_sales"))
    assertEquals(vip.action, None)
    assertEquals(blocked.description, Some("blocked customers never get a discount"))
    assertEquals(loyal.description, None)
    val stepped = Rule("r", F.a)
      .withPayload("b", 1)
      .withPayload(SeqMap("z" -> F.z, "a" -> "x"))
      .withPriority(3)
      .withTags("t1", "t2")
      .withDescription("d")
      .withMetadata(SeqMap("owner" -> "risk", "levels" -> Seq(1, 2), "extra" -> Map("k" -> None)))
      .withActionName("act")
    assertEquals(stepped.payload.keys.toSeq, Seq("b", "z", "a"))
    assertEquals(stepped.priority, 3L)
    assertEquals(stepped.tags, Seq("t1", "t2"))
    assertEquals(stepped.description, Some("d"))
    assertEquals(stepped.metadata, VectorMap("owner" -> "risk", "levels" -> Vector(1L, 2L), "extra" -> VectorMap("k" -> None)))
    assertEquals(stepped.actionName, Some("act"))
    // A map without an order is sorted by key; a SeqMap keeps its order.
    assertEquals(Rule("r", F.a, payload = Map("b" -> 1, "a" -> 2, "c" -> 3)).payload.keys.toSeq, Seq("a", "b", "c"))
    assertEquals(Rule("r", F.a, payload = SeqMap("b" -> 1, "a" -> 2)).payload.keys.toSeq, Seq("b", "a"))
    assertEquals(Rule("r", F.a, payload = Map("n" -> null)).payload("n"), Const(null))
    // Rules compare by identity.
    assertEquals(vip, vip)
    assertNotEquals(Rule("r", F.a), Rule("r", F.a))
    assertEquals(Rule.fromJava(vip.asJava), vip)
    assert(vip.toString.contains("vip"))

  test("rules that break the core's checks"):
    for make <- Seq(() => Rule("", F.a), () => Rule("r", F.a + 1), () => Rule("r", F.a, payload = Map("p" -> (F.a > 1)))) do
      assertEquals(intercept[RuleDefinitionException](make()).code(), "rule_definition_error")
    intercept[RuleDefinitionException](RuleSet(Rule("r", F.a), Rule("r", F.b)))
    intercept[RuleDefinitionException](RuleSet(Seq(Rule("r", F.a)), aggregate = Some(Aggregate.SUM)))

  test("rule sets: lookup, settings and field paths"):
    assertEquals(rules.size, 3)
    assertEquals(rules.rules.map(_.name), Seq("blocked", "vip", "loyal"))
    assertEquals(rules.get("vip"), Some(vip))
    assertEquals(rules.get("nope"), None)
    assertEquals(rules("loyal"), loyal)
    assert(rules.contains("vip"))
    val missing = intercept[RuleNotFoundException](rules("nope"))
    assertEquals(missing.available().asScala.toSeq, Seq("blocked", "loyal", "vip"))
    assertEquals(rules.hitPolicy, HitPolicy.PRIORITY)
    assertEquals(rules.aggregate, None)
    assertEquals(rules.logic, "boolean")
    assert(!rules.strict)
    assertEquals(
      rules.fieldPaths,
      Seq(Seq("customer", "blocked"), Seq("customer", "years"), Seq("order", "total"), Seq("customer", "newsletter")),
    )
    assertEquals(RuleSet.fromJava(rules.asJava), rules)
    assertEquals(rules.toString, rules.asJava.toString)

  test("decisions say what fired and was selected, with Scala values"):
    val decision = rules.evaluate(order(7, "1500.00"))
    assertEquals(decision.ruleSet, rules)
    assertEquals(decision.hitPolicy, HitPolicy.PRIORITY)
    assertEquals(decision.fired, Seq("vip", "loyal"))
    assertEquals(decision.selected.map(_.rule), Seq(vip))
    assertEquals(decision.output, Some(VectorMap("discount" -> BigDecimal("0.15"))))
    assertEquals(decision.outputs, Seq(VectorMap("discount" -> BigDecimal("0.15"))))
    assertEquals(decision.aggregated, None)
    decision.results match
      case Seq(RuleResult(first, false, Truth.UNKNOWN, None), RuleResult(_, true, Truth.TRUE, Some(_)), last) =>
        assertEquals(first, blocked)
        assertEquals(last.output, None)
        assertEquals(last.truth, Truth.TRUE)
      case other => fail(s"unexpected results $other")
    assertEquals(decision.toString, "Decision(priority: selected [vip])")
    assertEquals(rules.evaluate(order(0, "1")).output, None)
    // Read back from its document.
    val again = rules.readDecision(decision.toJson)
    assertEquals(again.fired, decision.fired)
    assert(decision.write(Format.YAML).contains("hit_policy:"))
    assertEquals(rules.readDecision(decision.write(Format.YAML), Format.YAML).fired, decision.fired)
    intercept[FormatException](RuleSet(loyal).readDecision(decision.toJson))
    // A batch.
    val many = rules.evaluateMany(Seq(order(7, "1500"), order(3, "10"), order(0, "0")))
    assertEquals(many.map(_.selected.map(_.rule.name)), Seq(Seq("vip"), Seq("loyal"), Nil))
    assertEquals(
      rules.evaluateMany(List(order(7, "1500")), io.github.cleanrules.Limits.defaults()).map(_.fired),
      Seq(Seq("vip", "loyal")),
    )
    // Closing is optional, and twice does nothing.
    decision.close()
    decision.close()
    assertEquals(decision.fired, Seq("vip", "loyal"))

  test("multi-hit policies, aggregates and strict mode"):
    val a = Rule("a", Const(true), payload = Map("n" -> 1))
    val b = Rule("b", Const(true), payload = Map("n" -> 2))
    val all = RuleSet(Seq(a, b), hitPolicy = HitPolicy.RULE_ORDER)
    assertEquals(all.evaluate(Map.empty).outputs, Seq(VectorMap("n" -> 1L), VectorMap("n" -> 2L)))
    assertEquals(intercept[HitPolicyException](all.evaluate(Map.empty).output).code(), "hit_policy_error")
    val sum = RuleSet(Seq(a, b), hitPolicy = HitPolicy.COLLECT, aggregate = Some(Aggregate.SUM))
    assertEquals(sum.aggregate, Some(Aggregate.SUM))
    assertEquals(sum.evaluate(Map.empty).aggregated, Some(VectorMap("n" -> 3L)))
    val strict = RuleSet(Seq(Rule("adult", F.age >= 18)), strict = true)
    assert(strict.strict)
    val missing = intercept[MissingFieldException](strict.evaluate(Map.empty))
    assertEquals(missing.rule().orElseThrow(), "adult")
    // Scala views of the error's context.
    assertEquals(missing.ruleOption, Some("adult"))
    assertEquals(missing.pathOption, Some(Seq("age")))
    assertEquals(missing.lineOption, None)
    assertEquals(missing.pointerOption, None)
    assertEquals(missing.columnOption, None)
    assertEquals(missing.rowOption, None)
    val syntax = intercept[io.github.cleanrules.RuleSyntaxException](ruleFromText("RULE r IF x >"))
    assertEquals(syntax.lineOption, Some(1))
    assert(syntax.columnOption.isDefined)
    assertEquals(RuleSet(Seq(a), logic = "boolean").logic, "boolean")
    intercept[io.github.cleanrules.RegistryException](RuleSet(Seq(a), logic = "nope"))

  test("every value kind comes back as a Scala value"):
    val opaque = new Object
    val madrid = ZonedDateTime.of(2026, 11, 5, 9, 30, 0, 0, ZoneId.of("Europe/Madrid"))
    val data = SeqMap[String, Any](
      "bool" -> true,
      "int" -> 7,
      "float" -> 1.5,
      "decimal" -> BigDecimal("0.10"),
      "string" -> "text",
      "date" -> LocalDate.of(2026, 11, 5),
      "naive" -> LocalDateTime.of(2026, 11, 5, 9, 30),
      "aware" -> madrid,
      "list" -> List(1, "a"),
      "map" -> Map("k" -> 1),
      "opaque" -> opaque,
      "null" -> None,
    )
    val echo = Rule("echo", Const(true), payload = SeqMap.from(data.keys.map(key => key -> F(key))))
    val output = RuleSet(echo).evaluate(data).output.get
    assertEquals(output.keys.toSeq, data.keys.toSeq)
    assertEquals(output("bool"), true)
    assertEquals(output("int"), 7L)
    assertEquals(output("float"), 1.5)
    assertEquals(output("decimal"), BigDecimal("0.10"))
    assertEquals(output("string"), "text")
    assertEquals(output("date"), LocalDate.of(2026, 11, 5))
    assertEquals(output("naive"), LocalDateTime.of(2026, 11, 5, 9, 30))
    assertEquals(output("aware"), madrid)
    assertEquals(output("list"), Vector(1L, "a"))
    assertEquals(output("map"), VectorMap("k" -> 1L))
    assert(output("opaque").asInstanceOf[AnyRef] eq opaque)
    assertEquals(output("null"), None)
    // Each reads back as the same value.
    assertEquals(RuleSet(echo).evaluate(output).output.get, output)

  test("explanations as case classes, with the core's text"):
    val decision = rules.evaluate(order(7, "900.00"))
    val explanation = decision.explain
    assert(explanation eq decision.explain)
    assertEquals(explanation.toString, decision.asJava.explain().toString)
    assertEquals(explanation.selected, Seq("loyal"))
    assertEquals(explanation.rules.map(_.rule), Seq("blocked", "vip", "loyal"))
    assert(explanation.toJson.contains("\"explanation\""))
    val why = decision.whyNot("vip")
    assertEquals(why.status, Status.NOT_FIRED)
    assertEquals(why.achievable, Some(true))
    val total = why.causes.head
    assertEquals(total.node.toString, "order.total > 1000")
    total.values match
      case Seq(read @ FieldValue(Seq("order", "total"), value)) =>
        assertEquals(value, BigDecimal("900.00"))
        assertEquals(read.asJava.path().size, 2)
      case other => fail(s"unexpected values $other")
    assertEquals(total.toString, "order.total > 1000 is false (order.total = 900.00); short by 100.00")
    total match
      case Cause(node, Truth.FALSE, false, false, _, None, Some(Counterfactual(path, "gt", 1000L, Some(gap)))) =>
        assertEquals(node, F.order.total > 1000)
        assertEquals(path, Seq("order", "total"))
        assertEquals(gap, BigDecimal("100.00"))
      case other => fail(s"unexpected cause $other")
    assertEquals(why.requirement, total)
    rules.evaluate(order(1, "1.00")).whyNot("vip").requirement match
      case CauseGroup(GroupKind.ALL, items) => assertEquals(items.size, 2)
      case other => fail(s"unexpected requirement $other")
    assertEquals(decision.whyNot("loyal").status, Status.SELECTED)
    val unknown = decision.whyNot("blocked")
    assertEquals(unknown.status, Status.UNKNOWN)
    // The field is there, holding None: null, not missing.
    unknown.requirement match
      case Cause(_, Truth.UNKNOWN, _, _, _, Some(Reason("null", paths, Seq(), None)), _) =>
        assertEquals(paths, Seq(Seq("customer", "blocked")))
      case other => fail(s"unexpected requirement $other")
    assertEquals(intercept[RuleNotFoundException](decision.whyNot("nope")).code(), "rule_not_found")
    assertEquals(explanation.forRule("vip"), why)
    // A note, and gaps between dates and datetimes.
    val dated = RuleSet(
      Seq(
        Rule("early", F.d < LocalDate.of(2026, 1, 1), priority = 1),
        Rule("any", Const(true), priority = 2),
        Rule("low", Const(true)),
      ),
      hitPolicy = HitPolicy.PRIORITY,
    ).evaluate(Map("d" -> LocalDate.of(2026, 1, 3)))
    assertEquals(dated.whyNot("early").causes.head.counterfactual.flatMap(_.gap), Some(Period.ofDays(2)))
    dated.whyNot("low").note match
      case Some(Note(NoteKind.LOWER_PRIORITY, "any", Some(0L), Some(2L))) => ()
      case other => fail(s"unexpected note $other")
    assertEquals(dated.whyNot("low").status, Status.FIRED)
    val timed = RuleSet(Rule("t", F.at < LocalDateTime.of(2026, 1, 1, 0, 0)))
      .evaluate(Map("at" -> LocalDateTime.of(2026, 1, 1, 0, 0, 1, 500_000_000)))
    assertEquals(timed.whyNot("t").causes.head.counterfactual.flatMap(_.gap), Some(Duration.ofMillis(1500)))

  test("summaries as case classes, with the core's text"):
    val summary = rules.summary
    assertEquals(summary.toString, rules.asJava.summary().toString)
    assertEquals(summary.hitPolicy, HitPolicy.PRIORITY)
    assertEquals(summary.aggregate, None)
    assertEquals(summary.logic, "boolean")
    assert(!summary.strict)
    assertEquals(summary.rules.map(_.name), Seq("blocked", "vip", "loyal"))
    summary.rules(1) match
      case RuleSummary("vip", 10L, Seq("discount"), true, Some("notify_sales")) => ()
      case other => fail(s"unexpected rule summary $other")
    assertEquals(summary.fields(1).path, Seq("customer", "years"))
    assertEquals(summary.fields(1).conditions, Seq("vip", "loyal"))
    assertEquals(summary.fields(1).payloads, Nil)
    assertEquals(summary.outputs, Seq("discount"))
    assertEquals(summary.tags, VectorMap("loyalty" -> Vector("vip", "loyal")))
    assert(summary.toJson.contains("\"summary\""))
    val echo = RuleSet(Rule("e", F.a, payload = Map("b" -> F.b))).summary
    echo.fields.find(_.path == Seq("b")).map(_.payloads) match
      case Some(Seq(payload @ FieldUsage.Payload("e", "b"))) => assertEquals(payload.asJava.rule(), "e")
      case other => fail(s"unexpected payloads $other")
    // Equal summaries of the same rules.
    assertEquals(rules.summary.rules, summary.rules)

  test("documents and decision tables through the Java API"):
    import io.github.cleanrules.{DecisionTable, Documents}
    val calls = scala.collection.mutable.ArrayBuffer[String]()
    val yaml = Documents.dumps(rules.asJava, Format.YAML)
    val loaded = Documents.loads(yaml, Format.YAML) match
      case read: io.github.cleanrules.RuleSet => RuleSet.fromJava(read).bind(Map("notify_sales" -> (ctx => calls += ctx.rule.name)))
      case other => fail(s"unexpected document $other")
    assertEquals(loaded.run(order(7, "1500")).fired, Seq("vip", "loyal"))
    assertEquals(calls.toSeq, Seq("vip"))
    val table = DecisionTable.of(
      java.util.List.of("RULE", "customer.years", "THEN discount"),
      java.util.List.of(java.util.List.of("vip", "> 5", java.math.BigDecimal("0.15"))),
    )
    assertEquals(RuleSet.fromJava(table.ruleSet()).evaluate(order(7, "1")).output, Some(VectorMap("discount" -> BigDecimal("0.15"))))

  test("rules from text"):
    val parsed = ruleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15")
    assertEquals(parsed.name, "vip")
    assertEquals(parsed.when, F.customer.years > 5)
    assertEquals(parsed.payload("discount"), Const(BigDecimal("0.15")))
    intercept[io.github.cleanrules.RuleSyntaxException](ruleFromText("x > 1"))
