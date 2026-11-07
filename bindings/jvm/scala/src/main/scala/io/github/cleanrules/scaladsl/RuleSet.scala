package io.github.cleanrules.scaladsl

import io.github.cleanrules.{Aggregate, Format, HitPolicy, Limits}
import io.github.cleanrules.scaladsl.internal.{FromJava, ToJava}
import scala.jdk.CollectionConverters.*
import scala.jdk.OptionConverters.*

/**
 * Rules evaluated together under a hit policy (`spec/rules.md`).
 *
 * {{{
 * val rules = RuleSet(Seq(blocked, vip, loyal), hitPolicy = HitPolicy.PRIORITY)
 * val decision = rules.evaluate(order)  // a case class, a Map, a Java record...
 * }}}
 *
 * Data is read as `spec/data-model.md` (*Java and Scala*) says, and only the
 * paths the rules read: case classes by field name, `Option` as its value (or
 * null), Scala sequences and maps, `BigDecimal` and `BigInt`, `java.time`, and
 * Java values as the Java binding reads them. Rule sets are immutable and may
 * be shared between threads.
 *
 * @param asJava the rule set of the Java binding
 */
final class RuleSet private (val asJava: io.github.cleanrules.RuleSet):
  /** The rules, in order. */
  def rules: Seq[Rule] = asJava.rules().asScala.iterator.map(Rule.fromJava).toVector

  /** The number of rules. */
  def size: Int = asJava.size()

  /** The rule named `name`, if any. */
  def get(name: String): Option[Rule] = asJava.get(name).toScala.map(Rule.fromJava)

  /**
   * The rule named `name`.
   *
   * @throws io.github.cleanrules.RuleNotFoundException if there is none, with the names there are
   */
  def apply(name: String): Rule = Rule.fromJava(asJava.rule(name))

  /** Whether a rule is named `name`. */
  def contains(name: String): Boolean = asJava.contains(name)

  /** The hit policy. */
  def hitPolicy: HitPolicy = asJava.hitPolicy()

  /** The aggregate of the `collect` hit policy, if any. */
  def aggregate: Option[Aggregate] = asJava.aggregate().toScala

  /** The logic, by name. */
  def logic: String = asJava.logic()

  /** Whether a missing field or a type mismatch is an error instead of unknown. */
  def strict: Boolean = asJava.isStrict()

  /** The field paths the rules read, in their conditions and payloads, without repeats. */
  def fieldPaths: Seq[Seq[Segment]] = FromJava.paths(asJava.fieldPaths())

  // ---- Evaluating.

  /**
   * Evaluate the rules on `data`.
   *
   * @throws io.github.cleanrules.EvaluationException in strict mode, for a missing field or a type mismatch
   */
  def evaluate(data: Any): Decision = Decision.fromJava(asJava.evaluate(ToJava.convert(data)))

  /** Evaluate the rules on `data` within `limits`. */
  def evaluate(data: Any, limits: Limits): Decision =
    Decision.fromJava(asJava.evaluate(ToJava.convert(data), limits))

  /** Evaluate the rules on each row: one decision per row, in order. */
  def evaluateMany(rows: Iterable[Any]): Seq[Decision] =
    asJava.evaluateMany(javaRows(rows)).asScala.iterator.map(Decision.fromJava).toVector

  /** Evaluate the rules on each row within `limits`. */
  def evaluateMany(rows: Iterable[Any], limits: Limits): Seq[Decision] =
    asJava.evaluateMany(javaRows(rows), limits).asScala.iterator.map(Decision.fromJava).toVector

  private def javaRows(rows: Iterable[Any]): java.util.List[AnyRef] =
    val convert = ToJava()
    rows.iterator.map(convert(_)).toVector.asJava

  /**
   * A decision this rule set made, recorded as JSON or YAML ([[Decision.toJson]]).
   *
   * @param limits the limits of reading it (`maxSize`, `maxYamlSize`, `maxNesting`, `maxDepth`)
   * @throws io.github.cleanrules.FormatException if the document is invalid, beyond the limits
   *   (`limit_exceeded`) or not of this rule set
   */
  def readDecision(text: String, format: Format = Format.JSON, limits: Limits = Limits.defaults()): Decision =
    Decision.fromJava(asJava.readDecision(text, format, limits))

  // ---- Actions.

  /**
   * This rule set with its named actions bound to code: the only place names are looked up.
   *
   * @throws io.github.cleanrules.RegistryException for a named action the registry lacks
   */
  def bind(actions: collection.Map[String, Action]): RuleSet =
    new RuleSet(asJava.bind(Rule.javaActions(actions).asJava))

  /**
   * Evaluate the rules on `data`, then run the actions of the selected rules in order.
   *
   * @throws io.github.cleanrules.ActionException if an action throws, with what it threw as the
   *   cause, the rules whose actions ran and the decision
   */
  def run(data: Any): Decision = Decision.fromJava(asJava.run(ToJava.convert(data)))

  /** [[run]] within `limits`. */
  def run(data: Any, limits: Limits): Decision = Decision.fromJava(asJava.run(ToJava.convert(data), limits))

  // ---- Describing.

  /** What the rules read and produce, without evaluating them (`spec/explain.md`, *Summaries*). */
  def summary: Summary = Summary.fromJava(asJava.summary())

  /** Identity: that of the Java rule set. */
  override def equals(other: Any): Boolean = other match
    case rules: RuleSet => asJava eq rules.asJava
    case _ => false

  override def hashCode: Int = System.identityHashCode(asJava)

  override def toString: String = asJava.toString

object RuleSet:
  /** The rules under the `first` hit policy and Boolean logic, not strict. */
  def apply(rules: Rule*): RuleSet = apply(rules)

  /**
   * Rules under a hit policy, an aggregate (for `collect`), a logic, strict or not.
   *
   * @throws io.github.cleanrules.RuleDefinitionException for two rules with one name
   * @throws io.github.cleanrules.HitPolicyException for an aggregate without `collect`
   * @throws io.github.cleanrules.RegistryException for an unknown logic
   */
  def apply(
      rules: Iterable[Rule],
      hitPolicy: HitPolicy = HitPolicy.FIRST,
      aggregate: Option[Aggregate] = None,
      logic: String = "boolean",
      strict: Boolean = false,
  ): RuleSet =
    new RuleSet(
      io.github.cleanrules.RuleSet
        .builder()
        .add(rules.iterator.map(_.asJava).toVector.asJava)
        .hitPolicy(hitPolicy)
        .aggregate(aggregate.orNull)
        .logic(logic)
        .strict(strict)
        .build()
    )

  /** The Scala rule set of a rule set of the Java binding (read from a document, say). */
  def fromJava(rules: io.github.cleanrules.RuleSet): RuleSet = new RuleSet(rules)
