package io.github.cleanrules.scaladsl

import io.github.cleanrules.{Closed, Conditions, ExpressionException}
import io.github.cleanrules.scaladsl.internal.{FromJava, ToJava}
import scala.jdk.CollectionConverters.*
import scala.language.dynamics

/**
 * A node of a condition tree (`spec/ast.md`): a condition such as
 * `customer.age >= 18`, or a value such as `order.total * 0.9`, built with
 * fuzzytool's operators.
 *
 * {{{
 * val adult = F.customer.age >= 18 & F.country.isIn("ES", "PT")
 * val open = ~F.account.closed
 * val price = F.order.total * BigDecimal("0.9")
 * }}}
 *
 * `&`, `|` and `~` are AND, OR and NOT; `<`, `<=`, `>` and `>=` compare; `===`
 * and `=!=` build `eq` and `ne`; `+`, `-`, `*`, `/` and unary `-` are
 * arithmetic. Scala's precedence puts comparisons above `&`, and `&` above
 * `|`, so `F.a > 1 & F.b < 2 | F.c` needs no parentheses; a prefix operator
 * binds first, so write `~(F.a >= 18)` (`~F.a >= 18` compares a condition, an
 * error). A number, a date or a datetime may stand on the left too:
 * `1000 < F.total`.
 *
 * Beware: `a == b` and `a.eq(b)` are Scala's, and give a `Boolean`: `==` is
 * the structural identity of `spec/ast.md` (as `equals` in Java) and `eq` the
 * identity of the objects. `F.a == 1` is `false`, not a condition: build
 * equality with `===` and `=!=`.
 *
 * The core builds every node and applies the construction rules, so a node
 * that breaks one (a condition compared, an invalid pattern) is an
 * `io.github.cleanrules.ExpressionException` with the core's message. Nodes
 * are immutable and may be shared between threads; `toString` is the node's
 * text in the rule syntax (`spec/text.md`), as explanations write it.
 *
 * @param asJava the node of the Java binding
 */
sealed abstract class Expr private[scaladsl] (val asJava: io.github.cleanrules.Expr):
  // ---- Comparisons.

  /** `this == other`: an `eq` node. */
  def ===(other: Operand): Expr = Expr(asJava.eq(Expr.operand(other)))

  /** `this != other`: an `ne` node. */
  def =!=(other: Operand): Expr = Expr(asJava.ne(Expr.operand(other)))

  /** `this < other`. */
  def <(other: Operand): Expr = Expr(asJava.lt(Expr.operand(other)))

  /** `this <= other`. */
  def <=(other: Operand): Expr = Expr(asJava.le(Expr.operand(other)))

  /** `this > other`. */
  def >(other: Operand): Expr = Expr(asJava.gt(Expr.operand(other)))

  /** `this >= other`. */
  def >=(other: Operand): Expr = Expr(asJava.ge(Expr.operand(other)))

  // ---- Connectives (fuzzytool's operators).

  /** AND (a t-norm under a fuzzy logic), flattened: `a & b & c` is one `and` node. */
  def &(other: Expr): Expr = Expr(asJava.and(other.asJava))

  /** OR (an s-norm under a fuzzy logic), flattened. */
  def |(other: Expr): Expr = Expr(asJava.or(other.asJava))

  /** NOT. */
  def unary_~ : Expr = Expr(asJava.not())

  // ---- Arithmetic.

  /** `this + other`. */
  def +(other: Operand): Expr = Expr(asJava.add(Expr.operand(other)))

  /** `this - other`. */
  def -(other: Operand): Expr = Expr(asJava.sub(Expr.operand(other)))

  /** `this * other`. */
  def *(other: Operand): Expr = Expr(asJava.mul(Expr.operand(other)))

  /** `this / other`. */
  def /(other: Operand): Expr = Expr(asJava.div(Expr.operand(other)))

  /** The negation of a value: `-x`. */
  def unary_- : Expr = Expr(asJava.neg())

  /** The absolute value. */
  def abs: Expr = Expr(asJava.abs())

  // ---- Tests.

  /** Membership in constants, such as `F.country.isIn("ES", "PT")`, kept sorted and without repeats. */
  def isIn(values: Constant*): Expr = isIn(values)

  /** Membership in a collection of constants (a `Seq`, a `Set`...). */
  def isIn(values: Iterable[Constant]): Expr =
    Expr(asJava.isIn(values.iterator.map(Expr.constant).toVector.asJava))

  /** A range: `low <= this <= high` with both ends included, or the ends `closed` says. */
  def between(low: Operand, high: Operand, closed: Closed = Closed.BOTH): Expr =
    Expr(asJava.between(Expr.operand(low), Expr.operand(high), closed))

  /** Whether the value is missing or null: never unknown. */
  def isNull: Expr = Expr(asJava.isNull())

  /** Whether the string matches a regular expression anywhere (the `regex` crate's syntax). */
  def matches(pattern: String): Expr = Expr(asJava.matches(pattern))

  /** Whether the string starts with `prefix`. */
  def startsWith(prefix: String): Expr = Expr(asJava.startsWith(prefix))

  /** Whether the string ends with `suffix`. */
  def endsWith(suffix: String): Expr = Expr(asJava.endsWith(suffix))

  /** Whether the string contains `part`. */
  def contains(part: String): Expr = Expr(asJava.contains(part))

  // ---- Evaluating one node.

  /**
   * Evaluate this condition on `data` (read as [[RuleSet.evaluate]] reads it)
   * and give its result tree (`spec/ast.md`, *Evaluating one node*).
   *
   * @param logic the logic, by name
   * @param strict whether a missing field or a type mismatch is an error instead of unknown
   */
  def evaluateTree(data: Any, logic: String = "boolean", strict: Boolean = false): ResultTree =
    ResultTree.fromJava(asJava.evaluateTree(ToJava.convert(data), logic, strict))

  /** Evaluate this value node on `data` and give its result tree. */
  def evaluateValueTree(data: Any, strict: Boolean = false): ResultTree =
    ResultTree.fromJava(asJava.evaluateValueTree(ToJava.convert(data), strict))

  // ---- Introspection (the rest is on `asJava`).

  /** The field paths this node reads, without repeats. */
  def fieldPaths: Seq[Seq[Segment]] = FromJava.paths(asJava.fieldPaths())

  /** The node in the JSON format (`spec/json-format.md`). */
  def toJson: String = asJava.toJson()

  /** Structural identity (`spec/ast.md`): the same shape, tags and constants. */
  override def equals(other: Any): Boolean = other match
    case node: Expr => asJava.equals(node.asJava)
    case _ => false

  override def hashCode: Int = asJava.hashCode

  /** The node's text in the rule syntax, such as `customer.age >= 18`. */
  override def toString: String = asJava.toString

object Expr:
  /** The Scala node of a node of the Java binding. */
  def fromJava(node: io.github.cleanrules.Expr): Expr = node match
    case field: io.github.cleanrules.Field => new Field(field)
    case constant: io.github.cleanrules.Const => new Const(constant)
    case other => new Composite(other)

  private[scaladsl] def apply(node: io.github.cleanrules.Expr): Expr = fromJava(node)

  /** What the Java binding takes for an operand: its node, or a Java constant. */
  private[scaladsl] def operand(value: Operand | Null): AnyRef = value match
    case node: Expr => node.asJava
    case other => constant(other)

  private[scaladsl] def constant(value: Any): AnyRef = value match
    case parameters: scala.reflect.Enum if parameters.productArity > 0 =>
      val message = s"the enum case $parameters has parameters, so it is not a constant"
      val hint = "only enum cases without parameters are constants (their name)"
      throw ExpressionException("expression_error", message, hint, Expr.errorJson(message, hint), null)
    case other => ToJava.convert(other)

  /** The JSON form of an `expression_error` this layer finds (`spec/errors.md`). */
  private def errorJson(message: String, hint: String): String =
    def quote(text: String): String =
      text.flatMap {
        case '"' => "\\\""
        case '\\' => "\\\\"
        case c if c < ' ' => f"\\u${c.toInt}%04x"
        case c => c.toString
      }
    s"""{"code":"expression_error","message":"${quote(message)}","hint":"${quote(hint)}"}"""

  /** A node that is neither a field nor a constant. */
  private final class Composite(node: io.github.cleanrules.Expr) extends Expr(node)

/**
 * A value looked up in the input by its path (`spec/ast.md`, `field`):
 * `F.customer.age`, `F.items(0).price`, or `F("customer", "age")`. Keys are
 * never split at dots.
 *
 * A key that is also a member of a field (an operator, `isIn`, `between`,
 * `isNull`, `matches`, `startsWith`, `endsWith`, `contains`, `abs`,
 * `evaluateTree`, `evaluateValueTree`, `fieldPaths`, `toJson`, `asJava`,
 * `apply`, a method of every Scala object such as `equals`, `eq`, `ne`,
 * `wait`, `notify`, `synchronized` or `toString`, or an extension method in
 * scope, such as `Predef`'s `->` or a library's `show` or `asJson`) is
 * reached with `apply`: `F.customer("contains")`. At the top,
 * only `apply` and the methods of every object clash: `F.contains` is the
 * field `contains`. A key that is not a Scala identifier takes backquotes,
 * `` F.`type` ``, or `apply`.
 *
 * @param asJava the field of the Java binding
 */
final class Field private[scaladsl] (override val asJava: io.github.cleanrules.Field) extends Expr(asJava), Dynamic:
  /** The field one key further down: `F.customer.age`. */
  def selectDynamic(key: String): Field = apply(key)

  /** The field a key and then segments further down: `F.items(0)`. */
  def applyDynamic(key: String)(segments: Segment*): Field = apply((key +: segments)*)

  /** The field further down by keys and indexes: `F.customer("and")`, `F.items(0, "price")`. */
  def apply(segments: Segment*): Field = new Field(asJava.at(segments.map(Field.segment)*))

private[scaladsl] object Field:
  def segment(segment: Segment): AnyRef = segment.asInstanceOf[AnyRef]

/**
 * The root of field paths: `F.customer.age`, `F.items(0).price`, or
 * `F("customer", "age")`. Only `apply` and the methods of every object clash
 * with a top-level key, which `F("apply")` reaches.
 */
object F extends Dynamic:
  /** The field of a top-level key: `F.age`. */
  def selectDynamic(key: String): Field = apply(key)

  /** The field of a top-level key and then segments: `F.items(0)`. */
  def applyDynamic(key: String)(segments: Segment*): Field = apply((key +: segments)*)

  /**
   * A field by its path of keys and indexes, outermost first.
   *
   * @throws io.github.cleanrules.ExpressionException for an empty path, an empty key or a negative index
   */
  def apply(segments: Segment*): Field =
    new Field(Conditions.field(segments.map(Field.segment)*))

/**
 * A constant (`spec/ast.md`, `const`), such as `Const(true)` where no node is
 * involved. Next to a node, Scala values become constants on their own.
 *
 * @param asJava the constant of the Java binding
 */
final class Const private[scaladsl] (override val asJava: io.github.cleanrules.Const) extends Expr(asJava):
  /** The constant as a Scala value (`spec/data-model.md`, *Values given back*): `None` for null. */
  def value: Any = FromJava(asJava.value())

object Const:
  /**
   * A constant of a Scala value.
   *
   * @throws io.github.cleanrules.ExpressionException for a NaN, an integer beyond 64 bits, a decimal
   *   beyond 38 digits, a datetime finer than a microsecond, or a value that is not a scalar
   */
  def apply(value: Constant | Null): Const = new Const(Conditions.constant(Expr.constant(value)))
