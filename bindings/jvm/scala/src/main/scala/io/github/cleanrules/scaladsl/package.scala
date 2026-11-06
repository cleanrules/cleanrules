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

import io.github.cleanrules.Documents
import scala.jdk.CollectionConverters.*

/**
 * A Scala value that can be a constant next to a node, such as the `18` of
 * `F.age >= 18`: a boolean, a number (`BigDecimal` stays exact), a string, a
 * date or a datetime of `java.time`, or an enum constant (Java's, or a Scala 3
 * case without parameters: its name).
 */
type Constant = Boolean | Byte | Short | Int | Long | Float | Double | String | BigInt | BigDecimal |
  java.math.BigInteger | java.math.BigDecimal | java.time.LocalDate | java.time.LocalDateTime |
  java.time.OffsetDateTime | java.time.ZonedDateTime | java.time.Instant | java.lang.Enum[?] | scala.reflect.Enum

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
