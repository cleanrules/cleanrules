"""numpy, pandas, Pydantic and attrs values (`spec/data-model.md`, *Python*; task C.3b)."""

import datetime as dt
from typing import Any

import pytest
from cleanrules import Const, F, Rule, RuleSet, Truth

np = pytest.importorskip("numpy")
pd = pytest.importorskip("pandas")


def read(value: object, condition: Any) -> tuple[Truth, object]:
    """The truth of `condition` on `{"x": value}`, and `x` as it comes back."""
    rules = RuleSet([Rule("r", condition | F.never, {"x": F.x})])
    decision = rules.evaluate({"x": value, "never": False})
    result = decision.results[0]
    return result.truth, None if result.output is None else result.output["x"]


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (np.int64(5), 5),
        (np.float32(1.5), 1.5),
        (np.bool_(True), True),
        (np.str_("a"), "a"),
    ],
)
def test_numpy_scalars_are_plain_values(value: object, expected: object) -> None:
    truth, back = read(value, F.x == expected)
    assert truth is Truth.TRUE
    assert back == expected
    assert type(back) is type(expected)


@pytest.mark.parametrize(
    "value",
    [
        np.float64("nan"),
        np.datetime64("NaT", "ns"),
        np.timedelta64("NaT", "D"),
        pd.NA,
        pd.NaT,
    ],
)
def test_missing_values_are_null(value: object) -> None:
    assert read(value, F.x.is_null()) == (Truth.TRUE, None)


def test_datetime64_is_a_naive_datetime_truncated_to_microseconds() -> None:
    _, back = read(np.datetime64("2026-01-02T03:04:05.123456789"), ~F.x.is_null())
    assert back == dt.datetime(2026, 1, 2, 3, 4, 5, 123456)
    _, day = read(np.datetime64("2026-01-02"), ~F.x.is_null())
    assert day == dt.datetime(2026, 1, 2)
    truth, _ = read(np.datetime64("1850-01-01T00:00:00.5"), F.x < dt.datetime(1900, 1, 1))
    assert truth is Truth.TRUE


def test_timedelta64_is_opaque() -> None:
    delta = np.timedelta64(3, "D")
    truth, back = read(delta, ~F.x.is_null())  # opaque values are not null
    assert truth is Truth.TRUE
    assert back is delta


def test_pandas_timestamps() -> None:
    _, back = read(pd.Timestamp("2026-01-02 03:04:05.123456789"), ~F.x.is_null())
    assert back == dt.datetime(2026, 1, 2, 3, 4, 5, 123456)
    madrid = pd.Timestamp("2026-01-02 03:04", tz="Europe/Madrid")
    truth, _ = read(madrid, F.x == dt.datetime(2026, 1, 2, 2, 4, tzinfo=dt.UTC))
    assert truth is Truth.TRUE


def test_arrays_are_lists() -> None:
    assert read(np.array([1, 2, 3]), F.x[1] == 2) == (Truth.TRUE, [1, 2, 3])


def test_pandas_rows_are_records() -> None:
    frame = pd.DataFrame({"a": [1, 5], "b": [2.5, 0.5]})
    rules = RuleSet([Rule("r", (F.a > 2) & (F[1] < 1))], hit_policy="first")
    by_row = [rules.evaluate(row).fired for _, row in frame.iterrows()]
    assert by_row == [(), ("r",)]
    # itertuples() rows are namedtuples whose first component is the index.
    tuples = list(frame.itertuples())
    assert [RuleSet([Rule("r", F[1] > 2)]).evaluate(row).fired for row in tuples] == [(), ("r",)]


def test_data_frames_are_not_rows() -> None:
    from cleanrules import EvaluationError

    with pytest.raises(EvaluationError, match="row by row") as info:
        RuleSet([Rule("r", F.a > 1)]).evaluate_many(pd.DataFrame({"a": [1]}))
    assert "to_dict" in (info.value.hint or "")


def test_numpy_constants() -> None:
    assert Const(np.datetime64("2026-01-02")).value == dt.datetime(2026, 1, 2)
    assert Const(pd.NA).value is None
    with pytest.raises(Exception, match="NaN"):
        Const(np.float64("nan"))


def test_pydantic_models_by_field_name() -> None:
    pydantic = pytest.importorskip("pydantic")

    class Customer(pydantic.BaseModel):
        model_config = pydantic.ConfigDict(extra="allow", populate_by_name=True)
        full_name: str = pydantic.Field(alias="fullName")
        age: int

        @pydantic.computed_field
        @property
        def adult(self) -> bool:
            return self.age >= 18

    customer = Customer(fullName="Ana", age=30, vip=True)
    rules = RuleSet([Rule("r", (F.c.full_name == "Ana") & F.c.adult & F.c.vip, {"c": F.c})])
    decision = rules.evaluate({"c": customer})
    assert decision.fired == ("r",)
    assert decision.output == {"c": {"full_name": "Ana", "age": 30, "vip": True, "adult": True}}
    alias = RuleSet([Rule("r", F.c.fullName == "Ana")]).evaluate({"c": customer})
    assert alias.results[0].truth is Truth.UNKNOWN  # read by name, not alias


def test_attrs_classes() -> None:
    attrs = pytest.importorskip("attrs")

    @attrs.define
    class Order:
        total: float
        lines: list[int] = attrs.field(factory=list)

    rules = RuleSet([Rule("r", (F.o.total > 10) & (F.o.lines[1] == 2), {"o": F.o})])
    decision = rules.evaluate({"o": Order(12.5, [1, 2])})
    assert decision.output == {"o": {"total": 12.5, "lines": [1, 2]}}
