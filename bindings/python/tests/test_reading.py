"""Reading native Python data (`spec/data-model.md`, *Reading native data*; task C.3a)."""

import collections
import dataclasses
import datetime as dt
import threading
import time
from typing import Any, NamedTuple

import pytest
from cleanrules import (
    EvaluationError,
    F,
    FormatError,
    Rule,
    RuleSet,
    Truth,
    evaluate_tree,
    evaluate_value_tree,
)


def truth(condition: Any, data: object, **options: Any) -> Truth:
    return RuleSet([Rule("r", condition)], **options).evaluate(data).results[0].truth


def reason(condition: Any, data: object) -> str | None:
    decision = RuleSet([Rule("r", condition)]).evaluate(data)
    cause = decision.explain().rules[0].causes[0]
    return None if cause.reason is None else cause.reason.code


@dataclasses.dataclass
class Customer:
    name: str
    age: int
    tags: list[str] = dataclasses.field(default_factory=list[str])


class Point(NamedTuple):
    x: int
    y: int


class Row:
    """A stand-in for a pandas row: a record read through keys() and items."""

    def __init__(self, **values: object) -> None:
        self._values = values

    def keys(self) -> list[str]:
        return list(self._values)

    def __getitem__(self, key: str) -> object:
        return self._values[key]


# --- Only the paths a rule set reads ------------------------------------------------


class Counting:
    def __init__(self) -> None:
        self.reads: collections.Counter[str] = collections.Counter()

    @property
    def age(self) -> int:
        self.reads["age"] += 1
        return 30

    @property
    def unread(self) -> int:
        raise AssertionError("a path no rule reads must not be read")


def test_only_the_paths_read_are_read_and_each_object_once() -> None:
    customer = Counting()
    rules = RuleSet(
        [
            Rule("adult", F.customer.age >= 18),
            Rule("senior", F.customer.age >= 65, {"age": F.customer.age}),
        ]
    )
    decision = rules.evaluate({"customer": customer})
    assert decision.fired == ("adult",)
    assert customer.reads["age"] == 1


# --- Objects, records and sequences ---------------------------------------------------


def test_dataclasses_and_plain_objects_are_read_by_attribute() -> None:
    assert truth(F.customer.age > 18, {"customer": Customer("ana", 30)}) is Truth.TRUE

    class Plain:
        def __init__(self) -> None:
            self.age = 10

        @property
        def double(self) -> int:
            return self.age * 2

    assert truth(F.p.double == 20, {"p": Plain()}) is Truth.TRUE
    assert truth(F.age > 5, Plain()) is Truth.TRUE  # the input itself


def test_records_are_read_by_name_and_by_position() -> None:
    assert truth(F.p.y == 2, {"p": Point(1, 2)}) is Truth.TRUE
    assert truth(F.p[1] == 2, {"p": Point(1, 2)}) is Truth.TRUE
    row = Row(name="ana", age=30)
    assert truth(F.row.age == 30, {"row": row}) is Truth.TRUE
    assert truth(F.row[1] == 30, {"row": row}) is Truth.TRUE
    assert truth((F.row[1] == 30) & (F.row.age == 30), {"row": row}) is Truth.TRUE


def test_namedtuple_internals_are_missing() -> None:
    assert truth(F.p["_fields"].is_null(), {"p": Point(1, 2)}) is Truth.TRUE
    assert truth(F.p.count == 1, {"p": Point(1, 2)}) is Truth.UNKNOWN


def test_integer_segments_index_sequences_only() -> None:
    data = {"items": [10, 20], "t": (1, 2), "s": "abc", "m": {"0": 1}}
    assert truth(F["items"][1] == 20, data) is Truth.TRUE
    assert truth(F["items"][2] == 20, data) is Truth.UNKNOWN  # out of range
    assert truth(F.t[0] == 1, data) is Truth.TRUE
    assert truth(F.s[0] == "a", data) is Truth.UNKNOWN  # never a string
    assert truth(F.m[0] == 1, data) is Truth.UNKNOWN  # never a map key


# --- Safety ---------------------------------------------------------------------------


def test_private_names_methods_classes_and_modules_are_missing() -> None:
    class Thing:
        kind = int
        module = dataclasses

        def __init__(self) -> None:
            self._secret = 1
            self.public = 2

        def method(self) -> int:
            return 3

    data = {"t": Thing()}
    assert truth(F.t.public == 2, data) is Truth.TRUE
    # Names that clash with node attributes or start with "_" need item access.
    for path in (F.t["_secret"], F.t.method, F.t["kind"], F.t.module):
        assert reason(path == 1, data) == "missing_field"
    assert truth(F.d["_id"] == 1, {"d": {"_id": 1}}) is Truth.TRUE  # a map key is data


def test_attributes_of_scalars_and_sequences_are_missing() -> None:
    data = {"d": dt.date(2026, 1, 1), "items": [1, 2], "s": "text"}
    assert reason(F.d.year == 2026, data) == "missing_field"
    assert reason(F["items"].count == 1, data) == "missing_field"
    assert reason(F.s.upper == "x", data) == "missing_field"


def test_reading_never_inserts_keys() -> None:
    counts: collections.defaultdict[str, int] = collections.defaultdict(int)
    assert truth(F.c.x > 0, {"c": counts}) is Truth.UNKNOWN
    assert "x" not in counts

    class Missing(dict[str, int]):
        def __missing__(self, key: str) -> int:
            raise AssertionError("__missing__ must not run")

    assert truth(F.c.x > 0, {"c": Missing()}) is Truth.UNKNOWN


def test_absent_is_missing_and_none_is_null() -> None:
    assert reason(F.a > 1, {}) == "missing_field"
    assert reason(F.a > 1, {"a": None}) == "null"
    assert truth(F.a.is_null(), {}) is Truth.TRUE


# --- Errors while reading -------------------------------------------------------------


class Failing:
    @property
    def broken(self) -> int:
        raise ValueError("the database is down")

    @property
    def absent(self) -> int:
        raise AttributeError("no such thing")


def test_a_getter_that_raises_gives_read_error() -> None:
    assert reason(F.f.broken > 1, {"f": Failing()}) == "read_error"
    assert reason(F.f.absent > 1, {"f": Failing()}) == "missing_field"


def test_strict_mode_chains_the_original_exception() -> None:
    rules = RuleSet([Rule("r", F.f.broken > 1)], strict=True)
    with pytest.raises(EvaluationError) as info:
        rules.evaluate({"f": Failing()})
    assert info.value.path == ("f", "broken")
    assert isinstance(info.value.__cause__, ValueError)
    assert "database is down" in str(info.value.__cause__)
    with pytest.raises(EvaluationError) as batch:
        rules.evaluate_many([{"f": Failing()}])
    assert batch.value.row == 0
    assert isinstance(batch.value.__cause__, ValueError)


# --- Values at the end of a path ------------------------------------------------------


def test_opaque_values_come_back_as_the_same_object() -> None:
    blob = object()
    rules = RuleSet([Rule("r", F.flag, {"blob": F.blob, "raw": F.raw})])
    decision = rules.evaluate({"flag": True, "blob": blob, "raw": b"bytes"})
    assert decision.output is not None
    assert decision.output["blob"] is blob
    assert decision.output["raw"] == b"bytes"
    assert truth(F.raw.is_null(), {"raw": b"x"}) is Truth.FALSE
    assert reason(F.raw == 1, {"raw": b"x"}) == "type_mismatch"


def test_objects_at_the_end_of_a_path_become_maps() -> None:
    rules = RuleSet([Rule("r", F.go, {"customer": F.customer, "point": F.point})])
    output = rules.evaluate(
        {"go": True, "customer": Customer("ana", 30, ["vip"]), "point": Point(1, 2)}
    ).output
    assert output == {
        "customer": {"name": "ana", "age": 30, "tags": ["vip"]},
        "point": {"x": 1, "y": 2},
    }


def test_objects_that_contain_themselves_do_not_crash() -> None:
    class Node:
        def __init__(self) -> None:
            self.value = 1
            self.next = self

    rules = RuleSet([Rule("r", F.n.value == 1, {"n": F.n})])
    decision = rules.evaluate({"n": Node()})
    assert decision.fired == ("r",)


# --- Review fixes ---------------------------------------------------------------------


def test_the_interpreter_cannot_be_reached_through_the_data() -> None:
    import logging
    import sys

    # A local that no rule may read.
    api_key = "secret"  # noqa: F841  # pyright: ignore[reportUnusedVariable]

    try:
        raise ValueError("boom")
    except ValueError:
        record = logging.LogRecord("x", logging.ERROR, "f", 1, "m", (), sys.exc_info())

    def generator() -> Any:
        secret = "local"  # noqa: F841  # pyright: ignore[reportUnusedVariable]
        yield 1

    running = generator()
    next(running)

    async def coroutine() -> None:
        pass

    pending = coroutine()
    frame = sys._getframe()  # pyright: ignore[reportPrivateUsage]
    data = {"rec": record, "g": running, "c": pending, "f": frame}
    for path in (
        F.rec.exc_info[2].tb_frame.f_globals,
        F.g.gi_frame.f_locals.secret,
        F.c.cr_frame,
        F.f.f_locals.api_key,
        F.f.f_code.co_name,
    ):
        assert reason(path == "x", data) == "missing_field"
    pending.close()


def test_a_decision_in_a_reference_cycle_is_freed() -> None:
    import gc
    import weakref

    class Holder:
        decision: object = None

    rules = RuleSet([Rule("r", F.h.is_null() | ~F.h.is_null(), {"h": F.h})])
    holder = Holder()
    holder.decision = rules.evaluate({"h": holder})
    gone = weakref.ref(holder)
    del holder
    gc.collect()
    assert gone() is None


def test_only_attribute_error_means_missing_for_attributes() -> None:
    class Lookup:
        @property
        def value(self) -> int:
            raise KeyError("not here")

    assert reason(F.o.value > 1, {"o": Lookup()}) == "read_error"


def test_rows_with_fields_are_records_even_if_not_tuples() -> None:
    class Row:  # a stand-in for a SQLAlchemy 2 row
        _fields = ("name", "age")

        def __init__(self, name: str, age: int) -> None:
            self.name, self.age = name, age

        def __len__(self) -> int:
            return 2

        def __getitem__(self, i: int) -> object:
            return (self.name, self.age)[i]

    row = Row("ana", 30)
    assert truth(F.r.age == 30, {"r": row}) is Truth.TRUE
    assert truth(F.r[1] == 30, {"r": row}) is Truth.TRUE


def test_functions_are_opaque() -> None:
    def function() -> None:
        pass

    function.public = 1  # type: ignore[attr-defined]
    decision = RuleSet([Rule("r", ~F.f.is_null(), {"f": F.f})]).evaluate({"f": function})
    assert decision.output == {"f": function}
    assert reason(F.f.public == 1, {"f": function}) == "missing_field"


def test_errors_while_finding_the_shape_are_read_errors() -> None:
    class Broken:
        def __len__(self) -> int:
            raise RuntimeError("no length")

        def __getitem__(self, i: int) -> int:
            return i

    assert reason(F.b[0] == 0, {"b": Broken()}) == "read_error"


def test_keys_are_listed_once_per_record() -> None:
    class Row:
        calls = 0

        def keys(self) -> list[str]:
            Row.calls += 1
            return ["a", "b", "c", "d"]

        def __getitem__(self, key: str) -> int:
            return 1

    rules = RuleSet([Rule("r", F.row.a == 1, {"row": F.row})])
    rules.evaluate({"row": Row()})
    assert Row.calls <= 2  # once to read `a`, once to convert the whole row


def test_a_read_error_in_the_middle_of_a_path_is_not_a_null() -> None:
    class Customer:
        @property
        def profile(self) -> object:
            raise ValueError("the profile service is down")

    data = {"c": Customer()}
    assert reason(F.c.profile.age > 18, data) == "read_error"
    with pytest.raises(EvaluationError) as info:
        RuleSet([Rule("r", F.c.profile.age > 18)], strict=True).evaluate(data)
    assert info.value.path == ("c", "profile", "age")
    assert isinstance(info.value.__cause__, ValueError)


# ---- max_values (`spec/data-model.md`, *Reading native data*, *Limits*).


def diamond(depth: int) -> object:
    """A list holding the same list twice, `depth` times over: 2**depth references."""
    node: object = 1
    for _ in range(depth):
        node = [node, node]
    return node


def on_small_stack(call: Any) -> Any:
    """Run `call` on a thread with 512 KiB of stack, as a host thread may have."""
    result: list[Any] = []
    failure: list[BaseException] = []

    def run() -> None:
        try:
            result.append(call())
        except BaseException as error:  # re-raised below
            failure.append(error)

    old = threading.stack_size(512 * 1024)
    try:
        thread = threading.Thread(target=run)
        thread.start()
        thread.join()
    finally:
        threading.stack_size(old)
    if failure:
        raise failure[0]
    return result[0]


def beyond(call: Any) -> FormatError:
    """The `limit_exceeded` that `call` raises for data beyond `max_values`."""
    with pytest.raises(FormatError) as raised:
        call()
    error = raised.value
    assert error.code == "limit_exceeded"
    assert error.name == "max_values"
    assert "max_values" in str(error)
    assert error.hint is not None
    return error


def test_a_diamond_of_depth_30_is_refused_within_max_values() -> None:
    rules = RuleSet([Rule("r", F.d.is_null()), Rule("s", F.x == 1)])
    data = {"d": diamond(30), "x": 1}
    started = time.monotonic()
    # Refused in every mode: an input beyond a limit is never evaluated.
    error = beyond(lambda: on_small_stack(lambda: rules.evaluate(data)))
    # A million values, not a thousand million.
    assert time.monotonic() - started < 30
    assert error.rule is None and error.path is None and error.row is None
    strict = RuleSet([Rule("r", F.d.is_null())], strict=True)
    beyond(lambda: strict.evaluate(data))


def test_max_values_counts_exactly() -> None:
    # diamond(3): 8 ints and 7 lists, plus the int at `x`: 16 values.
    rules = RuleSet([Rule("r", F.d.is_null()), Rule("s", F.x == 1)])
    data = {"d": diamond(3), "x": 1}
    assert [r.truth for r in rules.evaluate(data, max_values=16).results] == [
        Truth.FALSE,
        Truth.TRUE,
    ]
    beyond(lambda: rules.evaluate(data, max_values=15))
    # A shared object counts each time it is converted.
    shared = diamond(3)
    both = RuleSet([Rule("a", F.a.is_null()), Rule("b", F.b.is_null())])
    assert both.evaluate({"a": shared, "b": shared}, max_values=30).results[1].truth == Truth.FALSE
    beyond(lambda: both.evaluate({"a": shared, "b": shared}, max_values=29))
    # A list along the paths counts its length, and what is read at a position.
    second = RuleSet([Rule("r", F.xs[1] == 20)])
    assert second.evaluate({"xs": [10, 20, 30]}, max_values=4).results[0].truth == Truth.TRUE
    beyond(lambda: second.evaluate({"xs": [10, 20, 30]}, max_values=3))
    # A record counts each of its positions.
    record = RuleSet([Rule("r", F.p.x == 1)])
    assert record.evaluate({"p": Point(1, 2)}, max_values=3).results[0].truth == Truth.TRUE
    beyond(lambda: record.evaluate({"p": Point(1, 2)}, max_values=2))
    # A read error from a getter is a value placed in the tree.
    failing = RuleSet([Rule("r", F.f.broken > 1)])
    assert failing.evaluate({"f": Failing()}, max_values=1).results[0].truth == Truth.UNKNOWN
    beyond(lambda: failing.evaluate({"f": Failing()}, max_values=0))
    # The default allows a million values: 2**19 - 1 of them, not 2**20 - 1.
    one = RuleSet([Rule("r", F.d.is_null())])
    assert one.evaluate({"d": diamond(18)}).results[0].truth == Truth.FALSE
    beyond(lambda: one.evaluate({"d": diamond(19)}))


def test_a_huge_declared_length_fails_fast() -> None:
    huge = range(10**12)
    # Along the paths, before the list is allocated.
    beyond(lambda: RuleSet([Rule("r", F.r[0] == 0)]).evaluate({"r": huge}))
    # Where a path ends, before its elements are converted.
    beyond(lambda: RuleSet([Rule("r", F.r.is_null())]).evaluate({"r": huge}))


def test_a_batch_reports_the_row_beyond_the_limit() -> None:
    rules = RuleSet([Rule("s", F.x == 1)])
    error = beyond(
        lambda: rules.evaluate_many([{"x": 1}, {"x": [1, 1, 1]}, {"x": 1}], max_values=2)
    )
    assert error.row == 1
    assert [
        d.results[0].truth for d in rules.evaluate_many([{"x": 1}, {"x": [1]}], max_values=2)
    ] == [
        Truth.TRUE,
        Truth.UNKNOWN,
    ]
    # Read before evaluating: the limit comes before an evaluation error.
    strict = RuleSet([Rule("s", F.y == 1)], strict=True)
    assert beyond(lambda: strict.evaluate_many([{}, {"y": [1, 1, 1]}], max_values=2)).row == 1


def test_max_values_applies_to_runs_tables_trees_and_metadata() -> None:
    ran: list[str] = []
    rules = RuleSet([Rule("r", F.x.is_null(), do=lambda _: ran.append("r"))])
    beyond(lambda: rules.run({"x": diamond(30)}))
    # diamond(2): 4 ints and 3 lists.
    rules.run({"x": diamond(2)}, max_values=7)
    assert ran == []  # a list is not null
    beyond(lambda: rules.run({"x": diamond(2)}, max_values=6))
    beyond(lambda: evaluate_tree(F.x == 1, {"x": diamond(30)}))
    beyond(lambda: evaluate_tree(F.x == 1, {"x": diamond(2)}, max_values=6))
    assert evaluate_tree(F.x == 1, {"x": diamond(2)}, max_values=7).reason is not None
    beyond(lambda: evaluate_value_tree(F.x, {"x": diamond(2)}, max_values=6))
    # Metadata is converted with the default limit, naming the rule.
    error = beyond(lambda: Rule("big", F.x == 1, metadata={"m": diamond(20)}))
    assert error.rule == "big"
    assert Rule("small", F.x == 1, metadata={"m": diamond(3)}).metadata["m"] is not None
