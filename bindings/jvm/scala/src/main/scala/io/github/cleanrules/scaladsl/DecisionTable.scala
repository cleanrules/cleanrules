package io.github.cleanrules.scaladsl

import io.github.cleanrules.{Aggregate, Limits, TablePolicy}
import io.github.cleanrules.scaladsl.internal.{FromJava, ToJava}
import scala.jdk.CollectionConverters.*
import scala.jdk.OptionConverters.*

/**
 * A decision table and the rule set it compiles into (`spec/tables.md`): a
 * grid of inputs, outputs and rows.
 *
 * {{{
 * val table = DecisionTable(
 *   Seq("RULE", F.customer.years, "order.total", TableOutput("risk", "high", "low")),
 *   Seq(
 *     Seq("new", F.customer.years < 1, "> 1000", "'high'"),
 *     Seq("vip", "> 5", None, "'low'"),
 *   ),
 *   hitPolicy = TablePolicy.PRIORITY,
 * )
 * val decision = table.evaluate(order)
 * }}}
 *
 * Each header item ([[DecisionTable.HeaderItem]]) is the text of a CSV header
 * field (`"RULE"`, `"PRIORITY"`, `"DESCRIPTION"`, `"THEN key"`,
 * `"THEN key IN (...)"` or an input expression), an input [[Expr]], or a
 * [[TableOutput]]. A `String` cell is read as in the table's CSV, in the rule
 * syntax: `"> 5"`, `"-"`, and `"'VIP'"` for the string, since `"VIP"` is the
 * field named `VIP`. An [[Expr]] cell is an input's condition, which tests its
 * column (`F.age < 18` under `age`), or an output's value; `None` is an empty
 * cell and `Some(x)` is `x`; any other cell is a Scala constant (under an
 * input `x`, the test `x = value`; under an output, the value). Rows may be
 * shorter than the header.
 *
 * Or read one with [[load]] or [[loads]] (CSV, JSON or YAML), and write it
 * with [[dumps]]. The core reads, checks and compiles every table, so a table
 * means the same in every language; its errors point into the grid:
 * `/header/<c>` or `/rows/<r>/<c>`, counting from 0. Tables are immutable and
 * may be shared between threads; they compare by identity (that of the Java
 * table).
 *
 * @param asJava the table of the Java binding
 */
final class DecisionTable private (val asJava: io.github.cleanrules.DecisionTable):
  /** The hit policy. */
  def hitPolicy: TablePolicy = asJava.hitPolicy()

  /** The aggregate of the `collect` hit policy, if any. */
  def aggregate: Option[Aggregate] = asJava.aggregate().toScala

  /** The logic, by name. */
  def logic: String = asJava.logic()

  /** Whether a missing field or a type mismatch is an error instead of unknown. */
  def strict: Boolean = asJava.isStrict()

  /** The input columns: the value each one tests, such as `customer.years`. */
  def inputs: Seq[Expr] = asJava.inputs().asScala.iterator.map(Expr.fromJava).toVector

  /** The output columns: their payload keys and output values. */
  def outputs: Seq[TableOutput] = asJava.outputs().asScala.iterator.map(TableOutput.fromJava).toVector

  /** The rows, as the core read them: one rule each. */
  def rows: Seq[TableRow] = asJava.rows().asScala.iterator.map(TableRow.fromJava).toVector

  /** The rule set the table compiles into: one rule per row (`spec/tables.md`, *Compiling*). */
  def ruleSet: RuleSet = RuleSet.fromJava(asJava.ruleSet())

  // ---- Evaluating.

  /**
   * Evaluate the compiled rule set on `data`, read as [[RuleSet.evaluate]] reads it.
   *
   * @throws io.github.cleanrules.EvaluationException in strict mode, for a missing field or a type mismatch
   * @throws io.github.cleanrules.HitPolicyException when the hit policy cannot choose (`unique`, `any`)
   */
  def evaluate(data: Any): Decision = ruleSet.evaluate(data)

  /** Evaluate the compiled rule set on `data` within `limits`. */
  def evaluate(data: Any, limits: Limits): Decision = ruleSet.evaluate(data, limits)

  /** Evaluate the compiled rule set on each row: one decision per row, in order. */
  def evaluateMany(rows: Iterable[Any]): Seq[Decision] = ruleSet.evaluateMany(rows)

  /** Evaluate the compiled rule set on each row within `limits`. */
  def evaluateMany(rows: Iterable[Any], limits: Limits): Seq[Decision] = ruleSet.evaluateMany(rows, limits)

  /** What the compiled rule set reads and produces (`spec/explain.md`, *Summaries*). */
  def summary: Summary = Summary.fromJava(asJava.summary())

  /** The decision table document of the JSON format (`spec/tables.md`, *JSON*), as [[dumps]] writes it. */
  def toJson: String = asJava.toJson()

  /** Identity: that of the Java table. */
  override def equals(other: Any): Boolean = other match
    case table: DecisionTable => asJava eq table.asJava
    case _ => false

  override def hashCode: Int = System.identityHashCode(asJava)

  /** The hit policy and the size, such as `DecisionTable(first: 2 rows, 3 columns)`. */
  override def toString: String = asJava.toString

object DecisionTable:
  /** A header item: the text of a CSV header field, an input node, or an output column. */
  type HeaderItem = String | Expr | TableOutput

  /**
   * A cell: text in the rule syntax (`"> 5"`, `"-"`, `"'VIP'"`), a node, a
   * Scala constant, `None` (empty) or `Some` of one of them.
   */
  type Cell = Operand | Option[Operand]

  /**
   * A table of a header and rows, read, checked and compiled by the core one
   * row at a time.
   *
   * @param header the columns: texts, input nodes and output columns
   * @param rows the rows, each its cells in column order
   * @param hitPolicy the hit policy, `first` by default
   * @param aggregate how the outputs of `collect` combine, if they do
   * @param logic the logic, by name
   * @param strict whether a missing field or a type mismatch is an error instead of unknown
   * @param limits the limits of reading the table (`maxTextSize` for each text, `maxDepth`,
   *   `maxTableSize`)
   * @throws io.github.cleanrules.FormatException for an empty or repeated column, a row with more
   *   cells than the header, or a name, priority or description that is not one
   * @throws io.github.cleanrules.RuleSyntaxException for a header or cell whose text does not follow
   *   the syntax
   * @throws io.github.cleanrules.ExpressionException for an input that is a condition, or a cell that
   *   breaks a construction rule (a NaN, an enum case with parameters...)
   * @throws io.github.cleanrules.RuleDefinitionException for repeated output keys or rule names, an
   *   output cell that is not one of its column's values, an aggregate without `collect`, and the
   *   other rules of `spec/tables.md`
   * @throws io.github.cleanrules.UnsupportedException for a node cell that does not test its column
   * @throws io.github.cleanrules.RegistryException for an unknown logic
   */
  def apply(
      header: Iterable[HeaderItem],
      rows: Iterable[Iterable[Cell]] = Nil,
      hitPolicy: TablePolicy = TablePolicy.FIRST,
      aggregate: Option[Aggregate] = None,
      logic: String = "boolean",
      strict: Boolean = false,
      limits: Limits = Limits.defaults(),
  ): DecisionTable =
    val builder = io.github.cleanrules.DecisionTable
      .builder()
      .header(header.iterator.zipWithIndex.map(javaItem).toVector.asJava)
      .hitPolicy(hitPolicy)
      .aggregate(aggregate.orNull)
      .logic(logic)
      .strict(strict)
      .limits(limits)
    for (row, r) <- rows.iterator.zipWithIndex do
      builder.row(row.iterator.zipWithIndex.map((cell, c) => javaCell(cell, s"/rows/$r/$c")).toVector.asJava)
    new DecisionTable(builder.build())

  /** The Scala table of a table of the Java binding (read from a document, say). */
  def fromJava(table: io.github.cleanrules.DecisionTable): DecisionTable = new DecisionTable(table)

  private def javaItem(item: HeaderItem, c: Int): AnyRef = item match
    case text: String => text
    case node: Expr => node.asJava
    case output: TableOutput => output.javaAt(Some(s"/header/$c"))

  /** A cell as the Java builder takes it: text, a Java node, a Java constant, or null (empty). */
  private def javaCell(cell: Any, pointer: String): AnyRef | Null = cell match
    case null | None => null
    case Some(inner) => javaCell(inner, pointer)
    case text: String => text
    case node: Expr => node.asJava
    // An enum case without parameters or a case object is a constant, its
    // name, and only strings are text (spec/tables.md, *Grids*): the string
    // literal of its name, as the Java builder writes a Java enum constant.
    case constant: scala.reflect.Enum if constant.productArity == 0 => s"'${constant.productPrefix}'"
    case module: Product if ToJava.caseObject(module) => s"'${module.productPrefix}'"
    case constant => Expr.constant(constant, Some(pointer))

/**
 * An output column of a decision table: its payload key and its output
 * values, highest priority first; no values for any value. In a header,
 * `TableOutput("risk", "high", "low")` is the column
 * `THEN risk IN ('high', 'low')`: the values are Scala constants, not text. A
 * table gives its values back as Scala values (`Long`, `Double`,
 * `BigDecimal`...), which equal the ones given (`1 == 1L` in Scala).
 *
 * @param name the payload key
 * @param values the output values, highest priority first
 */
final case class TableOutput(name: String, values: Constant*):
  /**
   * The output column of the Java binding.
   *
   * @throws io.github.cleanrules.ExpressionException for a value that is not a constant (an enum
   *   case with parameters)
   */
  def asJava: io.github.cleanrules.TableOutput = javaAt(None)

  private[scaladsl] def javaAt(pointer: Option[String]): io.github.cleanrules.TableOutput =
    io.github.cleanrules.TableOutput(name, values.iterator.map(Expr.constant(_, pointer)).toVector.asJava)

object TableOutput:
  /** The Scala output column of an output column of the Java binding, with Scala values. */
  def fromJava(output: io.github.cleanrules.TableOutput): TableOutput =
    TableOutput(output.name(), output.values().asScala.iterator.map(FromJava(_).asInstanceOf[Constant]).toVector*)

/**
 * A row of a [[DecisionTable]], as the core read it: one rule. The engine
 * makes these.
 *
 * @param name the rule's name; a row without one is the rule `row_<n>`, counting from 1
 * @param priority the rule's own priority, if any
 * @param description the rule's description, if any
 * @param inputs one condition per input, testing that input (`age < 18`); `None` for any value
 * @param outputs one value per output; `None` for no value
 */
final case class TableRow private (
    name: Option[String],
    priority: Option[Long],
    description: Option[String],
    inputs: Seq[Option[Expr]],
    outputs: Seq[Option[Expr]],
)(val asJava: io.github.cleanrules.TableRow)

object TableRow:
  /** The Scala row of a row of the Java binding. */
  def fromJava(row: io.github.cleanrules.TableRow): TableRow =
    def cells(cells: java.util.List[java.util.Optional[io.github.cleanrules.Expr]]): Seq[Option[Expr]] =
      cells.asScala.iterator.map(_.toScala.map(Expr.fromJava)).toVector
    TableRow(
      row.name().toScala,
      FromJava.optionLong(row.priority()),
      row.description().toScala,
      cells(row.inputs()),
      cells(row.outputs()),
    )(row)
