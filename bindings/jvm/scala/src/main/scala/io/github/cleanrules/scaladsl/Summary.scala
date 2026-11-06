package io.github.cleanrules.scaladsl

import io.github.cleanrules.{Aggregate, HitPolicy, Truth}
import io.github.cleanrules.scaladsl.internal.FromJava
import scala.collection.immutable.{SeqMap, VectorMap}
import scala.jdk.CollectionConverters.*
import scala.jdk.OptionConverters.*

/**
 * What a rule set reads and produces, without evaluating it
 * (`spec/explain.md`, *Summaries*); `toString` is its text, the same in every
 * language.
 *
 * @param hitPolicy the hit policy
 * @param aggregate the aggregate of `collect`, if any
 * @param logic the logic, by name
 * @param strict whether the rule set is strict
 * @param rules each rule's summary, in rule order
 * @param fields each field read, and where
 * @param outputs the payload keys, in order
 * @param tags each tag and its rules
 */
final case class Summary private (
    hitPolicy: HitPolicy,
    aggregate: Option[Aggregate],
    logic: String,
    strict: Boolean,
    rules: Seq[RuleSummary],
    fields: Seq[FieldUsage],
    outputs: Seq[String],
    tags: SeqMap[String, Seq[String]],
)(val asJava: io.github.cleanrules.Summary):
  /** The summary document of the JSON format. */
  def toJson: String = asJava.toJson()

  /** The summary as text (`spec/explain.md`, *Summaries*). */
  override def toString: String = asJava.toString

object Summary:
  /** The Scala summary of a summary of the Java binding. */
  def fromJava(summary: io.github.cleanrules.Summary): Summary =
    Summary(
      summary.hitPolicy(),
      summary.aggregate().toScala,
      summary.logic(),
      summary.isStrict(),
      summary.rules().asScala.iterator.map(RuleSummary.fromJava).toVector,
      summary.fields().asScala.iterator.map(FieldUsage.fromJava).toVector,
      summary.outputs().asScala.toVector,
      VectorMap.from(summary.tags().asScala.iterator.map((tag, rules) => tag -> rules.asScala.toVector)),
    )(summary)

/**
 * One rule in a summary.
 *
 * @param name the rule's name
 * @param priority its priority
 * @param outputs its payload keys
 * @param action whether it has an action (a name, or code)
 * @param actionName the action's name, if any
 */
final case class RuleSummary private (name: String, priority: Long, outputs: Seq[String], action: Boolean, actionName: Option[String])(
    val asJava: io.github.cleanrules.RuleSummary
):
  override def toString: String = asJava.toString

object RuleSummary:
  /** The Scala rule summary of a rule summary of the Java binding. */
  def fromJava(rule: io.github.cleanrules.RuleSummary): RuleSummary =
    RuleSummary(rule.name(), rule.priority(), rule.outputs().asScala.toVector, rule.action(), rule.actionName().toScala)(
      rule
    )

/**
 * A field in a summary: the conditions that read it and the payloads that give it.
 *
 * @param path the field's path
 * @param conditions the rules whose conditions read it
 * @param payloads the rules and payload keys that give it
 */
final case class FieldUsage private (path: Seq[Segment], conditions: Seq[String], payloads: Seq[FieldUsage.Payload])(
    val asJava: io.github.cleanrules.FieldUsage
):
  override def toString: String = asJava.toString

object FieldUsage:
  /** A payload entry that gives a field: the rule and the key. */
  final case class Payload private (rule: String, key: String)(val asJava: io.github.cleanrules.FieldUsage.Payload)

  object Payload:
    /** The Scala payload entry of one of the Java binding. */
    def fromJava(payload: io.github.cleanrules.FieldUsage.Payload): Payload =
      Payload(payload.rule(), payload.key())(payload)

  /** The Scala field usage of a field usage of the Java binding. */
  def fromJava(usage: io.github.cleanrules.FieldUsage): FieldUsage =
    FieldUsage(
      FromJava.path(usage.path()),
      usage.conditions().asScala.toVector,
      usage.payloads().asScala.iterator.map(Payload.fromJava).toVector,
    )(usage)

/**
 * The result of evaluating a node, with its children's (`spec/ast.md`,
 * *Evaluating one node*), from [[Expr.evaluateTree]] or [[ActionContext.tree]].
 *
 * @param node the node evaluated (the halves of a `between` are comparisons)
 * @param truth the truth value, for a node in a condition position
 * @param value the value, for a node in a value position: `None` for null and for a condition
 * @param reason why the truth value is unknown or the value null, if it is
 * @param children the children's results, in order
 */
final case class ResultTree private (
    node: Expr,
    truth: Option[Truth],
    value: Any,
    reason: Option[Reason],
    children: Seq[ResultTree],
)(val asJava: io.github.cleanrules.ResultTree):
  /** This result and its children in the JSON format. */
  def toJson: String = asJava.toJson()

  /** The node and its outcome, such as `ResultTree(age >= 18: true)`. */
  override def toString: String = asJava.toString

object ResultTree:
  /** The Scala result tree of a result tree of the Java binding. */
  def fromJava(tree: io.github.cleanrules.ResultTree): ResultTree =
    ResultTree(
      Expr.fromJava(tree.node()),
      tree.truth().toScala,
      FromJava(tree.value()),
      tree.reason().toScala.map(Reason.fromJava),
      tree.children().asScala.iterator.map(fromJava).toVector,
    )(tree)
