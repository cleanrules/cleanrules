package io.github.cleanrules.scaladsl.tests

import io.github.cleanrules.{ActionException, HitPolicy, RegistryException, Truth}
import io.github.cleanrules.scaladsl.*
import scala.collection.immutable.VectorMap
import scala.collection.mutable.ArrayBuffer
import scala.jdk.CollectionConverters.*

/** Actions as Scala functions: run, bind, their context, and the errors they raise. */
class ActionsSuite extends munit.FunSuite:
  final case class Account(owner: String, balance: BigDecimal)

  test("run calls the selected rules' actions in order, with their context"):
    val calls = ArrayBuffer[String]()
    var context: Option[ActionContext] = None
    val low: Action = ctx => calls += s"low:${ctx.output("level")}"
    val rules = RuleSet(
      Seq(
        Rule("high", F.balance > 1000, payload = Map("level" -> "high"), action = Some(ctx => {
          context = Some(ctx)
          calls += s"high:${ctx.rule.name}"
        })),
        Rule("low", F.balance > 10, payload = Map("level" -> "low"), action = Some(low)),
        Rule("none", F.balance > 1),
      ),
      hitPolicy = HitPolicy.RULE_ORDER,
    )
    val account = Account("ana", BigDecimal(5000))
    val decision = rules.run(account)
    assertEquals(calls.toSeq, Seq("high:high", "low:low"))
    val ctx = context.get
    assertEquals(ctx.rule.name, "high")
    assert(ctx.data.asInstanceOf[AnyRef] eq account)
    assertEquals(ctx.output, VectorMap("level" -> "high"))
    assertEquals(ctx.result.truth, Truth.TRUE)
    assertEquals(ctx.result.rule, rules("high"))
    assertEquals(ctx.tree.node.toString, "balance > 1000")
    assertEquals(ctx.tree.truth, Some(Truth.TRUE))
    // The decision of the context is the decision run gives back.
    assertEquals(ctx.decision, decision)
    assert(ctx.decision.asJava eq decision.asJava)
    // The function comes back from the rule.
    assertEquals(rules("low").action, Some(low))
    // evaluate never runs actions.
    calls.clear()
    rules.evaluate(account)
    assertEquals(calls.toSeq, Nil)

  test("named actions are bound to functions"):
    val calls = ArrayBuffer[String]()
    val rules = RuleSet(Rule("vip", F.years > 5, actionName = Some("notify_sales")))
    // Unbound, run fails before reading the data.
    intercept[RegistryException](rules.run(Map("years" -> 7)))
    val bound = rules.bind(Map("notify_sales" -> (ctx => calls += ctx.rule.name)))
    bound.run(Map("years" -> 7))
    assertEquals(calls.toSeq, Seq("vip"))
    assertEquals(bound("vip").actionName, Some("notify_sales"))
    assert(bound("vip").action.isDefined)
    intercept[RegistryException](rules.bind(Map("other" -> (_ => ()))))
    // Code bound to a name keeps it; a name drops the code.
    val coded = Rule("c", F.x, actionName = Some("named"), action = Some(_ => ()))
    assertEquals(coded.actionName, Some("named"))
    assert(coded.action.isDefined)
    assertEquals(coded.withActionName("other").action, None)
    assert(Rule("c", F.x).withAction(_ => ()).action.isDefined)
    // Rules from text bind their DO to a function.
    val parsed = ruleFromText("RULE vip IF years > 5 THEN d = 1 DO notify_sales", Map("notify_sales" -> (_ => ())))
    assert(parsed.action.isDefined)

  test("an action that throws: the Java binding's ActionException, with the throwable as its cause"):
    val boom = IllegalStateException("boom")
    val rules = RuleSet(
      Seq(Rule("first", F.ok, action = Some(_ => ())), Rule("second", F.ok, action = Some(_ => throw boom))),
      hitPolicy = HitPolicy.RULE_ORDER,
    )
    val error = intercept[ActionException](rules.run(Map("ok" -> true)))
    assertEquals(error.code(), "action_error")
    assert(error.getCause eq boom)
    assertEquals(error.rule().orElseThrow(), "second")
    assertEquals(error.executed().asScala.toSeq, Seq("first"))
    assertEquals(Decision.fromJava(error.decision().orElseThrow()).fired, Seq("first", "second"))

  test("Java actions keep working from Scala"):
    val calls = ArrayBuffer[String]()
    val javaAction: io.github.cleanrules.Action = ctx => calls += ctx.rule().name()
    val rule = Rule.fromJava(io.github.cleanrules.Rule.named("j").when(F.ok.asJava).action(javaAction))
    RuleSet(rule).run(Map("ok" -> true))
    // Called from Scala, it gets the Java context of the Scala one.
    rule.action.get(ActionContextProbe.context())
    assertEquals(calls.toSeq, Seq("j", "probe"))

/** A context of a real run, to call a rule's action by hand. */
object ActionContextProbe:
  def context(): ActionContext =
    var seen: Option[ActionContext] = None
    RuleSet(Rule("probe", Const(true), action = Some(ctx => seen = Some(ctx)))).run(Map.empty)
    seen.get
