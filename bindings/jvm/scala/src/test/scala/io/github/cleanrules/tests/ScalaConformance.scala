package io.github.cleanrules.tests

import io.github.cleanrules.{Aggregate, CleanRulesException, Closed, Format, HitPolicy, HitPolicyException, Kind, Limits, TablePolicy, Truth}
import io.github.cleanrules.scaladsl.*
import io.github.cleanrules.tests.Conformance.{Attempt, How, VectorError}
import io.github.cleanrules.tests.JavaValues.{NoJavaForm, NotGiven}
import io.github.cleanrules.tests.TestJson.{Mode, Num}
import java.math.MathContext
import java.nio.charset.StandardCharsets.UTF_8
import java.time.{Duration, LocalDate, LocalDateTime, Period, ZonedDateTime}
import scala.collection.immutable.{ListMap, SeqMap, VectorMap}
import scala.jdk.CollectionConverters.*
import scala.jdk.OptionConverters.*
import scala.util.boundary
import scala.util.boundary.break

// Case classes for the shapes of map that the vectors' data use most, by
// their keys in order: a map with exactly these keys is, in turn, one of them
// (spec/data-model.md, Java and Scala: a case class is a map of its fields, in
// their order).
final case class AB(a: Any, b: Any)
final case class BA(b: Any, a: Any)
final case class ABC(a: Any, b: Any, c: Any)
final case class ABN(a: Any, b: Any, n: Any)
final case class ABCN(a: Any, b: Any, c: Any, n: Any)
final case class OneA(a: Any)
final case class OneX(x: Any)
final case class Total(total: Any)
final case class Years(years: Any)
final case class Age(age: Any)
final case class CustomerOrder(customer: Any, order: Any)

/**
 * The Scala layer's conformance route, `scala:<id>` (conformance/README.md,
 * Runners): the Java runner ([[Conformance]], a test fixture of the Java
 * project) with one more route, which drives `io.github.cleanrules.scaladsl`
 * only.
 *
 *  - Conditions, rules and rule sets are built in Scala code (`F`, `&`, `|`,
 *    `~`, `===`, the other operators, constants on either side, `Rule(...)` or
 *    its `with...` steps, `RuleSet(...)`) and checked against the core's
 *    reading of the same JSON, with `dumps` and `loads(dumps(x))`.
 *  - Inputs are Scala data made from the vector, never JSON: ints as `Long`,
 *    `Int` or `BigInt` in turn, `Double`, `BigDecimal`, `java.time`, `None`
 *    for null and sometimes `Some(x)` for a member, `Vector`, `List` or
 *    `Array` in turn, maps as `VectorMap`, `ListMap` or `SeqMap` in turn, as a
 *    plain `Map` when their keys are sorted (the layer sorts every other map)
 *    and as a case class when one has their keys.
 *  - Results are compared as the Scala values given back (spec/data-model.md,
 *    Java and Scala, the Scala paragraph of Values given back): `None` for
 *    null, `Long`, `Double`, `BigDecimal` with an unlimited `MathContext`,
 *    `Vector`, `VectorMap`, each class checked, through the layer's case
 *    classes (`Explanation`, `Cause`, `Summary`, `ResultTree`...) taken apart
 *    with patterns, and each `toJson` exactly.
 *  - `read` vectors go through `loads` (as a string or UTF-8 bytes in turn),
 *    `dumps`, `ruleFromText`, `DecisionTable(...)` with Scala cells (grids) and
 *    `RuleSet.readDecision(text, format, limits)`.
 *
 * Never the library's equality: results become trees of the format and
 * [[TestJson.same]] compares them; objects are compared by identity (`eq`).
 */
final class ScalaConformance extends Conformance:
  private type JsonMap = java.util.Map[String, AnyRef]

  /** Grid cells that Scala's `DecisionTable.Cell` type does not admit (a list), passed by a cast. */
  val castCells: java.util.Set[String] = java.util.TreeSet[String]()

  override protected def runRoute(route: String, vector: java.util.Map[?, ?]): String =
    if route != "scala" then super.runRoute(route, vector) else runScala(vector)

  // ---- The vectors, through the Java runner's readers.

  private def obj(value: Any): JsonMap = Conformance.`object`(value)
  private def arr(value: Any): Vector[AnyRef] = Conformance.array(value).asScala.iterator.map(_.asInstanceOf[AnyRef]).toVector
  private def need(value: Any, key: String): AnyRef = Conformance.need(value, key)
  private def text(value: Any, key: String): String = Conformance.text(value, key)

  /** The next of `count` equivalent Scala forms; the vectors' order is fixed, so a run is deterministic. */
  private var turns = 0L
  private def turn(count: Int): Int =
    val chosen = (turns % count).toInt
    turns += 1
    chosen

  /** Put without the value given back (`-Wvalue-discard`). */
  extension (map: java.util.Map[String, AnyRef])
    private def set(key: String, value: AnyRef): Unit =
      val _ = map.put(key, value)

  private def closing[C <: AutoCloseable, T](resource: C)(use: C => T): T =
    try use(resource)
    finally resource.close()

  // ---- From the format to Scala data.

  /** A data value of the format as Scala data, counting the classes used. */
  private def data(json: Any): Any =
    val value = json match
      case null => None
      case bool: java.lang.Boolean => bool.booleanValue
      case string: String => string
      case number: Num => if number.integral then integer(number.text) else number.text.toDouble
      case items: java.util.List[?] => sequence(items.asScala.iterator.map(data).toVector)
      case members: java.util.Map[?, ?] =>
        val only = if members.size == 1 then members.keySet.iterator.next.asInstanceOf[String] else ""
        if only.startsWith("$") then typed(only, members.get(only)) else map(members, caseClasses = true)
      case other => throw VectorError(s"not a value of the format: $other")
    count(value)
    value

  private def count(value: Any): Unit =
    val _ = forms.computeIfAbsent(label(value), _ => java.util.TreeSet[String]()).add(current)

  /** The Scala class of a value, as the report names it. */
  private def label(value: Any): String = value match
    case None => "None"
    case _: Some[?] => "Some"
    case _: java.lang.Boolean => "Boolean"
    case _: java.lang.Long => "Long"
    case _: java.lang.Integer => "Int"
    case _: BigInt => "BigInt"
    case _: java.lang.Double => "Double"
    case _: BigDecimal => "BigDecimal"
    case _: String => "String"
    case _: Vector[?] => "Vector"
    case _: List[?] => "List"
    case _: Array[?] => "Array"
    case _: VectorMap[?, ?] => "VectorMap"
    case _: ListMap[?, ?] => "ListMap"
    case _: SeqMap[?, ?] => "SeqMap"
    case _: Map[?, ?] => "Map"
    case product: Product => s"case class ${product.productPrefix}"
    case other => other.getClass.getSimpleName

  /** An int: `Long`, `Int` (where it fits) or `BigInt` in turn; beyond 64 bits a `BigInt`. */
  private def integer(text: String): Any =
    val value = BigInt(text)
    if !value.isValidLong then value
    else
      turn(3) match
        case 0 => value.toLong
        case 1 => if value.isValidInt then value.toInt else value
        case _ => value

  private def typed(key: String, inner: Any): Any =
    val written = inner match
      case number: Num => number.text
      case string: String => string
      case _ => ""
    key match
      case "$int" => integer(written)
      case "$float" =>
        written match
          case "inf" => Double.PositiveInfinity
          case "-inf" => Double.NegativeInfinity
          case _ => written.toDouble
      case "$decimal" =>
        if written == "inf" || written == "-inf" then throw NoJavaForm("decimal_infinity", s"the decimal $written")
        BigDecimal(written)
      case "$date" => LocalDate.parse(written)
      case "$datetime" => JavaValues.datetime(written)
      case "$map" => map(inner.asInstanceOf[java.util.Map[?, ?]], caseClasses = false)
      case _ => throw VectorError(s"no Scala form for $key")

  /** A list: `Vector`, `List` or `Array` in turn. */
  private def sequence(items: Vector[Any]): Any = turn(3) match
    case 0 => items
    case 1 => items.toList
    case _ => items.toArray[Any]

  /** A member's value: `None` for null, and sometimes `Some(x)`, which the layer reads as `x`. */
  private def member(json: Any): Any =
    if json != null && turn(7) == 0 then
      val wrapped = Some(data(json))
      count(wrapped)
      wrapped
    else data(json)

  /** Whether keys are in the order the layer sorts an unordered map in: by Unicode code point. */
  private def sorted(keys: Seq[String]): Boolean =
    keys.zip(keys.drop(1)).forall((a, b) => java.util.Arrays.compare(a.codePoints.toArray, b.codePoints.toArray) < 0)

  /** A map: an ordered Scala map in turn, a plain `Map` when its keys are sorted, or a case class with its keys. */
  private def map(members: java.util.Map[?, ?], caseClasses: Boolean): Any =
    val pairs = members.asScala.iterator.map((key, value) => key.asInstanceOf[String] -> member(value)).toVector
    val keys = pairs.map(_._1)
    val forms = Vector[() => Any](() => VectorMap.from(pairs), () => ListMap.from(pairs), () => SeqMap.from(pairs))
      ++ Option.when(sorted(keys))(() => Map.from(pairs))
      ++ (if caseClasses then caseClass(keys, pairs.map(_._2)) else None)
    forms(turn(forms.size))()

  private def caseClass(keys: Seq[String], values: Seq[Any]): Option[() => Any] = (keys, values) match
    case (Seq("a", "b"), Seq(a, b)) => Some(() => AB(a, b))
    case (Seq("b", "a"), Seq(b, a)) => Some(() => BA(b, a))
    case (Seq("a", "b", "c"), Seq(a, b, c)) => Some(() => ABC(a, b, c))
    case (Seq("a", "b", "n"), Seq(a, b, n)) => Some(() => ABN(a, b, n))
    case (Seq("a", "b", "c", "n"), Seq(a, b, c, n)) => Some(() => ABCN(a, b, c, n))
    case (Seq("a"), Seq(a)) => Some(() => OneA(a))
    case (Seq("x"), Seq(x)) => Some(() => OneX(x))
    case (Seq("total"), Seq(total)) => Some(() => Total(total))
    case (Seq("years"), Seq(years)) => Some(() => Years(years))
    case (Seq("age"), Seq(age)) => Some(() => Age(age))
    case (Seq("customer", "order"), Seq(customer, order)) => Some(() => CustomerOrder(customer, order))
    case _ => None

  /** A constant's value as a Scala constant. */
  private def constant(json: Any): Constant = data(json).asInstanceOf[Constant]

  private def inputOf(setup: JsonMap): Any = data(need(setup, "input"))

  // ---- Building in Scala code.

  private def isConst(json: Any): Boolean = obj(json).get("op") == "const"

  /** A condition or value node of the format, built with the Scala layer's operators. */
  private def build(json: Any): Expr =
    val node = obj(json)
    text(node, "op") match
      case "field" => field(arr(need(node, "path")))
      case "const" => Const(constant(need(node, "value")))
      case "and" => arr(need(node, "operands")).map(build).reduceLeft(_ & _)
      case "or" => arr(need(node, "operands")).map(build).reduceLeft(_ | _)
      case op if node.containsKey("left") => binary(op, need(node, "left"), need(node, "right"))
      case op => unary(op, node)

  /** A field: `F(segments*)`, or in turn the steps `F.a.b(0)` make (`selectDynamic` and `apply`). */
  private def field(path: Vector[AnyRef]): Field =
    val segments: Vector[Segment] = path.map:
      case index: Num => index.text.toInt
      case key: String => key
      case other => throw VectorError(s"a path segment $other")
    if segments.isEmpty || turn(2) == 0 then F(segments*)
    else
      val first = segments.head match
        case key: String => F.selectDynamic(key)
        case index => F(index)
      segments.tail.foldLeft(first): (field, segment) =>
        segment match
          case key: String => field.selectDynamic(key)
          case index => field(index)

  /** An operand: a constant as a Scala value in turn (not null, which only `Const` takes), or the node. */
  private def operand(json: AnyRef): Operand =
    if isConst(json) && need(json, "value") != null && turn(2) == 0 then constant(need(json, "value"))
    else build(json)

  private def binary(op: String, left: AnyRef, right: AnyRef): Expr =
    // A number, date or datetime on the left of a node: `1000 < F.total`.
    val onTheLeft: Option[LeftConstant] =
      if isConst(left) && !isConst(right) && turn(2) == 0 then
        constant(need(left, "value")) match
          case value: (Int | Long | Double | BigInt | BigDecimal | LocalDate | LocalDateTime |
                java.time.OffsetDateTime | ZonedDateTime | java.time.Instant) =>
            Some(value)
          case _ => None
      else None
    onTheLeft match
      case Some(value) => leftBinary(op, value, build(right))
      case None => rightBinary(op, build(left), operand(right))

  private def rightBinary(op: String, left: Expr, right: Operand): Expr = op match
    case "eq" => left === right
    case "ne" => left =!= right
    case "lt" => left < right
    case "le" => left <= right
    case "gt" => left > right
    case "ge" => left >= right
    case "add" => left + right
    case "sub" => left - right
    case "mul" => left * right
    case "div" => left / right
    case _ => throw VectorError(s"unknown op $op")

  private def leftBinary(op: String, left: LeftConstant, right: Expr): Expr = op match
    case "eq" => left === right
    case "ne" => left =!= right
    case "lt" => left < right
    case "le" => left <= right
    case "gt" => left > right
    case "ge" => left >= right
    case "add" => left + right
    case "sub" => left - right
    case "mul" => left * right
    case "div" => left / right
    case _ => throw VectorError(s"unknown op $op")

  private def unary(op: String, node: JsonMap): Expr =
    val operand0 = build(need(node, "operand"))
    op match
      case "not" => ~operand0
      case "in" =>
        val values = arr(need(node, "values")).map(constant)
        if turn(2) == 0 then operand0.isIn(values*) else operand0.isIn(values)
      case "between" =>
        val low = operand(need(node, "low"))
        val high = operand(need(node, "high"))
        node.get("closed") match
          case closed: String => operand0.between(low, high, Closed.valueOf(closed.toUpperCase(java.util.Locale.ROOT)))
          case _ => operand0.between(low, high)
      case "is_null" => operand0.isNull
      case "matches" => operand0.matches(text(node, "pattern"))
      case "starts_with" => operand0.startsWith(text(node, "text"))
      case "ends_with" => operand0.endsWith(text(node, "text"))
      case "contains" => operand0.contains(text(node, "text"))
      case "abs" => operand0.abs
      case "neg" => -operand0
      case _ => throw VectorError(s"unknown op $op")

  /** A payload's map: ordered in turn, or a plain `Map` (sorted by the layer) when its keys are sorted. */
  private def payloadOf(pairs: Vector[(String, Operand | Null)]): collection.Map[String, Operand | Null] =
    val forms = Vector[() => collection.Map[String, Operand | Null]](
      () => SeqMap.from(pairs),
      () => VectorMap.from(pairs),
      () => ListMap.from(pairs),
    ) ++ Option.when(sorted(pairs.map(_._1)))(() => Map.from(pairs))
    forms(turn(forms.size))()

  /** A rule of the format, built with `Rule(...)` or, in turn, with its `with...` steps. */
  private def buildRule(json: AnyRef): Rule =
    val source = obj(json)
    val name = text(source, "name")
    val when = build(need(source, "when"))
    val payload: Vector[(String, Operand | Null)] = source.get("then") match
      case members: java.util.Map[?, ?] =>
        members.asScala.iterator.map((key, value) => key.asInstanceOf[String] -> operand(value.asInstanceOf[AnyRef])).toVector
      case _ => Vector.empty
    val priority: Option[Long] = Option.when(source.containsKey("priority")):
      data(source.get("priority")) match
        case number: java.lang.Number => number.longValue
        case other => throw VectorError(s"a priority $other")
    val tags: Option[Vector[String]] =
      Option.when(source.containsKey("tags"))(arr(source.get("tags")).map(_.asInstanceOf[String]))
    val description = source.get("description") match
      case text: String => Some(text)
      case _ => None
    val metadata: Option[collection.Map[String, Any]] = Option.when(source.containsKey("metadata")):
      data(source.get("metadata")) match
        case map: collection.Map[?, ?] => map.asInstanceOf[collection.Map[String, Any]]
        case other => throw VectorError(s"metadata that is not a Scala map: $other")
    val action = source.get("do") match
      case name: String => Some(name)
      case _ => None
    if turn(2) == 0 then
      Rule(
        name,
        when,
        payload = payloadOf(payload),
        priority = priority.getOrElse(0L),
        tags = tags.getOrElse(Nil),
        description = description,
        metadata = metadata.getOrElse(SeqMap.empty),
        actionName = action,
      )
    else
      var rule = Rule(name, when)
      for (key, value) <- payload do rule = rule.withPayload(key, value)
      priority.foreach(p => rule = rule.withPriority(p))
      tags.foreach(t => rule = rule.withTags(t*))
      description.foreach(d => rule = rule.withDescription(d))
      metadata.foreach(m => rule = rule.withMetadata(m))
      action.foreach(a => rule = rule.withActionName(a))
      rule

  /** A rule set of the format, built with `RuleSet(...)`. */
  private def buildRuleSet(json: AnyRef): RuleSet =
    val content = obj(json)
    val rules = arr(need(content, "rules")).map(buildRule)
    val hitPolicy = content.get("hit_policy") match
      case name: String => HitPolicy.fromJsonName(name)
      case _ => HitPolicy.FIRST
    val aggregate = content.get("aggregate") match
      case name: String => Some(Aggregate.fromJsonName(name))
      case _ => None
    val logic = content.get("logic") match
      case name: String => name
      case _ => "boolean"
    val strict = java.lang.Boolean.TRUE.equals(content.get("strict"))
    if hitPolicy == HitPolicy.FIRST && aggregate.isEmpty && logic == "boolean" && !strict && turn(2) == 0 then
      RuleSet(rules*)
    else RuleSet(rules, hitPolicy = hitPolicy, aggregate = aggregate, logic = logic, strict = strict)

  /** The rule set built in Scala must be the one the core reads from the vector, and say what it holds. */
  private def checkBuilt(ruleset: RuleSet, setup: JsonMap): String =
    val built = Conformance.content(dumps(ruleset), "ruleset")
    val read = closing(io.github.cleanrules.ffi.RuleSet.read(Conformance.rulesetDocument(setup), Format.JSON)):
      core => Conformance.content(core.write(), "ruleset")
    if !TestJson.same(built, read) then
      return s"the rule set built in Scala writes\n  ${TestJson.write(built)}\nthe core reads\n  ${TestJson.write(read)}"
    val source = obj(read)
    val names = arr(need(source, "rules")).map(rule => text(rule, "name"))
    if ruleset.hitPolicy.jsonName != source.get("hit_policy") || ruleset.logic != source.get("logic")
      || ruleset.strict != java.lang.Boolean.TRUE.equals(source.get("strict"))
      || ruleset.aggregate.map(_.jsonName) != Option(source.get("aggregate"))
      || ruleset.size != names.size || ruleset.rules.map(_.name) != names
    then return s"the rule set built in Scala says $ruleset ${ruleset.logic} ${ruleset.aggregate} ${ruleset.rules}"
    loads(dumps(ruleset)) match
      case again: RuleSet =>
        val written = Conformance.content(dumps(again), "ruleset")
        if TestJson.same(written, read) then "" else s"loads(dumps(ruleset)) writes ${TestJson.write(written)}"
      case other => s"loads(dumps(ruleset)) gives $other"

  /** A node built in Scala must be the one the core reads from the vector. */
  private def checkBuiltNode(node: Expr, json: AnyRef): String =
    val built = TestJson.parse(node.toJson)
    val read = closing(io.github.cleanrules.ffi.Node.read(TestJson.write(json), Format.JSON))(core => TestJson.parse(core.write()))
    if !TestJson.same(built, read) then
      return s"the node built in Scala writes\n  ${TestJson.write(built)}\nthe core reads\n  ${TestJson.write(read)}"
    if node.asJava.tag() != need(read, "op") then return s"tag() ${node.asJava.tag()}, expected ${need(read, "op")}"
    if node.asJava.kind() == Kind.VALUE then return ""
    val dumped = Conformance.content(dumps(node), "condition")
    if !TestJson.same(dumped, read) then return s"dumps of the condition built in Scala gives ${TestJson.write(dumped)}"
    loads(dumps(node)) match
      case again: Expr =>
        val reloaded = TestJson.parse(again.toJson)
        if TestJson.same(reloaded, read) then "" else s"loads(dumps(node)) writes ${TestJson.write(reloaded)}"
      case other => s"loads(dumps(node)) gives $other"

  // ---- Scala values given back, as values of the format.

  /**
   * A value given back to Scala (spec/data-model.md, Java and Scala, Values
   * given back: the Scala layer's paragraph) as a value of the format, each
   * class checked: a Java `null`, a `Some`, an `Int`, a Java list or map, a
   * `List` or a `ListMap` is a [[NotGiven]]; any other object is opaque.
   */
  private def back(value: Any): AnyRef = value match
    case null => throw NotGiven("null given back to Scala, where null is None")
    case None => null
    case number: BigDecimal =>
      if number.mc != MathContext.UNLIMITED then throw NotGiven(s"the BigDecimal $number has the MathContext ${number.mc}")
      JavaValues.`given`(number.bigDecimal)
    case items: Vector[?] =>
      val list = java.util.ArrayList[AnyRef](items.size)
      items.foreach(item => list.add(back(item)))
      list
    case members: VectorMap[?, ?] =>
      val map = java.util.LinkedHashMap[String, AnyRef]()
      var dollar = false
      for (key, item) <- members do
        key match
          case name: String =>
            dollar |= name.startsWith("$")
            val _ = map.set(name, back(item))
          case other => throw NotGiven(s"a map key $other given back")
      if dollar then java.util.Map.of("$map", map) else map
    case _: (java.lang.Boolean | String | java.lang.Long | java.lang.Double | LocalDate | LocalDateTime | ZonedDateTime) =>
      JavaValues.`given`(value)
    case other: (Option[?] | java.lang.Number | java.lang.Character | java.util.Collection[?] | java.util.Map[?, ?] |
          scala.collection.Iterable[?] | Array[?] | java.time.temporal.Temporal | java.time.temporal.TemporalAmount) =>
      throw NotGiven(s"a ${other.getClass.getName} given back to Scala: $other")
    case other => java.util.Map.of("$opaque", other.getClass.getName)

  /** A path given back (`String` keys, `Int` or `Long` indexes) in the format. */
  private def pathOf(path: Seq[Segment]): java.util.List[AnyRef] =
    val out = java.util.ArrayList[AnyRef](path.size)
    path.foreach:
      case key: String => out.add(key)
      case index: Int => out.add(Num.of(index.toLong))
      case index: Long => out.add(Num.of(index))
    out

  private def list(items: Iterable[AnyRef]): java.util.List[AnyRef] = java.util.ArrayList[AnyRef](items.asJavaCollection)

  private def truthJson(truth: Truth): AnyRef = Conformance.truthJson(truth)

  private def reasonOf(reason: Reason): JsonMap =
    val Reason(code, paths, types, _) = reason
    val out = java.util.LinkedHashMap[String, AnyRef]()
    out.set("code", code)
    if paths.nonEmpty then out.set("paths", list(paths.map(pathOf)))
    if types.nonEmpty then out.set("types", list(types))
    out

  /** A [[ResultTree]] in the format (spec/json-format.md, Result trees). */
  private def resultTree(tree: ResultTree): JsonMap =
    val ResultTree(node, truth, value, reason, children) = tree
    val out = java.util.LinkedHashMap[String, AnyRef]()
    out.set("op", node.asJava.tag())
    node match
      case field: Field => out.set("path", pathOf(field.fieldPaths.head))
      case _ =>
    truth match
      case Some(truth) =>
        out.set("truth", truthJson(truth))
        if value != None then throw VectorError(s"a result with a truth value has the value $value")
      case None => out.set("value", back(value))
    reason.foreach(reason => out.set("reason", reasonOf(reason)))
    if children.nonEmpty then out.set("children", list(children.map(resultTree)))
    out

  private def gapOf(gap: Any): AnyRef = gap match
    case days: Period => Conformance.gapOf(days)
    case duration: Duration => Conformance.gapOf(duration)
    case other => back(other)

  private def requirementOf(requirement: Requirement): AnyRef = requirement match
    case CauseGroup(kind, items) => java.util.Map.of(kind.jsonName, list(items.map(requirementOf)))
    case Cause(node, truth, negated, holds, values, reason, counterfactual) =>
      val kleene = truth match
        case Truth.TRUE => !negated
        case Truth.FALSE => negated
        case Truth.UNKNOWN => false
      if holds != kleene then throw VectorError(s"a cause $truth, negated $negated, says holds $holds")
      val out = java.util.LinkedHashMap[String, AnyRef]()
      out.set("condition", TestJson.parse(node.toJson))
      out.set("truth", truthJson(truth))
      out.set("negated", java.lang.Boolean.valueOf(negated))
      out.set(
        "values",
        list(values.map { case FieldValue(path, value) =>
          val pair = java.util.LinkedHashMap[String, AnyRef]()
          pair.set("path", pathOf(path))
          pair.set("value", back(value))
          pair
        }),
      )
      reason.foreach(reason => out.set("reason", reasonOf(reason)))
      counterfactual.foreach { case Counterfactual(path, op, target, gap) =>
        val made = java.util.LinkedHashMap[String, AnyRef]()
        made.set("path", pathOf(path))
        made.set("op", op)
        made.set("target", back(target))
        gap.foreach(gap => made.set("gap", gapOf(gap)))
        out.set("counterfactual", made)
      }
      out

  private def leaves(requirement: Requirement): Vector[Cause] = requirement match
    case cause: Cause => Vector(cause)
    case CauseGroup(_, items) => items.toVector.flatMap(leaves)

  private def noteOf(note: Note): JsonMap =
    val Note(kind, winner, priority, winnerPriority) = note
    val out = java.util.LinkedHashMap[String, AnyRef]()
    out.set("kind", kind.jsonName)
    out.set("winner", winner)
    if priority.isDefined != winnerPriority.isDefined then throw VectorError(s"a note with one priority: $note")
    for p <- priority; w <- winnerPriority do
      out.set("priority", back(p))
      out.set("winner_priority", back(w))
    out

  /** An [[Explanation]] in the format, without the causes' `at` (the case classes do not say it). */
  private def explanationOf(decision: Decision, explanation: Explanation): JsonMap =
    val Explanation(selected, rules) = explanation
    val out = java.util.LinkedHashMap[String, AnyRef]()
    out.set("hit_policy", decision.hitPolicy.jsonName)
    out.set("logic", decision.ruleSet.logic)
    out.set("selected", list(selected))
    out.set(
      "rules",
      list(rules.map { rule =>
        val RuleExplanation(name, status, requirement, note, achievable) = rule
        if !(explanation.forRule(name) eq rule) || !(decision.whyNot(name) eq rule) then
          throw VectorError(s"forRule($name) and whyNot($name) are not the rule's explanation")
        val causes = leaves(requirement)
        if causes.isEmpty || causes.size != rule.causes.size
          || causes.zip(rule.causes).exists((leaf, cause) => !(leaf.asJava eq cause.asJava))
        then throw VectorError(s"causes of $name are not its requirement's, in order")
        val item = java.util.LinkedHashMap[String, AnyRef]()
        item.set("rule", name)
        item.set("status", status.jsonName)
        note.foreach(note => item.set("note", noteOf(note)))
        achievable.foreach(achievable => item.set("achievable", java.lang.Boolean.valueOf(achievable)))
        item.set("requirement", requirementOf(requirement))
        item
      }),
    )
    out

  /** A [[Summary]] in the format (spec/json-format.md, Summaries). */
  private def summaryOf(summary: Summary): JsonMap =
    val Summary(hitPolicy, aggregate, logic, strict, rules, fields, outputs, tags) = summary
    val out = java.util.LinkedHashMap[String, AnyRef]()
    out.set("hit_policy", hitPolicy.jsonName)
    aggregate.foreach(aggregate => out.set("aggregate", aggregate.jsonName))
    out.set("logic", logic)
    if strict then out.set("strict", java.lang.Boolean.TRUE)
    out.set(
      "rules",
      list(rules.map { case RuleSummary(name, priority, outputs, action, actionName) =>
        val item = java.util.LinkedHashMap[String, AnyRef]()
        item.set("name", name)
        item.set("priority", back(priority))
        item.set("outputs", list(outputs))
        if action then item.set("action", java.lang.Boolean.TRUE)
        actionName.foreach(name => item.set("do", name))
        item
      }),
    )
    out.set(
      "fields",
      list(fields.map { case FieldUsage(path, conditions, payloads) =>
        val item = java.util.LinkedHashMap[String, AnyRef]()
        item.set("path", pathOf(path))
        item.set("conditions", list(conditions))
        item.set(
          "payloads",
          list(payloads.map { case FieldUsage.Payload(rule, key) =>
            val pair = java.util.LinkedHashMap[String, AnyRef]()
            pair.set("rule", rule)
            pair.set("key", key)
            pair
          }),
        )
        item
      }),
    )
    out.set("outputs", list(outputs))
    out.set(
      "tags",
      list(tags.toVector.map { (tag, names) =>
        val item = java.util.LinkedHashMap[String, AnyRef]()
        item.set("tag", tag)
        item.set("rules", list(names))
        item
      }),
    )
    out

  /** A [[Decision]] of the Scala layer against the expected decision. */
  private def checkDecision(ruleset: RuleSet, d: Decision, expected: JsonMap, mode: Mode): String = boundary:
    val results = arr(need(expected, "results"))
    if !(d.ruleSet.asJava eq ruleset.asJava) then break("ruleSet is another rule set")
    if d.hitPolicy.jsonName != need(expected, "hit_policy") then break(s"hitPolicy ${d.hitPolicy}")
    if d.results.size != results.size then break(s"${d.results.size} results, expected ${results.size}")
    val rules = ruleset.asJava.rules()
    val outputs = for (got, i) <- d.results.zipWithIndex yield
      val result = obj(results(i))
      val RuleResult(rule, fired, truth, output) = got
      val at = s"results($i): "
      if !(rule.asJava eq rules.get(i)) || rule.name != need(result, "rule") then break(s"${at}rule ${rule.name}")
      if !java.lang.Boolean.valueOf(fired).equals(need(result, "fired")) then break(s"${at}fired $fired")
      val wantTruth = Conformance.truthOf(need(need(result, "when"), "truth"))
      if truth != wantTruth then break(s"${at}truth $truth, expected $wantTruth")
      val want = Conformance.outputOf(result)
      val matches = output match
        case None => want == null
        case Some(map) => want != null && TestJson.same(back(map), want, mode)
      if !matches then break(s"${at}output ${output.map(back).map(TestJson.write)}, expected ${TestJson.write(want)}")
      want
    val names = java.util.ArrayList[AnyRef]()
    val wantOutputs = java.util.ArrayList[AnyRef]()
    for selected <- d.selected do
      val position = d.results.indexWhere(_ eq selected)
      if position < 0 then break("selected holds a result that is not in results")
      names.add(selected.rule.name)
      wantOutputs.add(Option(outputs(position)).getOrElse(java.util.LinkedHashMap[String, AnyRef]()))
    if !TestJson.same(names, need(expected, "selected")) then break(s"selected ${TestJson.write(names)}")
    val fired = list(results.filter(result => need(result, "fired") == java.lang.Boolean.TRUE).map(need(_, "rule")))
    if !TestJson.same(list(d.fired), fired) then break(s"fired ${d.fired}")
    val gotOutputs = list(d.outputs.map(back))
    if !TestJson.same(gotOutputs, wantOutputs, mode) then
      break(s"outputs ${TestJson.write(gotOutputs)}, expected ${TestJson.write(wantOutputs)}")
    val policy = text(expected, "hit_policy")
    val want: AnyRef =
      if expected.containsKey("aggregated") then expected.get("aggregated")
      else if Conformance.SINGLE_HIT.contains(policy) then if wantOutputs.isEmpty then null else wantOutputs.get(0)
      else
        if !Conformance.attempt(() => d.output).error().isInstanceOf[HitPolicyException] then
          break(s"output under $policy: expected a HitPolicyException")
        break(if d.aggregated.isEmpty then "" else "aggregated is not None without an aggregate")
    val output = d.output.map(back).orNull
    if !TestJson.same(output, want, mode) then break(s"output ${TestJson.write(output)}, expected ${TestJson.write(want)}")
    val aggregated = d.aggregated.map(back).orNull
    val wantAggregated = if expected.containsKey("aggregated") then output else null
    if TestJson.same(aggregated, wantAggregated) then "" else s"aggregated ${TestJson.write(aggregated)}"

  /** A decision's `toJson` is the expected document, and `readDecision` reads it back as the same decision. */
  private def checkRecorded(ruleset: RuleSet, decision: Decision, expected: JsonMap): String =
    val written = Conformance.content(decision.toJson, "decision")
    if !TestJson.same(written, expected) then
      return s"toJson\n  got  ${TestJson.write(written)}\n  want ${TestJson.write(expected)}"
    // The JSON document with the defaults, or in turn the YAML one with `write`, its format and limits.
    val (how, read) =
      if turn(2) == 0 then ("readDecision(toJson)", attempt(ruleset.readDecision(decision.toJson)))
      else
        val yaml = decision.write(Format.YAML)
        ("readDecision(write(YAML), YAML, limits)", attempt(ruleset.readDecision(yaml, Format.YAML, Limits.defaults())))
    if read.error() != null then return s"$how: ${read.error().code}: ${read.error().getMessage}"
    closing(read.result().asInstanceOf[Decision]): again =>
      val issue = checkDecision(ruleset, again, expected, Mode.RECORDED)
      if issue.isEmpty then "" else s"$how: $issue"

  // ---- Comparing, with the Scala layer's views of an error.

  /** An error's code and context fields, as the Scala layer's views (`pointerOption`...) give them. */
  private def scalaForm(error: CleanRulesException): JsonMap =
    Conformance.checkClass(error)
    val form = java.util.LinkedHashMap[String, AnyRef]()
    form.set("code", error.code)
    error.pointerOption.foreach(pointer => form.set("pointer", pointer))
    error.lineOption.foreach(line => form.set("line", Num.of(line.toLong)))
    error.columnOption.foreach(column => form.set("column", Num.of(column.toLong)))
    error.name().toScala.foreach(name => form.set("name", name))
    error.ruleOption.foreach(rule => form.set("rule", rule))
    error.pathOption.foreach(path => form.set("path", pathOf(path)))
    if !error.types().isEmpty then form.set("types", list(error.types().asScala))
    if !error.rules().isEmpty then form.set("rules", list(error.rules().asScala))
    error.rowOption.foreach(row => form.set("row", Num.of(row.toLong)))
    form

  private val Exact = How(false, scalaForm, Mode.EXACT, view => view)
  private val ExactRead = How(true, scalaForm, Mode.EXACT, view => view)
  private val Given = How(false, scalaForm, Mode.GIVEN, view => view)
  private val GivenWithoutAt = How(false, scalaForm, Mode.GIVEN, view => Conformance.withoutAt(view))

  private def attempt(step: => Any): Attempt = Conformance.attempt(() => step.asInstanceOf[AnyRef])

  private def compare(outcome: Attempt, expect: JsonMap, key: String, how: How): String =
    Conformance.compare(outcome, expect, key, how)

  private def join(issues: String*): String = Conformance.join(issues.asJava)

  // ---- The route.

  private def runScala(vector: java.util.Map[?, ?]): String =
    val setup = obj(need(vector, "given"))
    val expect = obj(need(vector, "expect"))
    text(setup, "kind") match
      case "read" => read(setup, expect)
      case "summary" => summarize(setup, expect)
      case kind @ ("condition" | "value") => tree(kind, setup, expect)
      case kind @ ("evaluate" | "explain") => evaluate(kind, setup, expect)
      case kind => throw VectorError(s"unknown kind $kind")

  private def evaluate(kind: String, setup: JsonMap, expect: JsonMap): String = boundary:
    val ruleset = buildRuleSet(need(setup, "ruleset"))
    val built = checkBuilt(ruleset, setup)
    if built.nonEmpty then break(built)
    val failing = expect.containsKey("error")
    // `max_values`, through Java's Limits.
    val limits = Option(Conformance.limitsOf(setup)).getOrElse(Limits.defaults())
    if setup.containsKey("inputs") then
      val rows = arr(setup.get("inputs")).map(data)
      val outcome = attempt(ruleset.evaluateMany(if turn(2) == 0 then rows else rows.toList, limits))
      if outcome.error() != null || failing then break(compare(outcome, expect, "decisions", Exact))
      val decisions = arr(need(expect, "decisions"))
      val made = outcome.result().asInstanceOf[Seq[Decision]]
      if made.size != decisions.size then break(s"${made.size} decisions, expected ${decisions.size}")
      for (decision, i) <- made.zipWithIndex do
        closing(decision): decision =>
          val expected = obj(decisions(i))
          val issue = checkDecision(ruleset, decision, expected, Mode.GIVEN)
          val all = if issue.isEmpty then checkRecorded(ruleset, decision, expected) else issue
          if all.nonEmpty then break(s"decision $i: $all")
      break("")
    val input = inputOf(setup)
    val outcome = attempt(ruleset.evaluate(input, limits))
    if outcome.error() != null || failing then break(compare(outcome, expect, "decision", Exact))
    closing(outcome.result().asInstanceOf[Decision]): decision =>
      val expected = obj(need(expect, "decision"))
      val issue = checkDecision(ruleset, decision, expected, Mode.GIVEN)
      val recorded = if issue.isEmpty then checkRecorded(ruleset, decision, expected) else issue
      if recorded.nonEmpty || kind == "evaluate" then break(recorded)
      val explanation = decision.explain
      val explained = compare(attempt(explanationOf(decision, explanation)), expect, "explanation", GivenWithoutAt)
      if explained.nonEmpty then break(explained)
      val json = compare(attempt(Conformance.content(explanation.toJson, "explanation")), expect, "explanation", Exact)
      if json.nonEmpty then break(s"Explanation.toJson: $json")
      if expect.containsKey("text") then Conformance.compareText("explained", explanation.toString, expect.get("text"))
      else ""

  private def summarize(setup: JsonMap, expect: JsonMap): String =
    val ruleset = buildRuleSet(need(setup, "ruleset"))
    val built = checkBuilt(ruleset, setup)
    if built.nonEmpty then return built
    val summary = ruleset.summary
    val issue =
      val objects = compare(attempt(summaryOf(summary)), expect, "summary", Given)
      if objects.nonEmpty then objects
      else compare(attempt(Conformance.content(summary.toJson, "summary")), expect, "summary", Exact)
    if issue.nonEmpty || !expect.containsKey("text") then issue
    else Conformance.compareText("summarised", summary.toString, expect.get("text"))

  private def tree(kind: String, setup: JsonMap, expect: JsonMap): String =
    val json = need(setup, kind)
    val strict = java.lang.Boolean.TRUE.equals(setup.get("strict"))
    val logic = setup.get("logic") match
      case name: String => name
      case _ => "boolean"
    val node = build(json)
    val built = checkBuiltNode(node, json)
    if built.nonEmpty then return built
    val constants = checkConstants(node)
    if constants.nonEmpty then return constants
    val input = inputOf(setup)
    var made: Option[ResultTree] = None
    val evaluated = compare(
      attempt {
        val result = if kind == "value" then node.evaluateValueTree(input, strict) else node.evaluateTree(input, logic, strict)
        made = Some(result)
        resultTree(result)
      },
      expect,
      "result",
      Given,
    )
    if evaluated.nonEmpty then return evaluated
    made match
      case Some(result) =>
        val json = compare(attempt(TestJson.parse(result.toJson)), expect, "result", Exact)
        if json.isEmpty then "" else s"ResultTree.toJson: $json"
      case None => ""

  /** Each constant's `value` is the constant the node writes, as a Scala value given back. */
  private def checkConstants(expr: Expr): String = boundary:
    for node <- expr.asJava.walk().asScala do
      Expr.fromJava(node) match
        case constant: Const =>
          val written = need(TestJson.parse(constant.toJson), "value")
          val value = back(constant.value)
          if !TestJson.same(value, written, Mode.GIVEN) then
            break(s"the constant ${constant.toJson} gives back ${TestJson.write(value)}")
        case _ =>
    ""

  private def checkRule(rule: Rule, source: JsonMap): String = boundary:
    if rule.name != source.get("name") then break(s"name ${rule.name}")
    val priority = if source.containsKey("priority") then source.get("priority") else Num.of(0)
    if !TestJson.same(back(rule.priority), priority, Mode.GIVEN) then break(s"${rule.name}: priority ${rule.priority}")
    if rule.actionName != Option(source.get("do")) then break(s"${rule.name}: actionName ${rule.actionName}")
    if rule.description != Option(source.get("description")) then break(s"${rule.name}: description ${rule.description}")
    // Metadata's keys are literal, even those that start with $ (spec/json-format.md, Rules).
    val metadata = java.util.LinkedHashMap[String, AnyRef]()
    rule.metadata.foreach((key, value) => metadata.set(key, back(value)))
    if !rule.metadata.isInstanceOf[VectorMap[?, ?]] then break(s"${rule.name}: metadata is a ${rule.metadata.getClass}")
    val wantMetadata = if source.containsKey("metadata") then source.get("metadata") else java.util.Map.of()
    if !TestJson.same(metadata, wantMetadata, Mode.GIVEN) then
      break(s"${rule.name}: metadata ${TestJson.write(metadata)}, expected ${TestJson.write(wantMetadata)}")
    val when = TestJson.parse(rule.when.toJson)
    if !TestJson.same(when, source.get("when")) then break(s"${rule.name}: when ${TestJson.write(when)}")
    val keys = rule.payload.keys.toVector
    val wantKeys = source.get("then") match
      case members: java.util.Map[?, ?] => members.keySet.asScala.toVector
      case _ => Vector.empty
    if keys != wantKeys then break(s"${rule.name}: payload keys $keys")
    join((checkConstants(rule.when) +: rule.payload.values.toVector.map(checkConstants))*)

  /** What a document's Scala objects say, against the document. */
  private def checkContent(loaded: DocumentContent, wanted: java.util.Map[?, ?]): String = loaded match
    case expr: Expr => checkConstants(expr)
    case rule: Rule => checkRule(rule, obj(wanted.get("rule")))
    case rules: RuleSet =>
      val sources = arr(need(wanted.get("ruleset"), "rules"))
      if rules.size != sources.size then s"size ${rules.size}"
      else join(rules.rules.zip(sources).map((rule, source) => checkRule(rule, obj(source)))*)
    case table: DecisionTable =>
      val json = TestJson.parse(table.toJson)
      if !TestJson.same(json, wanted) then s"toJson ${TestJson.write(json)}"
      else if table.rows.size != table.ruleSet.size then s"${table.rows.size} rows, ${table.ruleSet.size} rules"
      else ""

  /** A grid as a Scala [[DecisionTable]]: text cells as text, other cells as Scala values. */
  private def table(grid: JsonMap, limits: Limits): DecisionTable =
    val header: Vector[DecisionTable.HeaderItem] = arr(need(grid, "header")).map:
      case text: String => text
      case other => throw VectorError(s"a header item that is not text, which Scala's HeaderItem does not admit: $other")
    val rows: Vector[Vector[DecisionTable.Cell]] = arr(need(grid, "rows")).map: row =>
      arr(row).map:
        case text: String => text
        case null => None
        case cell =>
          data(cell) match
            case collection: (Iterable[?] | Array[?]) =>
              // Not a DecisionTable.Cell: the core refuses it, with its pointer.
              val _ = castCells.add(current)
              collection.asInstanceOf[DecisionTable.Cell]
            case value => if turn(4) == 0 then Some(value.asInstanceOf[Constant]) else value.asInstanceOf[Constant]
    val hitPolicy = grid.get("hit_policy") match
      case name: String => TablePolicy.fromJsonName(name)
      case _ => TablePolicy.FIRST
    val aggregate = grid.get("aggregate") match
      case name: String => Some(Aggregate.fromJsonName(name))
      case _ => None
    val logic = grid.get("logic") match
      case name: String => name
      case _ => "boolean"
    val strict = java.lang.Boolean.TRUE.equals(grid.get("strict"))
    DecisionTable(header, rows, hitPolicy = hitPolicy, aggregate = aggregate, logic = logic, strict = strict, limits = limits)

  /** Whether a string is well-formed UTF-16 (no lone surrogate), so that its UTF-8 bytes say the same. */
  private def wellFormed(text: String): Boolean = text.codePoints.noneMatch(c => c >= 0xd800 && c <= 0xdfff)

  /** `ruleFromText` on a rule text: the rule `loads` reads, a `rule_syntax_error` for a condition or a rule set. */
  private def fromText(text: String, limits: Limits, expect: JsonMap): String =
    val outcome = attempt(ruleFromText(text, limits = limits))
    expect.get("document") match
      case document: java.util.Map[?, ?] if !expect.containsKey("error") =>
        if document.containsKey("rule") then
          if outcome.error() != null then s"ruleFromText: unexpected error ${TestJson.write(scalaForm(outcome.error()))}"
          else
            val written = TestJson.parse(dumps(outcome.result().asInstanceOf[Rule]))
            if TestJson.same(written, document) then "" else s"ruleFromText wrote ${TestJson.write(written)}"
        else if outcome.error() == null || outcome.error().code != "rule_syntax_error" then
          s"ruleFromText of a ${if document.containsKey("condition") then "condition" else "rule set"}: expected a rule_syntax_error"
        else
          Conformance.checkClass(outcome.error())
          ""
      case _ =>
        if expect.containsKey("error") && outcome.error() == null then
          s"ruleFromText read a rule, expected the error ${TestJson.write(expect.get("error"))}"
        else ""

  private def read(setup: JsonMap, expect: JsonMap): String =
    val limits = Option(Conformance.limitsOf(setup)).getOrElse(Limits.defaults())
    if setup.containsKey("ruleset") then
      val source = Conformance.sourceOf(setup)
      val ruleset = buildRuleSet(need(setup, "ruleset"))
      val built = checkBuilt(ruleset, setup)
      if built.nonEmpty then return built
      val outcome = attempt(ruleset.readDecision(source.text, source.format, limits))
      if outcome.error() != null || expect.containsKey("error") then
        return compare(outcome, expect, "document", ExactRead)
      return closing(outcome.result().asInstanceOf[Decision]): decision =>
        val expected = obj(need(need(expect, "document"), "decision"))
        val issue = checkDecision(ruleset, decision, expected, Mode.RECORDED)
        if issue.isEmpty then checkRecorded(ruleset, decision, expected) else issue
    val issues = Vector.newBuilder[String]
    def add(issue: String): Unit =
      val _ = issues += issue
    val outcome = setup.get("table_grid") match
      case grid: java.util.Map[?, ?] => attempt(table(obj(grid), limits))
      case _ =>
        val source = Conformance.sourceOf(setup)
        if source.format == Format.TEXT then add(fromText(source.text, limits, expect))
        if wellFormed(source.text) && turn(2) == 0 then attempt(loads(source.text.getBytes(UTF_8), source.format, limits = limits))
        else attempt(loads(source.text, source.format, limits = limits))
    if outcome.error() != null then
      add(compare(outcome, expect, "document", ExactRead))
      return join(issues.result()*)
    val loaded = outcome.result().asInstanceOf[DocumentContent]
    if expect.containsKey("error") then
      setup.get("write") match
        case target: String =>
          add(compare(attempt(dumps(loaded, Conformance.format(target))), expect, "document", ExactRead))
          return join(issues.result()*)
        case _ => return s"read a $loaded, expected the error ${TestJson.write(expect.get("error"))}"
    expect.get("document") match
      case wanted: java.util.Map[?, ?] =>
        add(compare(attempt(TestJson.parse(dumps(loaded))), expect, "document", ExactRead))
        val key = loaded match
          case _: Expr => "condition"
          case _: Rule => "rule"
          case _: RuleSet => "ruleset"
          case _: DecisionTable => "decision_table"
        if !wanted.containsKey(key) then add(s"loads gave a $key, expected ${TestJson.write(wanted)}")
        val again = TestJson.parse(dumps(loads(dumps(loaded))))
        if !TestJson.same(again, wanted) then add(s"loads(dumps(x)) writes ${TestJson.write(again)}")
        add(checkContent(loaded, wanted))
      case _ =>
    if expect.containsKey("printed") then
      add(Conformance.compareText("printed", dumps(loaded, Format.TEXT), expect.get("printed")))
    if expect.containsKey("csv") then
      add(Conformance.compareText("wrote the CSV", dumps(loaded, Format.CSV), expect.get("csv")))
    if expect.containsKey("compiled") then
      loaded match
        case table: DecisionTable =>
          add(compare(attempt(Conformance.content(dumps(table.ruleSet), "ruleset")), expect, "compiled", ExactRead))
        case other => return s"read a $other, expected a DecisionTable"
    join(issues.result()*)
