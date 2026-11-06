package io.github.cleanrules.scaladsl.internal

import java.math.MathContext
import java.util.AbstractMap.SimpleImmutableEntry
import java.util.{AbstractList, AbstractMap, AbstractSet, IdentityHashMap, LinkedHashMap, SequencedMap}
import java.util.concurrent.ConcurrentHashMap
import scala.collection.immutable.VectorMap
import scala.jdk.CollectionConverters.*

/**
 * A Java view of a Scala value (or of a Java list or map holding Scala values),
 * which the Java binding's reader walks lazily: it asks only for the keys and
 * indexes on the paths a rule set reads, so this layer reads no more than the
 * Java reader would (`spec/data-model.md`, *Reading native data*).
 */
private[scaladsl] sealed trait View:
  /** The value this view shows. */
  def underlying: Any

/**
 * Scala values made readable by the Java binding (`spec/data-model.md`,
 * *Java and Scala*): `None` and `Some`, `BigDecimal` and `BigInt`, Scala 3
 * enum cases without parameters, and views of sequences, arrays, maps and
 * case classes. Everything else goes as it is, and the Java reader decides:
 * Java values with its own rules, and every other object of the Scala library
 * (a set, a tuple, a function, a `LazyList`...) and of the binding is opaque,
 * so that it comes back as the very object.
 *
 * One instance converts one input: a view is made once per object, so the
 * Java reader sees the same object each time it meets it (each object is read
 * once, and a cycle ends as an opaque value).
 */
private[scaladsl] final class ToJava:
  private val views = IdentityHashMap[Any, AnyRef]()

  def apply(value: Any): AnyRef = value match
    case null | None => null
    case Some(inner) => apply(inner)
    case optional: java.util.Optional[?] => apply(optional.asInstanceOf[java.util.Optional[AnyRef]].orElse(null))
    case number: BigDecimal => number.bigDecimal
    case number: BigInt => number.bigInteger
    case constant: java.lang.Enum[?] => constant
    // A case without parameters is its name; one with parameters is a case class.
    case constant: scala.reflect.Enum if constant.productArity == 0 => constant.productPrefix
    case bytes: Array[Byte] => bytes
    case array: Array[?] => view(array)(ArrayView(array, this))
    case seq: scala.collection.Seq[?] if !ToJava.lazySeq(seq) => view(seq)(SeqView(seq, this))
    case map: scala.collection.SeqMap[?, ?] => view(map)(OrderedMapView(map, this))
    case map: scala.collection.SortedMap[?, ?] => view(map)(OrderedMapView(map, this))
    case map: scala.collection.Map[?, ?] => view(map)(MapView(map, this))
    case list: java.util.List[?] => view(list)(JavaListView(list, this))
    case map: java.util.SequencedMap[?, ?] => view(map)(OrderedJavaMapView(map, this))
    case map: java.util.Map[?, ?] => view(map)(JavaMapView(map, this))
    // A case object is its name, as an enum case without parameters.
    case product: Product if ToJava.caseObject(product) => product.productPrefix
    case product: Product if ToJava.caseClass(product) => view(product)(ProductView(product, this))
    // Anything else, also a product whose element names are empty or repeated,
    // goes as it is: the Java reader makes every scala.Product opaque.
    case other => other.asInstanceOf[AnyRef]

  private def view(value: Any)(make: => AnyRef): AnyRef =
    val found = views.get(value)
    if found != null then found
    else
      val made = make
      views.put(value, made)
      made

private[scaladsl] object ToJava:
  /** The packages of the binding: their objects are opaque, as the Java reader makes them. */
  private val binding = Set(
    "io.github.cleanrules",
    "io.github.cleanrules.ffi",
    "io.github.cleanrules.internal",
    "io.github.cleanrules.scaladsl",
    "io.github.cleanrules.scaladsl.internal",
  )

  /** The packages whose products are not case classes of the program. */
  private def library(name: String): Boolean =
    name == "scala" || name.startsWith("scala.") || name.startsWith("java.") || name.startsWith("jdk.")
      || binding.contains(name)

  /** A product of the program with element names that are keys: none empty, none repeated. */
  def caseClass(product: Product): Boolean =
    !library(product.getClass.getPackageName) && {
      val names = product.productElementNames.toVector
      names.forall(_.nonEmpty) && names.distinct.size == names.size
    }

  /** A case object of the program: a module (a class named `...$` with `MODULE$`) without elements. */
  def caseObject(product: Product): Boolean =
    product.productArity == 0
      && !library(product.getClass.getPackageName)
      && product.getClass.getName.endsWith("$")
      && product.getClass.getDeclaredFields.exists(field =>
        field.getName == "MODULE$" && java.lang.reflect.Modifier.isStatic(field.getModifiers)
      )

  /** A `LazyList` (or the old `Stream`), which is never forced: forcing it runs code, maybe forever. */
  def lazySeq(seq: scala.collection.Seq[?]): Boolean =
    seq.isInstanceOf[LazyList[?]] || seq.getClass.getName.startsWith("scala.collection.immutable.Stream")

  /** One value converted on its own: a constant or a whole value. */
  def convert(value: Any): AnyRef = ToJava()(value)

/** The Scala value of what the Java binding gives back (`spec/data-model.md`, *Values given back*). */
private[scaladsl] object FromJava:
  def apply(value: Any): Any = value match
    case null => None
    case view: View => view.underlying
    case number: java.math.BigDecimal => new BigDecimal(number, MathContext.UNLIMITED)
    case list: java.util.List[?] => list.asScala.iterator.map(apply).toVector
    case map: java.util.Map[?, ?] => this.map(map)
    case other => other

  def map(map: java.util.Map[?, ?]): VectorMap[String, Any] =
    VectorMap.from(map.asScala.iterator.map((key, value) => key.asInstanceOf[String] -> apply(value)))

  /** The object a run was given, before any view. */
  def original(value: Any): Any = value match
    case view: View => view.underlying
    case other => other

  /** A path: keys, and indexes as Int (or Long beyond them). */
  def path(path: java.util.List[?]): Seq[String | Int | Long] =
    path.asScala.iterator
      .map:
        case index: java.lang.Integer => index.intValue
        case index: java.lang.Long => index.longValue
        case key => key.asInstanceOf[String]
      .toVector

  def paths(paths: java.util.List[? <: java.util.List[?]]): Seq[Seq[String | Int | Long]] =
    paths.asScala.iterator.map(path).toVector

  def optionLong(value: java.util.OptionalLong): Option[Long] =
    if value.isPresent then Some(value.getAsLong) else None

// ---- Views.

private final class SeqView(val underlying: scala.collection.Seq[?], convert: ToJava)
    extends AbstractList[AnyRef]
    with View:
  override def get(index: Int): AnyRef = convert(underlying(index))
  override def size: Int = underlying.length
  override def iterator: java.util.Iterator[AnyRef] = underlying.iterator.map(convert(_)).asJava

private final class ArrayView(val underlying: Array[?], convert: ToJava) extends AbstractList[AnyRef] with View:
  override def get(index: Int): AnyRef = convert(underlying(index))
  override def size: Int = underlying.length

private final class JavaListView(val underlying: java.util.List[?], convert: ToJava)
    extends AbstractList[AnyRef]
    with View:
  override def get(index: Int): AnyRef = convert(underlying.get(index))
  override def size: Int = underlying.size
  override def iterator: java.util.Iterator[AnyRef] = underlying.asScala.iterator.map(convert(_)).asJava

/** Entries in a given order; the Java reader keeps the order of a `SequencedMap`. */
private sealed abstract class Entries(convert: ToJava) extends AbstractMap[Any, AnyRef] with View:
  /** The keys and the values, in order, unconverted. */
  protected def pairs: Iterator[(Any, Any)]
  protected def count: Int

  override def entrySet: java.util.Set[java.util.Map.Entry[Any, AnyRef]] =
    new AbstractSet[java.util.Map.Entry[Any, AnyRef]]:
      override def iterator: java.util.Iterator[java.util.Map.Entry[Any, AnyRef]] =
        pairs.map((key, value) => SimpleImmutableEntry[Any, AnyRef](key, convert(value))).asJava
      override def size: Int = count

  /** A reversed copy: the reader never asks for one. */
  def reversed: SequencedMap[Any, AnyRef] = LinkedHashMap[Any, AnyRef](this).reversed

private class MapView(val underlying: scala.collection.Map[?, ?], convert: ToJava) extends Entries(convert):
  private def scalaMap = underlying.asInstanceOf[scala.collection.Map[Any, Any]]
  // `contains` and `get`, never `apply`: a map's default is not a key.
  override def containsKey(key: Any): Boolean = scalaMap.contains(key)
  override def get(key: Any): AnyRef = scalaMap.get(key) match
    case Some(value) => convert(value)
    case None => null
  protected def pairs: Iterator[(Any, Any)] = scalaMap.iterator
  protected def count: Int = underlying.size

private final class OrderedMapView(underlying: scala.collection.Map[?, ?], convert: ToJava)
    extends MapView(underlying, convert)
    with SequencedMap[Any, AnyRef]:
  override def reversed: SequencedMap[Any, AnyRef] = super.reversed

private class JavaMapView(val underlying: java.util.Map[?, ?], convert: ToJava) extends Entries(convert):
  private def javaMap = underlying.asInstanceOf[java.util.Map[Any, Any]]
  override def containsKey(key: Any): Boolean = javaMap.containsKey(key)
  override def get(key: Any): AnyRef = convert(javaMap.get(key))
  protected def pairs: Iterator[(Any, Any)] = javaMap.asScala.iterator
  protected def count: Int = underlying.size

private final class OrderedJavaMapView(underlying: java.util.SequencedMap[?, ?], convert: ToJava)
    extends JavaMapView(underlying, convert)
    with SequencedMap[Any, AnyRef]:
  override def reversed: SequencedMap[Any, AnyRef] = super.reversed

/**
 * A case class as a map of its public fields, in their order: a field the
 * class declares without a public accessor (a `private val`) is left out. A
 * product written by hand, with no member of an element's name, shows it.
 */
private final class ProductView(val underlying: Product, convert: ToJava)
    extends Entries(convert)
    with SequencedMap[Any, AnyRef]:
  private lazy val fields: Vector[(String, Int)] =
    underlying.productElementNames.zipWithIndex
      .filter((name, _) => ProductView.public(underlying.getClass, name))
      .toVector

  private def position(key: Any): Int = fields.find(_._1 == key).fold(-1)(_._2)

  override def containsKey(key: Any): Boolean = position(key) >= 0
  override def get(key: Any): AnyRef =
    val at = position(key)
    if at < 0 then null else convert(underlying.productElement(at))
  protected def pairs: Iterator[(Any, Any)] = fields.iterator.map((name, at) => name -> underlying.productElement(at))
  protected def count: Int = fields.size
  override def reversed: SequencedMap[Any, AnyRef] = super.reversed

private object ProductView:
  /** For each class, whether a field name has a public accessor. */
  private val accessors = new ClassValue[ConcurrentHashMap[String, java.lang.Boolean]]:
    override def computeValue(kind: Class[?]): ConcurrentHashMap[String, java.lang.Boolean] = ConcurrentHashMap()

  def public(kind: Class[?], name: String): Boolean =
    accessors.get(kind).computeIfAbsent(name, field => check(kind, scala.reflect.NameTransformer.encode(field))).booleanValue

  private def check(kind: Class[?], name: String): java.lang.Boolean =
    try !java.lang.reflect.Modifier.isStatic(kind.getMethod(name).getModifiers)
    catch case _: NoSuchMethodException => !declares(kind, name)

  /** Whether `kind` or a superclass declares a field or an accessor named `name`, of any access. */
  private def declares(kind: Class[?], name: String): Boolean =
    Iterator
      .iterate[Class[?] | Null](kind)(_.nn.getSuperclass)
      .takeWhile(_ != null)
      .map(_.nn)
      .exists(owner =>
        owner.getDeclaredFields.exists(_.getName == name)
          || owner.getDeclaredMethods.exists(method => method.getName == name && method.getParameterCount == 0)
      )
