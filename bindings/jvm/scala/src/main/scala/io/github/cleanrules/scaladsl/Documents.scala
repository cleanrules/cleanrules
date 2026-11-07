package io.github.cleanrules.scaladsl

import io.github.cleanrules.{Documents, Format, Limits}
import java.nio.file.Path
import scala.jdk.CollectionConverters.*

/**
 * What a document holds (`spec/json-format.md`, *Documents*): a condition
 * ([[Expr]]), a [[Rule]], a [[RuleSet]] or a [[DecisionTable]]. [[loads]] and
 * [[load]] give one, to take apart with an exhaustive `match`:
 *
 * {{{
 * load(Path.of("pricing.yaml")) match
 *   case rules: RuleSet => rules.evaluate(order)
 *   case table: DecisionTable => table.evaluate(order)
 *   case rule: Rule => RuleSet(rule).evaluate(order)
 *   case condition: Expr => condition.evaluateTree(order)
 * }}}
 *
 * and [[dumps]] and [[dump]] write one.
 */
type DocumentContent = Expr | Rule | RuleSet | DecisionTable

object DocumentContent:
  /** The Scala content of a document read by the Java binding. */
  def fromJava(content: io.github.cleanrules.DocumentContent): DocumentContent = content match
    case node: io.github.cleanrules.Expr => Expr.fromJava(node)
    case rule: io.github.cleanrules.Rule => Rule.fromJava(rule)
    case rules: io.github.cleanrules.RuleSet => RuleSet.fromJava(rules)
    case table: io.github.cleanrules.DecisionTable => DecisionTable.fromJava(table)

  /**
   * The Java binding's content of a document: the `asJava` of the node, rule,
   * rule set or table (a function, not an extension: an extension named
   * `asJava` would compete with `scala.jdk.CollectionConverters`).
   */
  def asJava(content: DocumentContent): io.github.cleanrules.DocumentContent = content match
    case node: Expr => node.asJava
    case rule: Rule => rule.asJava
    case rules: RuleSet => rules.asJava
    case table: DecisionTable => table.asJava

// ---- Reading.

/**
 * Read a document (`spec/json-format.md`): a condition, a rule, a rule set or
 * a decision table, in JSON, YAML, the rule syntax (`spec/text.md`) or CSV (a
 * decision table, `spec/tables.md`). The core reads it strictly and within
 * the limits. A rule's action is the name the document gives it, never code:
 * `actions` binds those names to Scala functions ([[RuleSet.bind]]); empty, the
 * default, leaves them unbound.
 *
 * {{{
 * loads(yaml, Format.YAML, actions = Map("notify_sales" -> notify)) match
 *   case rules: RuleSet => rules.run(order)
 *   case other => sys.error(s"not a rule set: $other")
 * }}}
 *
 * @param text the document, as a string or as UTF-8 bytes
 * @param format its format, JSON by default
 * @param actions each action's name and its code
 * @param limits the limits of reading it (`Limits.defaults()` by default)
 * @throws io.github.cleanrules.FormatException for an invalid document, with a JSON Pointer, or a
 *   document beyond the limits (`limit_exceeded`)
 * @throws io.github.cleanrules.RuleSyntaxException for text that does not follow the syntax, with
 *   its line and column
 * @throws io.github.cleanrules.ExpressionException if a condition breaks a construction rule
 * @throws io.github.cleanrules.RuleDefinitionException if a rule, rule set or table breaks one
 * @throws io.github.cleanrules.RegistryException for an unknown logic, or a named action that
 *   `actions` lacks
 */
def loads(
    text: String | Array[Byte],
    format: Format = Format.JSON,
    actions: collection.Map[String, Action] = Map.empty,
    limits: Limits = Limits.defaults(),
): DocumentContent =
  DocumentContent.fromJava(text match
    case string: String => Documents.loads(string, format, limits, registry(actions))
    case bytes: Array[Byte] => Documents.loads(bytes, format, limits, registry(actions)))

/**
 * Read a document from a file, its format given by its extension (`.json`,
 * `.yaml`, `.yml`, `.rules`, `.csv`) unless `format` gives it. A file larger
 * than the size limit of its format is not read (`limit_exceeded`).
 *
 * @param file the file
 * @param format its format, if its extension does not tell it
 * @param actions each action's name and its code, as for [[loads]]
 * @param limits the limits of reading it
 * @throws IllegalArgumentException if no format is given and the extension does not tell it
 * @throws java.io.UncheckedIOException if the file cannot be read
 * @throws io.github.cleanrules.CleanRulesException as [[loads]]
 */
def load(
    file: Path,
    format: Option[Format] = None,
    actions: collection.Map[String, Action] = Map.empty,
    limits: Limits = Limits.defaults(),
): DocumentContent =
  DocumentContent.fromJava(Documents.load(file, format.orNull, limits, registry(actions)))

/**
 * A rule written in the rule syntax (`spec/text.md`), as fuzzytool's
 * `rule_from_text` reads it, such as
 * `RULE vip IF customer.years > 5 THEN discount = 0.15`; without a `RULE` line
 * it is named `rule_1`. A rule naming its action (`DO notify`) is bound to
 * `actions`; to give it code without a name, use [[Rule.withAction]]. To read
 * a condition or a rule set written as text, use [[loads]] with `Format.TEXT`.
 *
 * @param text the rule's text
 * @param actions the actions the rule's `DO` may name; empty leaves a named action unbound
 * @param limits the limits of reading it (its size, `maxTextSize`, and its depth, `maxDepth`)
 * @throws io.github.cleanrules.RuleSyntaxException for text that does not follow the syntax, or
 *   holds a condition or a rule set instead of one rule
 * @throws io.github.cleanrules.FormatException for a text beyond the limits (`limit_exceeded`)
 * @throws io.github.cleanrules.RegistryException if the rule names an action that `actions` lacks
 */
def ruleFromText(
    text: String,
    actions: collection.Map[String, Action] = Map.empty,
    limits: Limits = Limits.defaults(),
): Rule =
  Rule.fromJava(Documents.ruleFromText(text, limits, registry(actions)))

/** A registry for the Java binding, or null (leave names unbound) for none. */
private def registry(actions: collection.Map[String, Action]): java.util.Map[String, io.github.cleanrules.Action] | Null =
  if actions.isEmpty then null else Rule.javaActions(actions).asJava

// ---- Writing.

/**
 * Write a condition, rule, rule set or decision table in canonical form.
 *
 * @param value what to write
 * @param format JSON (compact, the default), YAML (block style), TEXT (the
 *   canonical text, `IF ... THEN ...`) or CSV (decision tables only)
 * @param omitActions write rules whose action is code without a name without
 *   it, instead of refusing them: a document holds an action's name, never code
 * @throws io.github.cleanrules.UnsupportedException for a rule whose action is code without a name
 *   (at `/rule/do` or `/ruleset/rules/<i>/do`), CSV for anything but a decision table, and, in
 *   text, a rule with metadata or a decision table
 * @throws io.github.cleanrules.ExpressionException if `value` is a value node, not a condition
 */
def dumps(value: DocumentContent, format: Format = Format.JSON, omitActions: Boolean = false): String =
  Documents.dumps(DocumentContent.asJava(value), format, omitActions)

/**
 * Write a condition, rule, rule set or decision table to a file, in the format
 * of its extension unless `format` gives it. A rule text ends with a line break.
 *
 * @param value what to write
 * @param file the file
 * @param format its format, if its extension does not tell it
 * @param omitActions as for [[dumps]]
 * @throws IllegalArgumentException if no format is given and the extension does not tell it
 * @throws java.io.UncheckedIOException if the file cannot be written
 * @throws io.github.cleanrules.CleanRulesException as [[dumps]]
 */
def dump(value: DocumentContent, file: Path, format: Option[Format] = None, omitActions: Boolean = false): Unit =
  Documents.dump(DocumentContent.asJava(value), file, format.orNull, omitActions)
