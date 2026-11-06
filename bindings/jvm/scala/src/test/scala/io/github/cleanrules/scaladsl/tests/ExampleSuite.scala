package io.github.cleanrules.scaladsl.tests

import java.io.ByteArrayOutputStream

/** The example docs/examples/DiscountRules.scala, compiled with the tests, runs and prints what it says. */
class ExampleSuite extends munit.FunSuite:
  test("DiscountRules.scala"):
    val main = Class.forName("discountRules").getMethod("main", classOf[Array[String]])
    val out = ByteArrayOutputStream()
    Console.withOut(out)(main.invoke(null, Array.empty[String]))
    val text = out.toString("UTF-8")
    for line <- Seq(
        "discount: 0.15",
        "fired: vip, loyal",
        "vip: order.total > 1000 is false; needs gt, short by 100.00",
        "notify_sales for vip: 0.15",
        "missing_field in rule adult: [customer, age]",
      )
    do assert(text.contains(line), text)
    assert(text.contains("[x] vip"), text)
