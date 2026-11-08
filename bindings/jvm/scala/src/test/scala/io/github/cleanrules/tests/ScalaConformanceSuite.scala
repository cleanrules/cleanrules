package io.github.cleanrules.tests

import java.nio.file.{Files, Path}
import java.util.Locale
import scala.collection.mutable
import scala.jdk.CollectionConverters.*

/**
 * The conformance vectors (conformance/README.md) through the Scala layer:
 * every vector of conformance/vectors as `scala:<id>` ([[ScalaConformance]],
 * which says what the route calls and compares), one test per file. Every
 * failure is reported with its id, what came back and what the vector
 * expects; the vectors the route cannot run are counted and listed with their
 * reason in the report (printed, and written to the file that the system
 * property `cleanrules.conformance.report` names). The JSON route and the
 * Java route run in the Java project's `ConformanceTest`.
 */
class ScalaConformanceSuite extends munit.FunSuite:
  /** The skips the Scala route may have, as the Java route's: values no JVM value holds. Any other is a failure. */
  private val allowedSkips = Set("decimal_infinity", "zone_unknown_to_java")

  private val files = Conformance.files().asScala.toVector
  private val runner = ScalaConformance()
  private val vectors = mutable.LinkedHashMap[String, java.util.Map[?, ?]]()
  private val skipped = mutable.TreeMap[String, Vector[String]]()
  private var passed = 0
  private var failed = 0
  private var started = 0L

  test("start"):
    started = System.nanoTime()

  for (name, items) <- files do
    test(s"vectors in $name"):
      val failures = Vector.newBuilder[String]
      for item <- items.asScala do
        val vector = item.asInstanceOf[java.util.Map[?, ?]]
        val id = vector.get("id").asInstanceOf[String]
        vectors(id) = vector
        val outcome = runner.run(vector, "scala")
        if outcome.passed() then passed += 1
        else if outcome.skip() != null then skipped(outcome.skip()) = skipped.getOrElse(outcome.skip(), Vector()) :+ id
        else
          failed += 1
          failures += s"scala:$id: ${outcome.failure()}"
      val found = failures.result()
      assert(found.isEmpty, s"${found.size} failed:\n${found.mkString("\n")}")

  test("every vector ran the Scala way, but those listed with their reason"):
    val seconds = (System.nanoTime() - started) / 1e9
    val total = files.map(_._2.size).sum
    assert(total >= Conformance.MINIMUM_VECTORS, s"only $total vectors, expected at least ${Conformance.MINIMUM_VECTORS}")
    assertEquals(vectors.size, total, "every vector ran once, and ids do not repeat")
    val skips = skipped.values.map(_.size).sum
    val report = Vector.newBuilder[String]
    report += String.format(
      Locale.ROOT,
      "conformance of the Scala layer on Java %s (%s %s): %d vectors in %d files, %.1f s",
      System.getProperty("java.version"),
      System.getProperty("os.name"),
      System.getProperty("os.arch"),
      Integer.valueOf(total),
      Integer.valueOf(files.size),
      java.lang.Double.valueOf(seconds),
    )
    report += s"scala: $passed of $total passed, $failed failed, $skips skipped"
    for (why, ids) <- skipped do
      report += s"  ${ids.size} skipped: ${Conformance.SKIPS.get(why)}: ${ids.mkString(", ")}"
    val cast = runner.castCells.asScala.toVector
    report += s"scala: grids with a list cell, which Scala's DecisionTable.Cell does not admit, passed by a cast so that" +
      s" the core refuses it: ${cast.size} (${cast.map(_.stripPrefix("scala:")).mkString(", ")})"
    report += "scala: vectors using each Scala class (inputs, constants, grid cells): " +
      runner.forms.asScala.map((form, ids) => s"$form ${ids.size}").mkString(", ")
    val text = report.result().mkString("\n")
    println(text)
    Option(System.getProperty("cleanrules.conformance.report")).foreach: file =>
      val path = Path.of(file)
      Files.createDirectories(path.getParent)
      val _ = Files.writeString(path, text + "\n")
    assertEquals(passed + failed + skips, total, "every vector is counted once")
    for why <- skipped.keys do
      assert(allowedSkips.contains(why), s"scala skips for $why")
      assert(Conformance.SKIPS.get(why) != null, why)
    // Grids run the Scala way: no table_grid vector is skipped.
    val grids = vectors.collect { case (id, vector) if vector.get("given").asInstanceOf[java.util.Map[?, ?]].containsKey("table_grid") => id }
    assert(grids.nonEmpty)
    assert(grids.forall(id => !skipped.values.exists(_.contains(id))), "a grid was skipped")

  test("a wrong expected output fails the Scala way"):
    val all = mutable.LinkedHashMap[String, java.util.Map[?, ?]]()
    for (_, items) <- files; item <- items.asScala do
      val vector = item.asInstanceOf[java.util.Map[?, ?]]
      all(vector.get("id").asInstanceOf[String]) = vector
    val runner = ScalaConformance()
    for Array(id, from, to) <- Conformance.WRONG do
      val vector = all.getOrElse(id, fail(s"no vector $id"))
      val expect = TestJson.write(vector.get("expect"))
      assert(expect.contains(from), s"$id: no $from in $expect")
      val copy = java.util.LinkedHashMap[Any, Any](vector)
      val _ = copy.put("expect", TestJson.parse(expect.replace(from, to)))
      val outcome = runner.run(copy, "scala")
      assert(outcome.failure() != null, s"scala:$id passes with $to for $from")
