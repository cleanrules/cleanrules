"""Task 2.12: RuleSet.summary(), a static overview of a rule set."""

from rulepy import ActionContext, F, Rule, RuleSet
from rulepy.core.summary import FieldUsage, RuleSetSummary
from tests.snapshot import assert_snapshot


def notify(ctx: ActionContext) -> None: ...


RULES = RuleSet(
    [
        Rule("small", when=F.total > 10, then={"discount": 0.05}, priority=1, tags=["pricing"]),
        Rule(
            "big",
            when=(F.total > 1000) & ~F.customer.blocked,
            then={"discount": F.total * 0.1, "review": True},
            priority=5,
            tags=["risk", "pricing"],
            do=notify,
        ),
        Rule(
            "vip",
            when=F.customer.years.between(5, 50)
            & F.customer.country.is_in({"ES", "PT"})
            & (F.customer.segment == "gold"),
            then={"discount": 0.2},
        ),
        Rule("audit", when=F["odd key"].is_null()),
    ],
    hit_policy="priority",
)


def test_fields_are_listed_in_order_with_where_they_are_read() -> None:
    summary = RULES.summary()
    assert isinstance(summary, RuleSetSummary)
    assert summary.fields[0] == FieldUsage(("total",), ("small", "big"), (("big", "discount"),))
    assert [f.path for f in summary.fields] == [
        ("total",),
        ("customer", "blocked"),
        ("customer", "years"),
        ("customer", "country"),
        ("customer", "segment"),
        ("odd key",),
    ]


def test_outputs_and_tags() -> None:
    summary = RULES.summary()
    assert summary.outputs == ("discount", "review")
    assert dict(summary.tags) == {"pricing": ("small", "big"), "risk": ("big",)}


def test_fields_read_only_by_payloads() -> None:
    rules = RuleSet([Rule("r", when=F.x > 0, then={"v": F.rate * 2})])
    usage = rules.summary().fields
    assert usage == (FieldUsage(("x",), ("r",), ()), FieldUsage(("rate",), (), (("r", "v"),)))


def test_summary_snapshot() -> None:
    assert_snapshot("summary_priority", str(RULES.summary()))


def test_empty_and_configured_rule_sets() -> None:
    text = str(RuleSet([], hit_policy="collect", aggregate="sum", strict=True).summary())
    assert text.splitlines() == [
        "RuleSet: 0 rules, hit policy COLLECT, aggregate sum, logic boolean, strict",
        "  (no rules)",
    ]


def test_payload_only_fields_and_repeated_reads() -> None:
    rules = RuleSet([Rule("r", when=(F.x > 0) & (F.x < 9), then={"v": F.rate})])
    summary = rules.summary()
    assert summary.fields[0] == FieldUsage(("x",), ("r",), ())
    assert "  rate  payloads of r (v)" in str(summary)


def test_single_rule_wording_and_ascii() -> None:
    text = str(RuleSet([Rule("only", when=F.x > 1)]).summary())
    assert text.startswith("RuleSet: 1 rule, ")
    assert text.isascii()


def test_odd_names_tags_and_keys_cannot_break_the_layout() -> None:
    rules = RuleSet(
        [
            Rule("x\ny", when=F["a\nb"] > 1, then={"k\n": 1}, tags=["t, u"]),
            Rule("a, b", when=F.x > 1, tags=["t, u"]),
        ]
    )
    text = str(rules.summary())
    assert len(text.splitlines()) == 9
    assert "'x\\ny'" in text and "'a, b'" in text and "'t, u'" in text


def test_unicode_is_kept_but_control_characters_are_escaped() -> None:
    rules = RuleSet(
        [Rule("descuento año", when=F["año"] == "ñandú", then={"tasa€": 1}, tags=["prício"])]
    )
    text = str(rules.summary())
    assert "'descuento año'" in text and "'tasa€'" in text and "año" in text
    assert all(ch.isprintable() for line in text.splitlines() for ch in line)
