/*
 * The rules of discounts.rules, built in code with the Scala 3 layer (task
 * G.2): fuzzytool's operators on F paths, rules with named arguments, case
 * classes as data, Scala values back (a BigDecimal discount), the explanation
 * as case classes and as text, why a rule did not fire, the summary, a named
 * action bound to a function, and a strict-mode error.
 *
 * The Scala tests compile and run it (`./gradlew :scala:test` in
 * bindings/jvm). By hand, with the C ABI built (`cargo build -p cleanrules-ffi`)
 * and the jars (`./gradlew jar`) on the class path of Scala 3.3:
 *
 *     scala run --java-opt --enable-native-access=ALL-UNNAMED \
 *         --java-prop cleanrules.library.path=target/debug \
 *         --jar bindings/jvm/java/build/libs/cleanrules-<version>.jar \
 *         --jar bindings/jvm/scala/build/libs/cleanrules-scala_3-<version>.jar \
 *         docs/examples/DiscountRules.scala
 */

import io.github.cleanrules.{HitPolicy, MissingFieldException}
import io.github.cleanrules.scaladsl.*

case class Customer(years: Int, newsletter: Boolean, blocked: Boolean = false)
case class Purchase(total: BigDecimal)
case class Order(customer: Customer, order: Purchase)

@main def discountRules(): Unit =
  val rules = RuleSet(
    Seq(
      Rule(
        "blocked",
        F.customer.blocked,
        payload = Map("discount" -> BigDecimal(0)),
        priority = 99,
        description = Some("blocked customers never get a discount"),
      ),
      Rule(
        "vip",
        F.customer.years > 5 & F.order.total > 1000,
        payload = Map("discount" -> BigDecimal("0.15")),
        priority = 10,
        tags = Seq("loyalty"),
        actionName = Some("notify_sales"),
      ),
      Rule(
        "loyal",
        F.customer.years > 2 | F.customer.newsletter,
        payload = Map("discount" -> BigDecimal("0.05")),
        priority = 5,
        tags = Seq("loyalty"),
      ),
    ),
    hitPolicy = HitPolicy.PRIORITY,
  )

  // Case classes in, Scala values out: the discount is a BigDecimal.
  val order = Order(Customer(7, newsletter = false), Purchase(BigDecimal("1500.00")))
  val decision = rules.evaluate(order)
  println(s"discount: ${decision.output.map(_("discount")).getOrElse("none")}")
  println(s"fired: ${decision.fired.mkString(", ")}")
  println()
  println(decision.explain)
  println()

  // Why a rule did not fire, as case classes.
  val small = rules.evaluate(order.copy(order = Purchase(BigDecimal("900.00"))))
  for cause <- small.whyNot("vip").causes do
    val needed = cause.counterfactual match
      case Some(Counterfactual(_, op, _, Some(gap))) => s"; needs $op, short by $gap"
      case _ => ""
    println(s"vip: ${cause.node} is ${cause.truth.jsonName}$needed")
  println()
  println(rules.summary)
  println()

  // A named action is data; the program binds it to a function.
  val bound = rules.bind(
    Map("notify_sales" -> (ctx => println(s"notify_sales for ${ctx.rule.name}: ${ctx.output("discount")}")))
  )
  bound.run(order)

  // Strict mode throws instead of giving unknown.
  val strict = RuleSet(Seq(Rule("adult", F.customer.age >= 18)), strict = true)
  try
    strict.evaluate(Map("customer" -> Map.empty))
    println("no error")
  catch
    case error: MissingFieldException =>
      println()
      println(s"${error.code} in rule ${error.rule.orElseThrow}: ${error.path.orElseThrow}")
