package io.github.cleanrules.scaladsl

import io.github.cleanrules.{GroupKind, NoteKind, Status, Truth}
import io.github.cleanrules.scaladsl.internal.FromJava
import scala.jdk.CollectionConverters.*
import scala.jdk.OptionConverters.*

/**
 * The explanation of a [[Decision]] (`spec/explain.md`): each rule's status and
 * what it needed, built by the core. Case classes, for pattern matching; each
 * keeps the Java binding's object in `asJava`, and `toString` is the text, the
 * same in every language.
 *
 * {{{
 * decision.whyNot("vip").requirement match
 *   case Cause(node, Truth.FALSE, _, _, _, _, Some(needed)) => s"$node: short by ${needed.gap}"
 *   case CauseGroup(GroupKind.ALL, items) => s"${items.size} conditions"
 *   case other => other.toString
 * }}}
 *
 * @param selected the names of the selected rules, in rule order
 * @param rules each rule's explanation, in rule order
 */
final case class Explanation private (selected: Seq[String], rules: Seq[RuleExplanation])(
    val asJava: io.github.cleanrules.Explanation
):
  /**
   * One rule's explanation.
   *
   * @throws io.github.cleanrules.RuleNotFoundException if no rule has that name
   */
  def forRule(name: String): RuleExplanation = rules(asJava.rules().indexOf(asJava.forRule(name)))

  /** The explanation document of the JSON format. */
  def toJson: String = asJava.toJson()

  /** The explanation as text (`spec/explain.md`, *Text*). */
  override def toString: String = asJava.toString

object Explanation:
  /** The Scala explanation of an explanation of the Java binding. */
  def fromJava(explanation: io.github.cleanrules.Explanation): Explanation =
    Explanation(
      explanation.selected().asScala.toVector,
      explanation.rules().asScala.iterator.map(RuleExplanation.fromJava).toVector,
    )(explanation)

/**
 * One rule's explanation.
 *
 * @param rule the rule's name
 * @param status selected, fired, not fired or unknown
 * @param requirement what the rule's condition needed: a cause, or a group of them
 * @param note why a rule that fired was not selected, if it was not
 * @param achievable whether the condition can hold by changing the fields it reads, when the core can tell
 */
final case class RuleExplanation private (
    rule: String,
    status: Status,
    requirement: Requirement,
    note: Option[Note],
    achievable: Option[Boolean],
)(val asJava: io.github.cleanrules.RuleExplanation):
  /** The causes of the requirement, in order, flattened. */
  def causes: Seq[Cause] = asJava.causes().asScala.iterator.map(Cause.fromJava).toVector

  /** The rule's lines of the explanation. */
  override def toString: String = asJava.toString

object RuleExplanation:
  /** The Scala explanation of a rule's explanation of the Java binding. */
  def fromJava(explanation: io.github.cleanrules.RuleExplanation): RuleExplanation =
    RuleExplanation(
      explanation.rule(),
      explanation.status(),
      Requirement.fromJava(explanation.requirement()),
      explanation.note().toScala.map(Note.fromJava),
      explanation.achievable().toScala.map(_.booleanValue),
    )(explanation)

/** What a rule's condition needed: a [[Cause]], or a [[CauseGroup]] of them. */
sealed trait Requirement

object Requirement:
  /** The Scala requirement of a requirement of the Java binding. */
  def fromJava(requirement: io.github.cleanrules.Requirement): Requirement = requirement match
    case cause: io.github.cleanrules.Cause => Cause.fromJava(cause)
    case group: io.github.cleanrules.CauseGroup => CauseGroup.fromJava(group)

/**
 * A condition in a rule's requirement, with what was read (`spec/explain.md`,
 * *Causes*); `toString` is its line, such as `x > 10 is false (x = 8); short by 2`.
 *
 * @param node the condition (a `between` half is the comparison it is)
 * @param truth its truth value
 * @param negated whether it appears under a NOT
 * @param holds whether it holds as the rule needs it
 * @param values the fields it read, and their values
 * @param reason why the truth value is unknown, if it is
 * @param counterfactual what would make it hold, when the core can tell
 */
final case class Cause private (
    node: Expr,
    truth: Truth,
    negated: Boolean,
    holds: Boolean,
    values: Seq[FieldValue],
    reason: Option[Reason],
    counterfactual: Option[Counterfactual],
)(val asJava: io.github.cleanrules.Cause)
    extends Requirement:
  override def toString: String = asJava.toString

object Cause:
  /** The Scala cause of a cause of the Java binding. */
  def fromJava(cause: io.github.cleanrules.Cause): Cause =
    Cause(
      Expr.fromJava(cause.node()),
      cause.truth(),
      cause.negated(),
      cause.holds(),
      cause.values().asScala.iterator.map(FieldValue.fromJava).toVector,
      cause.reason().toScala.map(Reason.fromJava),
      cause.counterfactual().toScala.map(Counterfactual.fromJava),
    )(cause)

/**
 * A field a cause read, and its value (`None` for null).
 *
 * @param path the field's path
 * @param value its value, as Scala gives values back
 */
final case class FieldValue private (path: Seq[Segment], value: Any)(val asJava: io.github.cleanrules.Cause.FieldValue)

object FieldValue:
  /** The Scala field value of a field value of the Java binding. */
  def fromJava(value: io.github.cleanrules.Cause.FieldValue): FieldValue =
    FieldValue(FromJava.path(value.path()), FromJava(value.value()))(value)

/**
 * Requirements that must all hold (`ALL`), or of which one must (`ANY`).
 *
 * @param kind all or any
 * @param items the requirements
 */
final case class CauseGroup private (kind: GroupKind, items: Seq[Requirement])(val asJava: io.github.cleanrules.CauseGroup)
    extends Requirement:
  override def toString: String = asJava.toString

object CauseGroup:
  /** The Scala group of a group of the Java binding. */
  def fromJava(group: io.github.cleanrules.CauseGroup): CauseGroup =
    CauseGroup(group.kind(), group.items().asScala.iterator.map(Requirement.fromJava).toVector)(group)

/**
 * What would make a comparison hold: the field, the operator, the target and
 * the gap (`spec/explain.md`, *Counterfactuals*).
 *
 * @param path the field's path
 * @param op the comparison's operator, such as `gt`
 * @param target the value the field should reach
 * @param gap how far it is: a `Long`, `Double` or `BigDecimal`, a `java.time.Period` of days between dates,
 *   a `java.time.Duration` between datetimes; `None` when there is no distance
 */
final case class Counterfactual private (path: Seq[Segment], op: String, target: Any, gap: Option[Any])(
    val asJava: io.github.cleanrules.Counterfactual
):
  override def toString: String = asJava.toString

object Counterfactual:
  /** The Scala counterfactual of a counterfactual of the Java binding. */
  def fromJava(counterfactual: io.github.cleanrules.Counterfactual): Counterfactual =
    Counterfactual(
      FromJava.path(counterfactual.path()),
      counterfactual.op(),
      FromJava(counterfactual.target()),
      counterfactual.gap().toScala.map(FromJava(_)),
    )(counterfactual)

/**
 * Why a rule that fired was not selected (`spec/explain.md`, *Notes*).
 *
 * @param kind the reason
 * @param winner the rule selected instead
 * @param priority this rule's priority, where the hit policy compares them
 * @param winnerPriority the winner's priority, where the hit policy compares them
 */
final case class Note private (kind: NoteKind, winner: String, priority: Option[Long], winnerPriority: Option[Long])(
    val asJava: io.github.cleanrules.Note
):
  override def toString: String = asJava.toString

object Note:
  /** The Scala note of a note of the Java binding. */
  def fromJava(note: io.github.cleanrules.Note): Note =
    Note(note.kind(), note.winner(), FromJava.optionLong(note.priority()), FromJava.optionLong(note.winnerPriority()))(
      note
    )

/**
 * Why a truth value is unknown or a value null (`spec/explain.md`, *Reasons*).
 *
 * @param code the reason's code, such as `missing_field`
 * @param paths the fields involved
 * @param types the types involved, for a type mismatch
 * @param detail more, such as a read error's message
 */
final case class Reason private (code: String, paths: Seq[Seq[Segment]], types: Seq[String], detail: Option[String])(
    val asJava: io.github.cleanrules.Reason
):
  override def toString: String = asJava.toString

object Reason:
  /** The Scala reason of a reason of the Java binding. */
  def fromJava(reason: io.github.cleanrules.Reason): Reason =
    Reason(reason.code(), FromJava.paths(reason.paths()), reason.types().asScala.toVector, reason.detail().toScala)(
      reason
    )
