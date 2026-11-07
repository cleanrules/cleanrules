package io.github.cleanrules.scaladsl.tests

import io.github.cleanrules.{
  Documents,
  ExpressionException,
  Format,
  FormatException,
  HitPolicy,
  Limits,
  RegistryException,
  RuleSyntaxException,
  UnsupportedException,
}
import io.github.cleanrules.scaladsl.*
import java.nio.charset.StandardCharsets
import java.nio.file.Files
import scala.collection.immutable.VectorMap
import scala.collection.mutable.ArrayBuffer

/** Documents with Scala types: loads, load, dumps, dump and ruleFromText over the Java binding's Documents. */
class DocumentsSuite extends munit.FunSuite:
  import Discounts.*

  /** What a document holds, by an exhaustive match (no wildcard: -Werror checks it). */
  private def kind(content: DocumentContent): String = content match
    case _: Expr => "condition"
    case _: Rule => "rule"
    case _: RuleSet => "ruleset"
    case _: DecisionTable => "decision_table"

  private val tableCsv = "HIT POLICY,C+\nRULE,age,THEN n\na,> 1,1\nb,> 2,2\n"

  test("each kind of document in each of its formats, both ways"):
    val condition = F.customer.years > 5 & F.order.total > 1000
    val rule = vip.withPayload("tier", "gold")
    val table = loads(tableCsv, Format.CSV)
    val cases: Seq[(DocumentContent, Seq[Format])] = Seq(
      condition -> Seq(Format.JSON, Format.YAML, Format.TEXT),
      rule -> Seq(Format.JSON, Format.YAML, Format.TEXT),
      rules -> Seq(Format.JSON, Format.YAML, Format.TEXT),
      table -> Seq(Format.JSON, Format.YAML, Format.CSV),
    )
    for (value, formats) <- cases; format <- formats do
      val text = dumps(value, format)
      // The Java binding's writer, given the Java object: the layer only delegates.
      assertEquals(text, Documents.dumps(DocumentContent.asJava(value), format), s"$value as $format")
      val back = loads(text, format)
      assertEquals(kind(back), kind(value), s"$value as $format")
      assertEquals(dumps(back), dumps(value), s"$value as $format")
    // JSON by default, compact and canonical.
    assertEquals(dumps(condition), Documents.dumps(condition.asJava))
    assert(dumps(condition).startsWith("{\"version\":1,\"condition\":"), dumps(condition))
    assertEquals(dumps(rule, Format.TEXT).linesIterator.next(), "RULE vip PRIORITY 10 TAGS loyalty")

  test("loads gives the Scala types, with the Scala values of what was read"):
    loads(dumps(F.customer.years > 5)) match
      case condition: Expr => assertEquals(condition, F.customer.years > 5)
      case other => fail(s"not a condition: $other")
    loads(dumps(vip), Format.JSON) match
      case rule: Rule =>
        assertEquals(rule.name, "vip")
        assertEquals(rule.priority, 10L)
        assertEquals(rule.tags, Seq("loyalty"))
        assertEquals(rule.actionName, Some("notify_sales"))
        assertEquals(rule.action, None)
        assertEquals(rule.payload("discount"), Const(BigDecimal("0.15")))
      case other => fail(s"not a rule: $other")
    loads(dumps(rules, Format.YAML), Format.YAML) match
      case read: RuleSet =>
        assertEquals(read.rules.map(_.name), Seq("blocked", "vip", "loyal"))
        assertEquals(read.hitPolicy, HitPolicy.PRIORITY)
        assertEquals(read.evaluate(order(7, "1500")).output, Some(VectorMap("discount" -> BigDecimal("0.15"))))
      case other => fail(s"not a rule set: $other")
    loads(tableCsv, Format.CSV) match
      case table: DecisionTable => assertEquals(table.evaluate(Map("age" -> 5)).output, Some(VectorMap("n" -> 3L)))
      case other => fail(s"not a table: $other")

  test("documents as UTF-8 bytes"):
    val yaml = dumps(rules, Format.YAML)
    val read = loads(yaml.getBytes(StandardCharsets.UTF_8), Format.YAML)
    assertEquals(dumps(read, Format.YAML), yaml)
    val invalid = intercept[FormatException](loads(Array[Byte]('{', 0xff.toByte, '}'), Format.JSON))
    assertEquals(invalid.code(), "format_error")

  test("files: the format of the extension, or the one given, within the limits"):
    val folder = Files.createTempDirectory("cleanrules-scala")
    try
      for (name, format) <- Seq(
          "rules.json" -> Format.JSON,
          "rules.yaml" -> Format.YAML,
          "rules.yml" -> Format.YAML,
          "rules.rules" -> Format.TEXT,
        )
      do
        val file = folder.resolve(name)
        dump(rules, file)
        val written = Files.readString(file)
        assertEquals(written, dumps(rules, format) + (if format == Format.TEXT then "\n" else ""), name)
        assertEquals(dumps(load(file), format), dumps(rules, format), name)
      val table = folder.resolve("table.csv")
      dump(loads(tableCsv, Format.CSV), table)
      assertEquals(kind(load(table)), "decision_table")
      // A format given: any extension.
      val other = folder.resolve("rules.txt")
      intercept[IllegalArgumentException](dump(rules, other))
      dump(rules, other, Some(Format.YAML))
      intercept[IllegalArgumentException](load(other))
      assertEquals(dumps(load(other, Some(Format.YAML))), dumps(rules))
      // A file beyond the size limit of its format is not read.
      val big = intercept[FormatException](load(folder.resolve("rules.yaml"), limits = Limits.defaults().withMaxYamlSize(10)))
      assertEquals(big.code(), "limit_exceeded")
      assert(big.hint().orElseThrow().contains("Limits.withMaxYamlSize"), big.hint())
      intercept[java.io.UncheckedIOException](load(folder.resolve("missing.json")))
    finally
      Files.list(folder).forEach(Files.delete(_))
      Files.delete(folder)

  test("actions bound on load, as Scala functions"):
    val calls = ArrayBuffer[String]()
    val notify: Action = context => calls += s"${context.rule.name}: ${context.output("discount")}"
    val yaml = dumps(rules, Format.YAML)
    val bound = loads(yaml, Format.YAML, actions = Map("notify_sales" -> notify)) match
      case read: RuleSet => read
      case other => fail(s"not a rule set: $other")
    // The very function, as the rule's action.
    assert(bound("vip").action.exists(_ eq notify))
    assertEquals(bound.run(order(7, "1500")).fired, Seq("vip", "loyal"))
    assertEquals(calls.toSeq, Seq("vip: 0.15"))
    // A single rule is bound too.
    loads(dumps(vip), actions = Map("notify_sales" -> notify)) match
      case rule: Rule => assert(rule.action.exists(_ eq notify))
      case other => fail(s"not a rule: $other")
    // Without actions, names stay unbound, and running them is the core's registry_error.
    val unbound = loads(yaml, Format.YAML).asInstanceOf[RuleSet]
    assertEquals(unbound("vip").action, None)
    assertEquals(intercept[RegistryException](unbound.run(order(7, "1500"))).code(), "registry_error")
    // A name the registry lacks, and one no rule uses, are the Java binding's RegistryException.
    val lacking = intercept[RegistryException](loads(yaml, Format.YAML, actions = Map("other" -> notify)))
    assertEquals(lacking.ruleOption, Some("vip"))
    // Names that no rule uses are left aside.
    loads(yaml, Format.YAML, actions = Map("notify_sales" -> notify, "other" -> notify))
    // From a file as well.
    val file = Files.createTempFile("cleanrules-scala", ".yaml")
    try
      dump(rules, file)
      calls.clear()
      load(file, actions = Map("notify_sales" -> notify)).asInstanceOf[RuleSet].run(order(9, "2000"))
      assertEquals(calls.toSeq, Seq("vip: 0.15"))
    finally Files.delete(file)

  test("omitActions: a rule whose action is code without a name"):
    val coded = Rule("coded", F.x > 1, action = Some(_ => ()))
    val one = intercept[UnsupportedException](dumps(coded))
    assertEquals(one.pointerOption, Some("/rule/do"))
    assertEquals(one.ruleOption, Some("coded"))
    val many = intercept[UnsupportedException](dumps(RuleSet(vip, coded), Format.YAML))
    assertEquals(many.pointerOption, Some("/ruleset/rules/1/do"))
    val written = dumps(RuleSet(vip, coded), omitActions = true)
    // The named action is written as its name; the code is left out.
    loads(written) match
      case read: RuleSet =>
        assertEquals(read("vip").actionName, Some("notify_sales"))
        assertEquals(read("coded").actionName, None)
      case other => fail(s"not a rule set: $other")
    val file = Files.createTempFile("cleanrules-scala", ".json")
    try
      intercept[UnsupportedException](dump(coded, file))
      dump(coded, file, omitActions = true)
      assertEquals(Files.readString(file), dumps(coded, omitActions = true))
    finally Files.delete(file)
    // A value node is not a condition document.
    intercept[ExpressionException](dumps(F.total * 2))

  test("loads within limits, and the Java binding's errors with their context"):
    val json = dumps(rules)
    val big = intercept[FormatException](loads(json, limits = Limits.defaults().withMaxSize(10)))
    assertEquals(big.code(), "limit_exceeded")
    val deep = intercept[FormatException](loads(dumps(F.a > 1 & (F.b > 2 | ~(F.c > 3))), limits = Limits.defaults().withMaxDepth(2)))
    assertEquals(deep.code(), "limit_exceeded")
    val unknown = intercept[FormatException](loads(dumps(F.a > 1).dropRight(1) + ""","x":1}"""))
    assertEquals(unknown.pointerOption, Some("/x"))
    val syntax = intercept[RuleSyntaxException](loads("RULE r IF x >", Format.TEXT))
    assertEquals(syntax.lineOption, Some(1))
    assert(syntax.columnOption.isDefined)

  test("ruleFromText with limits and actions"):
    val parsed = ruleFromText("IF customer.years > 5 THEN discount = 0.15")
    assertEquals(parsed.name, "rule_1")
    assertEquals(parsed.when, F.customer.years > 5)
    val text = "RULE vip IF customer.years > 5 THEN discount = 0.15 DO notify"
    assertEquals(ruleFromText(text).actionName, Some("notify"))
    assertEquals(ruleFromText(text).action, None)
    val calls = ArrayBuffer[String]()
    val notify: Action = context => calls += context.rule.name
    val bound = ruleFromText(text, Map("notify" -> notify))
    assert(bound.action.exists(_ eq notify))
    RuleSet(bound).run(Map("customer" -> Map("years" -> 7)))
    assertEquals(calls.toSeq, Seq("vip"))
    intercept[RegistryException](ruleFromText(text, Map("other" -> notify)))
    // Code without a name in the text: a step, which keeps the text's name.
    assertEquals(ruleFromText(text).withAction(notify).actionName, Some("notify"))
    // Limits: the size of the text and the depth of its condition.
    val size = intercept[FormatException](ruleFromText(text, limits = Limits.defaults().withMaxTextSize(10)))
    assertEquals(size.code(), "limit_exceeded")
    val depth = intercept[FormatException](
      ruleFromText("IF a > 1 AND (b > 2 OR NOT c > 3) THEN x = 1", limits = Limits.defaults().withMaxDepth(2))
    )
    assertEquals(depth.code(), "limit_exceeded")
    assertEquals(
      ruleFromText(text, Map("notify" -> notify), Limits.defaults().withMaxTextSize(1000)).actionName,
      Some("notify"),
    )
    // One rule only: a condition or a rule set is a rule_syntax_error.
    intercept[RuleSyntaxException](ruleFromText("x > 1"))
    intercept[RuleSyntaxException](ruleFromText("RULE a IF x > 1 THEN y = 1\nRULE b IF x > 2 THEN y = 2"))

  test("to and from the Java binding"):
    val javaContent = Documents.loads(dumps(rules))
    DocumentContent.fromJava(javaContent) match
      case read: RuleSet => assert(read.asJava eq javaContent)
      case other => fail(s"not a rule set: $other")
    val table = loads(tableCsv, Format.CSV)
    assert(DocumentContent.asJava(table) eq table.asInstanceOf[DecisionTable].asJava)
    assertEquals(kind(DocumentContent.fromJava(Documents.loads(tableCsv, Format.CSV))), "decision_table")
    assertEquals(kind(DocumentContent.fromJava((F.a > 1).asJava)), "condition")
    assertEquals(kind(DocumentContent.fromJava(vip.asJava)), "rule")
    // Java's writer reads Scala objects through asJava.
    assertEquals(Documents.dumps(rules.asJava, Format.YAML), dumps(rules, Format.YAML))
