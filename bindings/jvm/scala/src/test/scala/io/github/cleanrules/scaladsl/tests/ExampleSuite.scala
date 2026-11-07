package io.github.cleanrules.scaladsl.tests

import java.io.ByteArrayOutputStream
import java.nio.file.Path

/** The examples docs/examples/DiscountRules.scala and RiskTable.scala, compiled with the tests, run and print what they say. */
class ExampleSuite extends munit.FunSuite:
  /** What an example's `@main` prints, given `args`. */
  private def run(main: String, args: String*): String =
    val method = Class.forName(main).getMethod("main", classOf[Array[String]])
    val out = ByteArrayOutputStream()
    Console.withOut(out)(method.invoke(null, args.toArray))
    out.toString("UTF-8")

  test("DiscountRules.scala"):
    val text = run("discountRules")
    for line <- Seq(
        "discount: 0.15",
        "fired: vip, loyal",
        "vip: order.total > 1000 is false; needs gt, short by 100.00",
        "notify_sales for vip: 0.15",
        "missing_field in rule adult: [customer, age]",
      )
    do assert(text.contains(line), text)
    assert(text.contains("[x] vip"), text)

  test("RiskTable.scala"):
    val csv = Path.of(System.getProperty("cleanrules.examples"), "risk.csv").toString
    val text = run("riskTable", csv)
    val lines = text.linesIterator.toVector
    assertEquals(lines.head, "risk: medium, discount: 0.05", text)
    assertEquals(lines(1), "HIT POLICY priority: selected big", text)
    for line <- Seq(
        "same table: true",
        "rows: new, vip, big, default",
        "vip: unknown; order.total > 1000 is unknown (missing_field)",
      )
    do assert(lines.contains(line), text)
    assertEquals(lines.last, "notify for vip: customer.years > 5 is true", text)
