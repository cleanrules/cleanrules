// The smoke test of the Scala artifact (task G.3b): the Scala 3 layer, and
// the Java API it reaches through the artifact's dependencies, evaluate rules
// with the native library the Java jar bundles for this platform, with no
// override. The clean project of scripts/consumer runs it.

import io.github.cleanrules.{CleanRules, HitPolicy}
import io.github.cleanrules.scaladsl.*
import java.nio.file.{Files, Path}

case class Customer(years: Int, newsletter: Boolean)
case class Order(customer: Customer, total: BigDecimal)

private def check(condition: Boolean, what: => String): Unit =
  if !condition then throw AssertionError(what)

@main def scalaSmoke(): Unit =
  check(System.getProperty("cleanrules.library.path") == null, "cleanrules.library.path must not be set")
  check(System.getenv("CLEANRULES_LIBRARY_PATH") == null, "CLEANRULES_LIBRARY_PATH must not be set")

  val rules = RuleSet(
    Seq(
      Rule("vip", F.customer.years > 5 & F.total > 1000, payload = Map("discount" -> BigDecimal("0.15")), priority = 10),
      Rule("loyal", F.customer.years > 2 | F.customer.newsletter, payload = Map("discount" -> BigDecimal("0.05")), priority = 5),
    ),
    hitPolicy = HitPolicy.PRIORITY,
  )
  val decision = rules.evaluate(Order(Customer(7, newsletter = false), BigDecimal("900")))
  check(decision.output == Some(Map("discount" -> BigDecimal("0.05"))), s"output ${decision.output}")
  check(decision.fired == Seq("loyal"), s"fired ${decision.fired}")
  check(rules("vip").when == (F.customer.years > 5 & F.total > 1000), "vip's condition")

  // From the jar, as JarSmoke.java checks it.
  val library = CleanRules.libraryPath().orElseThrow(() => AssertionError("no library path"))
  val tmpdir = System.getProperty("cleanrules.tmpdir", System.getProperty("java.io.tmpdir"))
  check(library.getFileName.toString == System.mapLibraryName("cleanrules_ffi"), s"file $library")
  check(library.getParent.getFileName.toString.startsWith("cleanrules-native-"), s"directory $library")
  check(library.getParent.getParent == Path.of(tmpdir), s"$library is not in $tmpdir")
  val windows = System.getProperty("os.name").startsWith("Windows")
  check(Files.exists(library) == windows, s"$library ${if windows then "was deleted" else "was not deleted"}")

  println(
    s"cleanrules ${CleanRules.version()} (Scala) on " +
      s"${System.getProperty("os.name")} ${System.getProperty("os.arch")}: ${decision.output.get}",
  )
