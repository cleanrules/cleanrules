/*
 * The decision table of risk.csv with the Scala 3 layer (task G.2b), as
 * RiskTable.java does in Java: read from its file and taken apart with a
 * match, built again from Scala rows, evaluated on a case class and explained,
 * written as YAML; then one rule read from text within limits, a result tree,
 * and a rule set written as a document and read back with a Scala function
 * bound to its action's name.
 *
 * The Scala tests compile and run it (`./gradlew :scala:test` in
 * bindings/jvm). By hand, as DiscountRules.scala, with risk.csv as its argument:
 *
 *     scala run --java-opt --enable-native-access=ALL-UNNAMED \
 *         --java-prop cleanrules.library.path=target/debug \
 *         --jar bindings/jvm/java/build/libs/cleanrules-<version>.jar \
 *         --jar bindings/jvm/scala/build/libs/cleanrules-scala_3-<version>.jar \
 *         docs/examples/RiskTable.scala -- docs/examples/risk.csv
 */

import io.github.cleanrules.{Format, Limits, TablePolicy}
import io.github.cleanrules.scaladsl.*
import java.nio.file.Path

case class Buyer(years: Int)
case class Basket(total: BigDecimal)
case class Sale(customer: Buyer, order: Basket)

@main def riskTable(csv: String): Unit =
  // A decision table from its CSV file (the format comes from the extension).
  val table = load(Path.of(csv)) match
    case table: DecisionTable => table
    case other => sys.error(s"$csv does not hold a decision table but $other")
  val sale = Sale(Buyer(9), Basket(BigDecimal(6000)))
  val decision = table.evaluate(sale)
  for output <- decision.output do println(s"risk: ${output("risk")}, discount: ${output("discount")}")
  println(decision.explain.toString.linesIterator.next())

  // The same table built from Scala rows: strings are cells as the CSV writes
  // them, nodes test their column, None is an empty cell, other values are
  // constants, and a TableOutput lists an output's values.
  val built = DecisionTable(
    Seq("RULE", F.customer.years, "order.total", "THEN discount", TableOutput("risk", "high", "medium", "low"), "DESCRIPTION"),
    Seq(
      Seq("new", F.customer.years < 1, "> 1000", 0, "'high'", "New customers with large orders are checked first"),
      Seq("vip", "> 5", "-", BigDecimal("0.15"), "'low'", "Long-standing customers"),
      Seq("big", None, "> 5000", BigDecimal("0.05"), "'medium'"),
      Seq("default", None, None, 0, "'low'"),
    ),
    hitPolicy = TablePolicy.PRIORITY,
  )
  println(s"same table: ${dumps(built, Format.CSV) == dumps(table, Format.CSV)}")
  println(s"rows: ${built.rows.flatMap(_.name).mkString(", ")}")
  dumps(built, Format.YAML).linesIterator.take(3).foreach(println)

  // One rule from text (fuzzytool's verb), within limits, and why its condition is unknown.
  val rule = ruleFromText(
    "RULE vip IF customer.years > 5 AND order.total > 1000 THEN discount = 0.15",
    limits = Limits.defaults().withMaxTextSize(4096),
  )
  val tree = rule.when.evaluateTree(Map("customer" -> Map("years" -> 7)))
  val total = tree.children(1)
  println(s"${rule.name}: ${tree.truth.map(_.jsonName).getOrElse("?")}; ${total.node} is "
    + s"${total.truth.map(_.jsonName).getOrElse("?")} (${total.reason.map(_.code).getOrElse("?")})")

  // A rule set written as a document and read back, its action bound to a Scala function.
  val rules = RuleSet(ruleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15 DO notify"))
  val notify: Action = context =>
    println(s"notify for ${context.rule.name}: ${context.tree.node} is ${context.tree.truth.map(_.jsonName).getOrElse("?")}")
  val back = loads(dumps(rules, Format.YAML), Format.YAML, actions = Map("notify" -> notify)) match
    case read: RuleSet => read
    case other => sys.error(s"the document holds no rule set but $other")
  val _ = back.run(sale)
