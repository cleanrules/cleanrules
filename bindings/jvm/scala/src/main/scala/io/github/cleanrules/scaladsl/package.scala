/**
 * cleanrules for Scala 3: an idiomatic layer over the Java binding
 * (`io.github.cleanrules`), with fuzzytool's operators.
 *
 * {{{
 * import io.github.cleanrules.scaladsl.*
 *
 * case class Customer(years: Int, country: String)
 * case class Order(customer: Customer, total: BigDecimal)
 *
 * val vip = Rule("vip", F.customer.years > 5 & F.total > 1000, payload = Map("discount" -> BigDecimal("0.15")))
 * val decision = RuleSet(vip).evaluate(Order(Customer(7, "ES"), BigDecimal(1200)))
 * decision.output        // Some(VectorMap(discount -> 0.15))
 * decision.whyNot("vip") // the rule's explanation, as case classes
 * }}}
 *
 * The core builds, checks, evaluates and explains; this layer delegates to
 * the Java binding's idiomatic API and converts Scala values. Errors are the
 * Java binding's exceptions (`io.github.cleanrules.CleanRulesException` and
 * its subclasses), raised as they are.
 */
package io.github.cleanrules.scaladsl

import io.github.cleanrules.{ActionException, CleanRulesException, Documents}
import scala.jdk.CollectionConverters.*
import scala.jdk.OptionConverters.*

/**
 * A Scala value that can be a constant next to a node, such as the `18` of
 * `F.age >= 18`: a boolean, a number (`BigDecimal` stays exact), a string, a
 * date or a datetime of `java.time`, an enum constant (Java's, or a Scala 3
 * case without parameters: its name), or `None`, the null constant (which only
 * a payload or `Const` takes: the core refuses it in a comparison).
 */
type Constant = Boolean | Byte | Short | Int | Long | Float | Double | String | BigInt | BigDecimal |
  java.math.BigInteger | java.math.BigDecimal | java.time.LocalDate | java.time.LocalDateTime |
  java.time.OffsetDateTime | java.time.ZonedDateTime | java.time.Instant | java.lang.Enum[?] | scala.reflect.Enum |
  None.type

/**
 * A number, a date or a datetime that can stand on the left of an operator,
 * such as `1000 < F.total` (strings cannot: `"a" + F.x` is Scala's string
 * concatenation).
 */
type LeftConstant = Int | Long | Double | BigInt | BigDecimal | java.math.BigDecimal | java.time.LocalDate |
  java.time.LocalDateTime | java.time.OffsetDateTime | java.time.ZonedDateTime | java.time.Instant

/**
 * Comparisons and arithmetic with a constant on the left: `1000 < F.total` is
 * `Const(1000) < F.total`. A Scala operator of the constant's own type (`1 < 2`)
 * still wins when its argument fits.
 */
extension (value: LeftConstant)
  def <(node: Expr): Expr = Const(value) < node
  def <=(node: Expr): Expr = Const(value) <= node
  def >(node: Expr): Expr = Const(value) > node
  def >=(node: Expr): Expr = Const(value) >= node
  def ===(node: Expr): Expr = Const(value) === node
  def =!=(node: Expr): Expr = Const(value) =!= node
  def +(node: Expr): Expr = Const(value) + node
  def -(node: Expr): Expr = Const(value) - node
  def *(node: Expr): Expr = Const(value) * node
  def /(node: Expr): Expr = Const(value) / node

/** Scala views of an error's context (the Java accessors give `Optional`s). */
extension (error: CleanRulesException)
  /** The rule involved, if any. */
  def ruleOption: Option[String] = error.rule().toScala

  /** The field path involved, if any. */
  def pathOption: Option[Seq[Segment]] = error.path().toScala.map(internal.FromJava.path)

  /** The JSON pointer of a document error, if any. */
  def pointerOption: Option[String] = error.pointer().toScala

  /** The line of a text error, from 1, if any. */
  def lineOption: Option[Int] = error.line().toScala

  /** The column of a text error, from 1, if any. */
  def columnOption: Option[Int] = error.column().toScala

  /** The row of a batch error, if any. */
  def rowOption: Option[Int] = error.row().toScala

/** The decision of an `ActionException`, as a Scala decision. */
extension (error: ActionException)
  def scalaDecision: Option[Decision] = error.decision().toScala.map(Decision.fromJava)

/** What an operator takes: a node, or a Scala constant. */
type Operand = Expr | Constant

/** A segment of a field's path: a key, or an index from 0. */
type Segment = String | Int | Long

/** An action: code that a selected rule runs, given its context (`spec/rules.md`, *Actions*). */
type Action = ActionContext => Unit

/**
 * A rule parsed from the rule syntax (`spec/text.md`), such as
 * `RULE vip IF customer.years > 5 THEN discount = 0.15`.
 *
 * @param text the rule's text
 * @param actions the actions the rule's `DO` may name
 * @throws io.github.cleanrules.RuleSyntaxException for text that does not parse
 */
def ruleFromText(text: String, actions: Map[String, Action] = Map.empty): Rule =
  Rule.fromJava(
    if actions.isEmpty then Documents.ruleFromText(text)
    else Documents.ruleFromText(text, Rule.javaActions(actions).asJava)
  )
