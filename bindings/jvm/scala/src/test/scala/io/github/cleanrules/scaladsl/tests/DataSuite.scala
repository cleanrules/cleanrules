package io.github.cleanrules.scaladsl.tests

import io.github.cleanrules.{EvaluationException, FormatException, Limits, Truth}
import io.github.cleanrules.scaladsl.*
import java.time.LocalDate
import scala.collection.immutable.{ListMap, SortedMap, TreeMap, VectorMap}

final case class Address(city: String, zip: Option[String])
final case class Customer(name: String, years: Int, address: Address, tags: Seq[String] = Nil)
final case class Secretive(open: Int, private val hidden: Int)
final case class Pricing(total: BigDecimal, units: BigInt, rate: Double)
final case class Access(
    open: Int,
    protected val guarded: Int,
    private[tests] val qualified: Int,
    private val hidden: Int,
    private[this] val mine: Int,
):
  def sum: Int = hidden + mine

enum Tier:
  case Gold

object Tiers:
  case object Silver
  case object Bronze

/** A hand-written product whose element names are not keys. */
final class Unnamed(names: String*) extends Product:
  def canEqual(that: Any): Boolean = false
  def productArity: Int = names.size
  override def productElementName(n: Int): String = names(n)
  def productElement(n: Int): Any = n

/** A JavaBean holding a case class: the Java reader reads the bean, and the case class is opaque. */
final class Holder:
  def getInner: Address = Address("Madrid", None)

enum Shape:
  case Circle
  case Square(side: Int)

/** A product that counts its reads and throws on one element: what the reader touches. */
final class Probe extends Product:
  val reads: Array[Int] = Array(0, 0, 0)
  def canEqual(that: Any): Boolean = false
  def productArity: Int = 3
  override def productElementName(n: Int): String = Seq("a", "b", "boom")(n)
  def productElement(n: Int): Any =
    reads(n) += 1
    if n == 2 then throw IllegalStateException("no boom today") else n

/** A product that holds itself: a cycle. */
final class Loop extends Product:
  def canEqual(that: Any): Boolean = false
  def productArity: Int = 1
  override def productElementName(n: Int): String = "me"
  def productElement(n: Int): Any = this

/** A plain class: read by the Java binding as a JavaBean. */
final class Bean:
  def getZip: Option[String] = Some("28001")
  def getCity: String = "Madrid"

/** Scala data read by the Java binding (`spec/data-model.md`, *Java and Scala*). */
class DataSuite extends munit.FunSuite:
  /** What `value` reads as: the output of a rule that gives it back whole. */
  def read(value: Any): Any =
    RuleSet(Rule("r", F.ok, payload = Map("v" -> F.v))).evaluate(Map("ok" -> true, "v" -> value)).output.get("v")

  test("case classes are maps of their fields, by name, in their order"):
    val customer = Customer("Ana", 7, Address("Madrid", Some("28001")), Seq("vip"))
    val rules = RuleSet(Rule("r", F.years > 5 & F.address.city === "Madrid", payload = Map("who" -> F.address)))
    val decision = rules.evaluate(customer)
    assertEquals(decision.fired, Seq("r"))
    val address = decision.output.get("who").asInstanceOf[VectorMap[String, Any]]
    assertEquals(address.toSeq, Seq("city" -> "Madrid", "zip" -> "28001"))
    assertEquals(read(customer).asInstanceOf[VectorMap[String, Any]].keys.toSeq, Seq("name", "years", "address", "tags"))
    // A private field is left out.
    assertEquals(read(Secretive(1, 2)), VectorMap("open" -> 1L))
    assertEquals((F.hidden.isNull).evaluateTree(Secretive(1, 2)).children.head.reason.map(_.code), Some("missing_field"))

  test("fields with a public accessor in the bytecode: private and private[this] are left out"):
    val read = this.read(Access(1, 2, 3, 4, 5)).asInstanceOf[VectorMap[String, Any]]
    assertEquals(read, VectorMap("open" -> 1L, "guarded" -> 2L, "qualified" -> 3L))
    assertEquals(Access(1, 2, 3, 4, 5).sum, 9)

  test("case objects are their names, as enum cases without parameters"):
    assertEquals(read(Tiers.Silver), "Silver")
    assertEquals((F.tier === "Bronze").evaluateTree(Map("tier" -> Tiers.Bronze)).truth, Some(Truth.TRUE))
    assertEquals(read(Tier.Gold), "Gold")

  test("hand-written products with empty or repeated element names are opaque"):
    for product <- Seq(Unnamed("a", ""), Unnamed("a", "a")) do assert(read(product).asInstanceOf[AnyRef] eq product)
    assertEquals(read(Unnamed("a", "b")), VectorMap("a" -> 0L, "b" -> 1L))

  test("a case class inside a JavaBean is opaque: the Java reader never reads it as a bean"):
    val holder = read(Holder()).asInstanceOf[VectorMap[String, Any]]
    assert(holder("inner").isInstanceOf[Address])
    val tree = (F.h.inner.city === "Madrid").evaluateTree(Map("h" -> Holder()))
    assertEquals(tree.truth, Some(Truth.UNKNOWN))

  test("Option: Some is its value, None is null (present), an absent key is missing"):
    val zipped = (F.zip === "28001").evaluateTree(Address("Madrid", Some("28001")))
    assertEquals(zipped.truth, Some(Truth.TRUE))
    val none = (F.zip === "28001").evaluateTree(Address("Madrid", None))
    assertEquals(none.truth, Some(Truth.UNKNOWN))
    assertEquals(none.reason.map(_.code), Some("null"))
    val absent = (F.zip === "28001").evaluateTree(Map("city" -> "Madrid"))
    assertEquals(absent.reason.map(_.code), Some("missing_field"))
    assertEquals(F.zip.isNull.evaluateTree(Address("Madrid", None)).truth, Some(Truth.TRUE))
    assertEquals(read(Some(Some(3))), 3L)
    assertEquals(read(None), None)
    assertEquals(read(java.util.Optional.of(4)), 4L)

  test("numbers: BigDecimal stays exact, BigInt beyond 64 bits is a null with int_overflow"):
    val pricing = Pricing(BigDecimal("1234.50"), BigInt(3), 0.5)
    assertEquals(read(pricing), VectorMap("total" -> BigDecimal("1234.50"), "units" -> 3L, "rate" -> 0.5))
    assertEquals(read(BigDecimal("0.1")).asInstanceOf[BigDecimal].bigDecimal, java.math.BigDecimal("0.1"))
    val tree = (F.n > 1).evaluateTree(Map("n" -> BigInt(2).pow(70)))
    assertEquals(tree.children.head.reason.map(_.code), Some("int_overflow"))
    assertEquals(read(1.toByte), 1L)
    assertEquals(read(2.5f), 2.5)

  test("Scala collections: Seq, List, Vector, arrays and maps"):
    assertEquals(read(List(1, 2)), Vector(1L, 2L))
    assertEquals(read(Vector("a")), Vector("a"))
    assertEquals(read(Array(1, 2, 3)), Vector(1L, 2L, 3L))
    assertEquals(read(Array(Address("A", None))), Vector(VectorMap("city" -> "A", "zip" -> None)))
    assertEquals(read(Range(0, 3)), Vector(0L, 1L, 2L))
    // A SeqMap and a SortedMap keep their order; any other map is sorted by key.
    assertEquals(read(ListMap("b" -> 1, "a" -> 2)).asInstanceOf[VectorMap[String, Any]].keys.toSeq, Seq("b", "a"))
    assertEquals(read(VectorMap("b" -> 1, "a" -> 2)).asInstanceOf[VectorMap[String, Any]].keys.toSeq, Seq("b", "a"))
    val reversed = TreeMap("a" -> 1, "b" -> 2)(using Ordering.String.reverse)
    assertEquals(read(reversed: SortedMap[String, Int]).asInstanceOf[VectorMap[String, Any]].keys.toSeq, Seq("b", "a"))
    assertEquals(read(Map("b" -> 1, "a" -> 2, "c" -> 3)).asInstanceOf[VectorMap[String, Any]].keys.toSeq, Seq("a", "b", "c"))
    // Only string keys are keys.
    assertEquals(read(Map(1 -> "x")), VectorMap.empty)
    // Paths through sequences and maps.
    val data = Map("items" -> List(Map("price" -> 3), Map("price" -> 12)), "byKey" -> Map("x" -> Vector(1, 2)))
    assertEquals((F.items(1).price > 10 & F.byKey.x(0) === 1).evaluateTree(data).truth, Some(Truth.TRUE))
    assertEquals((F.items(5).price > 10).evaluateTree(data).children.head.reason.map(_.code), Some("missing_field"))
    // A map's default is not a key.
    assertEquals((F.k.isNull).evaluateTree(Map("x" -> 1).withDefaultValue(9)).truth, Some(Truth.TRUE))

  test("Java lists and maps hold Scala values too"):
    val javaList = java.util.List.of[Any](Address("A", Some("1")), BigDecimal("2.5"))
    assertEquals(read(javaList), Vector(VectorMap("city" -> "A", "zip" -> "1"), BigDecimal("2.5")))
    val javaMap = java.util.LinkedHashMap[String, Any]()
    javaMap.put("z", Some(1))
    javaMap.put("a", List(2))
    assertEquals(read(javaMap), VectorMap("z" -> 1L, "a" -> Vector(2L)))
    assertEquals(read(java.util.HashMap[String, Any](java.util.Map.of("b", 1, "a", 2))).asInstanceOf[VectorMap[String, Any]].keys.toSeq, Seq("a", "b"))

  test("enums: Java's and Scala 3 cases without parameters are their names"):
    assertEquals(read(java.time.DayOfWeek.FRIDAY), "FRIDAY")
    assertEquals(read(Shape.Circle), "Circle")
    assertEquals(read(Shape.Square(2)), VectorMap("side" -> 2L))
    assertEquals((F.shape === Shape.Circle).evaluateTree(Map("shape" -> Shape.Circle)).truth, Some(Truth.TRUE))
    assertEquals(read(LocalDate.of(2026, 11, 6)), LocalDate.of(2026, 11, 6))

  test("sets, tuples, functions, lazy lists and the binding's objects are opaque, given back as the very object"):
    var forced = 0
    val lazyList = LazyList.continually { forced += 1; forced }
    val function = (x: Int) => x + 1
    val rule = Rule("inner", F.x)
    for opaque <- Seq[AnyRef](Set(1, 2), (1, "a"), function, lazyList, rule, F.x > 1, Iterator(1)) do
      assert(read(opaque).asInstanceOf[AnyRef] eq opaque, opaque.getClass.getName)
    assertEquals(forced, 0)
    // Nothing is read through them.
    assertEquals((F.s.size > 0).evaluateTree(Map("s" -> Set(1))).children.head.reason.map(_.code), Some("missing_field"))
    assertEquals((F.t._1 === 1).evaluateTree(Map("t" -> (1, 2))).children.head.reason.map(_.code), Some("missing_field"))
    assertEquals((F.l(0) === 1).evaluateTree(Map("l" -> lazyList)).children.head.reason.map(_.code), Some("missing_field"))
    assertEquals(forced, 0)
    // The type's name is the object's class.
    val mismatch = (F.s > 1).evaluateTree(Map("s" -> Set(1)))
    assertEquals(mismatch.reason.map(_.code), Some("type_mismatch"))

  test("a cycle ends as an opaque value, given back as the very object"):
    val loop = Loop()
    assertEquals(read(loop), VectorMap("me" -> loop))

  test("other objects are read by the Java binding, and a Scala value inside one is opaque"):
    val bean = read(Bean()).asInstanceOf[VectorMap[String, Any]]
    assertEquals(bean("city"), "Madrid")
    assertEquals(bean("zip"), Some("28001"))
    assertEquals((F.zip === "28001").evaluateTree(Bean()).reason.map(_.code), Some("type_mismatch"))

  test("Scala values given to the Java binding directly are opaque too"):
    val javaRules = io.github.cleanrules.RuleSet.of(
      io.github.cleanrules.Rule.named("r").when(io.github.cleanrules.Conditions.field("ok")).`then`(
        "v",
        io.github.cleanrules.Conditions.field("v"),
      )
    )
    val set = Set(1, 2)
    val output = javaRules.evaluate(java.util.Map.of("ok", true, "v", set)).output().get()
    assert(output.get("v") eq set)
    // A case class handed to Java as it is is opaque (a scala.Product), never a JavaBean.
    val address = Address("A", None)
    val raw = javaRules.evaluate(java.util.Map.of("ok", true, "v", address)).output().get()
    assert(raw.get("v") eq address)

  test("only the paths read, each element once; a read that throws is a null with read_error"):
    val probe = Probe()
    val decision = RuleSet(Rule("r", F.p.a === 0, payload = Map("b" -> F.p.b))).evaluate(Map("p" -> probe))
    assertEquals(decision.fired, Seq("r"))
    assertEquals(probe.reads.toSeq, Seq(1, 1, 0))
    val failed = (F.boom === 1).evaluateTree(Probe())
    assertEquals(failed.children.head.reason.map(_.code), Some("read_error"))
    assert(failed.children.head.reason.flatMap(_.detail).exists(_.contains("no boom today")))
    val strict = RuleSet(Seq(Rule("r", F.boom === 1)), strict = true)
    val error = intercept[EvaluationException](strict.evaluate(Probe()))
    assert(error.getCause.isInstanceOf[IllegalStateException])

  test("the data of a run is the very object, and opaque outputs come back as themselves"):
    val customer = Customer("Ana", 7, Address("Madrid", None))
    var seen: Any = null
    val rules = RuleSet(Rule("r", F.years > 1, payload = Map("tags" -> F.tags), action = Some(ctx => seen = ctx.data)))
    rules.run(customer)
    assert(seen.asInstanceOf[AnyRef] eq customer)
    val decisions = rules.evaluateMany(Seq(customer, Map("years" -> 0), Customer("B", 3, Address("X", None))))
    assertEquals(decisions.map(_.fired), Seq(Seq("r"), Nil, Seq("r")))

  test("max_values: Scala data beyond the limit is refused in every mode (spec/data-model.md, Limits)"):
    def beyond(call: => Any): FormatException =
      val error = intercept[FormatException](call)
      assertEquals(error.code, "limit_exceeded")
      assertEquals(error.name.orElseThrow, "max_values")
      error
    // A vector holding the same vector twice, 30 times over: 2^30 references.
    val diamond = (0 until 30).foldLeft[Any](1)((inner, _) => Vector(inner, inner))
    val rules = RuleSet(Rule("r", F.d.isNull), Rule("s", F.x === 1))
    beyond(rules.evaluate(Map("d" -> diamond, "x" -> 1)))
    beyond(RuleSet(Seq(Rule("r", F.d.isNull)), strict = true).evaluate(Map("d" -> diamond)))
    beyond((F.d === 1).evaluateTree(Map("d" -> diamond)))
    // Java's Limits, here too: 3 levels are 15 values, plus `x`.
    val small = Map("d" -> (0 until 3).foldLeft[Any](1)((inner, _) => Vector(inner, inner)), "x" -> 1)
    assertEquals(rules.evaluate(small, Limits.defaults().withMaxValues(16)).results.map(_.truth), Seq(Truth.FALSE, Truth.TRUE))
    beyond(rules.evaluate(small, Limits.defaults().withMaxValues(15)))
    // A case class is a map of its fields: only those read count.
    assertEquals(
      RuleSet(Rule("r", F.a.city === "Madrid")).evaluate(Map("a" -> Address("Madrid", None)), Limits.defaults().withMaxValues(1)).fired,
      Seq("r"),
    )
    beyond((F.d === 1).evaluateTree(small, "boolean", false, Limits.defaults().withMaxValues(3)))
    beyond((F.d).evaluateValueTree(small, false, Limits.defaults().withMaxValues(3)))
    val rows = Seq(Map("x" -> 1), Map("x" -> Vector(1, 1, 1)))
    assertEquals(beyond(rules.evaluateMany(rows, Limits.defaults().withMaxValues(2))).row.getAsInt, 1)
