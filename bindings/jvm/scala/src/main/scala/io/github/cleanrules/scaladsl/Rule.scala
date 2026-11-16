package io.github.cleanrules.scaladsl

import io.github.cleanrules.scaladsl.internal.{FromJava, ToJava}
import scala.collection.immutable.{SeqMap, VectorMap}
import scala.jdk.CollectionConverters.*
import scala.jdk.OptionConverters.*

/**
 * A rule (`spec/rules.md`, *Rules*): a name, a condition, a payload, and a
 * priority, tags, a description, metadata, an action and an id.
 *
 * {{{
 * val vip = Rule(
 *   "vip",
 *   F.customer.years > 5 & F.order.total > 1000,
 *   payload = SeqMap("discount" -> BigDecimal("0.15"), "tier" -> "gold"),
 *   priority = 10,
 *   tags = Seq("loyalty"),
 *   actionName = Some("notify_sales"),
 *   id = Some("loyalty-discount"),
 * )
 * }}}
 *
 * Rules are immutable: each `with...` step gives a new rule, which the core
 * checks (an empty name, a value as the condition, a condition in the payload,
 * an empty id are an `io.github.cleanrules.RuleDefinitionException`). Payload values are
 * nodes or Scala constants. A payload is an association: whatever the map
 * and its order, its keys are kept sorted by Unicode code point. Rules
 * compare by identity (that of the Java rule), and may be shared between
 * threads.
 *
 * @param asJava the rule of the Java binding
 */
final class Rule private (val asJava: io.github.cleanrules.Rule):
  /** The name. */
  def name: String = asJava.name()

  /** The id given to the rule (`spec/rules.md`, *Identity*), if any. */
  def id: Option[String] = asJava.id().toScala

  /** The effective id: the id, or the name when the rule has none; unique within a rule set. */
  def effectiveId: String = asJava.effectiveId()

  /** The condition. */
  def when: Expr = Expr.fromJava(asJava.when())

  /** The payload: its keys, sorted by Unicode code point, and their nodes. */
  def payload: SeqMap[String, Expr] =
    VectorMap.from(asJava.`then`().asScala.iterator.map((key, node) => key -> Expr.fromJava(node)))

  /** The priority (0 unless set). */
  def priority: Long = asJava.priority()

  /** The tags, in order. */
  def tags: Seq[String] = asJava.tags().asScala.toVector

  /** The description, if any. */
  def description: Option[String] = asJava.description().toScala

  /** The metadata, as Scala values. */
  def metadata: SeqMap[String, Any] = FromJava.map(asJava.metadata())

  /** The action's name, if the rule names one. */
  def actionName: Option[String] = asJava.actionName().toScala

  /** The action's code, if bound. */
  def action: Option[Action] = asJava.action().toScala.map(Rule.scalaAction)

  // ---- Steps.

  /** This rule with one more payload entry (a node or a Scala constant). */
  def withPayload(key: String, value: Operand | Null): Rule = Rule(asJava.`then`(key, Expr.operand(value)))

  /** This rule with more payload entries, added by key, by Unicode code point. */
  def withPayload(payload: collection.Map[String, Operand | Null]): Rule =
    Rule(asJava.`then`(Rule.javaPayload(payload)))

  /** This rule with a priority: the higher, the earlier under the `priority` hit policy. */
  def withPriority(priority: Long): Rule = Rule(asJava.priority(priority))

  /** This rule with tags, replacing any. */
  def withTags(tags: String*): Rule = withTags(tags)

  /** This rule with the tags of a collection, replacing any. */
  def withTags(tags: Iterable[String]): Rule = Rule(asJava.tags(tags.toVector.asJava))

  /** This rule with a description. */
  def withDescription(description: String): Rule = Rule(asJava.description(description))

  /** This rule with metadata (Scala values, also lists and maps), replacing any. */
  def withMetadata(metadata: collection.Map[String, Any]): Rule = Rule(asJava.metadata(Rule.javaMap(metadata)))

  /**
   * This rule naming its action, which [[RuleSet.bind]] resolves. A named
   * action is data, written with the rule; the code it had, if any, is dropped.
   */
  def withActionName(name: String): Rule = Rule(asJava.action(name))

  /** This rule running `action` when it is selected by [[RuleSet.run]]; a name it has stays. */
  def withAction(action: Action): Rule = Rule(asJava.action(Rule.ScalaAction(action)))

  /**
   * This rule with an id (`spec/rules.md`, *Identity*): what names it as the
   * same rule across versions of a rule set, so that a renamed rule keeps its
   * identity. Without one, its name is its id. The id changes nothing in
   * evaluation: decisions, explanations and summaries name rules by name.
   *
   * @throws io.github.cleanrules.RuleDefinitionException for an empty id
   */
  def withId(id: String): Rule = Rule(asJava.id(id))

  /** Identity: that of the Java rule. */
  override def equals(other: Any): Boolean = other match
    case rule: Rule => asJava eq rule.asJava
    case _ => false

  override def hashCode: Int = System.identityHashCode(asJava)

  /** The rule's text, as the Java binding writes it. */
  override def toString: String = asJava.toString

object Rule:
  /**
   * A rule.
   *
   * @param name the rule's name
   * @param when its condition
   * @param payload what it gives when it fires: nodes or Scala constants
   * @param priority the higher, the earlier under the `priority` hit policy
   * @param tags labels, such as `loyalty`
   * @param description what the rule is for
   * @param metadata any Scala values, also lists and maps
   * @param actionName the name of its action, which [[RuleSet.bind]] resolves
   * @param action the code [[RuleSet.run]] runs when the rule is selected
   * @param id what names the rule across versions of a rule set (`spec/rules.md`, *Identity*);
   *   without one, its name is its id
   * @throws io.github.cleanrules.RuleDefinitionException for an empty name, a value as the
   *   condition, a condition in the payload or an empty id
   */
  def apply(
      name: String,
      when: Expr,
      payload: collection.Map[String, Operand | Null] = SeqMap.empty,
      priority: Long = 0,
      tags: Seq[String] = Nil,
      description: Option[String] = None,
      metadata: collection.Map[String, Any] = SeqMap.empty,
      actionName: Option[String] = None,
      action: Option[Action] = None,
      id: Option[String] = None,
  ): Rule =
    var rule = io.github.cleanrules.Rule.named(name).when(when.asJava)
    id.foreach(text => rule = rule.id(text))
    if payload.nonEmpty then rule = rule.`then`(javaPayload(payload))
    if priority != 0 then rule = rule.priority(priority)
    if tags.nonEmpty then rule = rule.tags(tags.asJava)
    description.foreach(text => rule = rule.description(text))
    if metadata.nonEmpty then rule = rule.metadata(javaMap(metadata))
    actionName.foreach(named => rule = rule.action(named))
    action.foreach(code => rule = rule.action(ScalaAction(code)))
    new Rule(rule)

  /** The Scala rule of a rule of the Java binding (read from a document, say). */
  def fromJava(rule: io.github.cleanrules.Rule): Rule = new Rule(rule)

  private def apply(rule: io.github.cleanrules.Rule): Rule = new Rule(rule)

  /** A Scala action as the Java binding runs it; the function comes back from [[Rule.action]]. */
  private[scaladsl] final class ScalaAction(val function: Action) extends io.github.cleanrules.Action:
    override def run(context: io.github.cleanrules.ActionContext): Unit = function(new ActionContext(context))

  private[scaladsl] def scalaAction(action: io.github.cleanrules.Action): Action = action match
    case bound: ScalaAction => bound.function
    case java => context => java.run(context.asJava)

  private[scaladsl] def javaActions(actions: collection.Map[String, Action]): Map[String, io.github.cleanrules.Action] =
    actions.iterator.map((name, code) => name -> (ScalaAction(code): io.github.cleanrules.Action)).toMap

  /** A payload for the Java binding, which adds its entries by key. */
  private def javaPayload(payload: collection.Map[String, Operand | Null]): java.util.Map[String, AnyRef] =
    val map = java.util.HashMap[String, AnyRef]()
    payload.foreach((key, value) => map.put(key, Expr.operand(value)))
    map

  private def javaMap(map: collection.Map[String, Any]): java.util.Map[String, ?] =
    ToJava.convert(map).asInstanceOf[java.util.Map[String, ?]]

/**
 * What an action receives: the rule, the data, its result, its output, the
 * result tree of its condition and the decision.
 *
 * @param asJava the context of the Java binding
 */
final class ActionContext private[scaladsl] (val asJava: io.github.cleanrules.ActionContext):
  /** The rule whose action runs. */
  def rule: Rule = Rule.fromJava(asJava.rule())

  /** The data the rule set ran on: the very object given to [[RuleSet.run]]. */
  def data: Any = FromJava.original(asJava.data())

  /** The rule's result. */
  def result: RuleResult = RuleResult.fromJava(asJava.result())

  /** The rule's output: its evaluated payload. */
  def output: SeqMap[String, Any] = FromJava.map(asJava.output())

  /** The result tree of the rule's condition. */
  def tree: ResultTree = ResultTree.fromJava(asJava.tree())

  /** The decision: equal to the one [[RuleSet.run]] gives back. */
  def decision: Decision = Decision.fromJava(asJava.decision())
