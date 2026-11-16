package io.github.cleanrules.scaladsl.tests

import io.github.cleanrules.{Documents, ExpressionException, Format, HitPolicy, UnsupportedException}
import io.github.cleanrules.scaladsl.*
import java.nio.charset.StandardCharsets.UTF_8
import java.security.MessageDigest

/** Canonical bytes and digests with Scala types, over the Java binding's Documents. */
class DigestsSuite extends munit.FunSuite:
  private val text =
    """HIT POLICY priority
      |
      |RULE vip PRIORITY 10
      |IF customer.years > 5
      |THEN discount = 0.15
      |""".stripMargin

  /** The same rule set as JSON with other whitespace, member order and number layout. */
  private val json =
    """{ "ruleset": { "rules": [ { "then": { "discount": { "value": { "$decimal": "1.5e-1" }, "op": "const" } },
      |  "when": { "right": { "value": 5, "op": "const" }, "op": "gt",
      |            "left": { "path": [ "customer", "years" ], "op": "field" } },
      |  "priority": 10, "name": "vip" } ],
      |  "logic": "boolean", "hit_policy": "priority" },
      |  "version": 1 }""".stripMargin

  private val built =
    RuleSet(Seq(Rule("vip", F.customer.years > 5, payload = Map("discount" -> BigDecimal("0.15")), priority = 10)), hitPolicy = HitPolicy.PRIORITY)

  private def sha256(bytes: Array[Byte]): String =
    "sha256:" + MessageDigest.getInstance("SHA-256").digest(bytes).map(b => f"${b & 0xff}%02x").mkString

  test("the same rule set has the same bytes and digest however it is written"):
    val fromText = loads(text, Format.TEXT)
    val bytes = canonicalBytes(fromText)
    val hash = digest(fromText)
    assert(hash.matches("sha256:[0-9a-f]{64}"), hash)
    assertEquals(hash, sha256(bytes))
    assert(String(bytes, UTF_8).startsWith("{\"ruleset\":{\"hit_policy\":\"priority\",\"logic\":\"boolean\",\"rules\":["))
    for other <- Seq[DocumentContent](loads(json), loads(dumps(fromText, Format.YAML), Format.YAML), loads(json.getBytes(UTF_8)), built) do
      assertEquals(canonicalBytes(other).toSeq, bytes.toSeq, other.toString)
      assertEquals(digest(other), hash, other.toString)
    // The layer only delegates to the Java binding.
    assertEquals(Documents.digest(DocumentContent.asJava(built)), hash)

  test("a digest identifies an artifact, not its meaning"):
    assertNotEquals(digest(F.x > 5), digest(5 < F.x))
    assertEquals(digest(F.x > 5), digest(loads("x > 5", Format.TEXT)))
    val one = Rule("r", Const(true), payload = Map("price" -> BigDecimal("1.0")))
    val two = Rule("r", Const(true), payload = Map("price" -> BigDecimal("1.00")))
    assertNotEquals(digest(RuleSet(one)), digest(RuleSet(two)))
    assert(String(canonicalBytes(two), UTF_8).contains("{\"$decimal\":\"1.00\"}"))
    assertNotEquals(digest(one), digest(one.withDescription("the price")))

  test("omitActions: a rule whose action is code without a name"):
    val plain = Rule("coded", F.x > 1)
    val coded = Rule("coded", F.x > 1, action = Some(_ => ()))
    assertEquals(intercept[UnsupportedException](digest(coded)).pointerOption, Some("/rule/do"))
    assertEquals(intercept[UnsupportedException](canonicalBytes(RuleSet(Rule("a", F.x > 1), coded))).pointerOption, Some("/ruleset/rules/1/do"))
    assertEquals(digest(coded, omitActions = true), digest(plain))
    assertEquals(canonicalBytes(RuleSet(coded), omitActions = true).toSeq, canonicalBytes(RuleSet(plain)).toSeq)
    // A named action is part of the artifact.
    assertNotEquals(digest(Rule("coded", F.x > 1, actionName = Some("notify"))), digest(plain))
    // A value node is not a condition document.
    intercept[ExpressionException](digest(F.total * 2))

  test("a decision table has its own digest, and its rule set another"):
    val table = loads("RULE,customer.years,THEN discount\nvip,> 5,0.15\nloyal,> 2,0.05\n", Format.CSV) match
      case table: DecisionTable => table
      case other => fail(s"not a table: $other")
    assertNotEquals(digest(table), digest(table.ruleSet))
    assert(String(canonicalBytes(table), UTF_8).startsWith("{\"decision_table\":"))
    assertEquals(digest(loads(dumps(table))), digest(table))
    assertEquals(digest(loads(dumps(table.ruleSet))), digest(table.ruleSet))
