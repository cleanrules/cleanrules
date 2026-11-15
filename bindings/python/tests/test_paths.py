"""The limit `max_path_segments` on the paths of fields.

`spec/json-format.md`, *Errors and limits*; and the depth of the Python data
read along them.
"""

import json
import threading
from typing import Any

import cleanrules
import pytest
from cleanrules import Field, FormatError, Rule, RuleSet


def long_path(n: int) -> list[str | int]:
    """A path of `n` segments, keys and indices in turns."""
    return ["k" if i % 2 == 0 else 0 for i in range(n)]


def along(n: int) -> object:
    """Data as deep as `long_path(n)`, with 1 at its end."""
    data: object = 1
    for i in reversed(range(n)):
        data = {"k": data} if i % 2 == 0 else [data]
    return data


def beyond(error: pytest.ExceptionInfo[FormatError]) -> None:
    assert error.value.code == "limit_exceeded"
    assert error.value.name == "max_path_segments"
    assert "max_path_segments" in str(error.value)
    assert error.value.hint is not None


def test_a_field_has_at_most_256_segments() -> None:
    assert Field(*long_path(256)).path == tuple(long_path(256))
    with pytest.raises(FormatError) as raised:
        Field(*long_path(257))
    beyond(raised)
    # F builds the same fields, one segment at a time.
    field: Any = cleanrules.F.k
    for segment in long_path(256)[1:]:
        field = field[segment]
    with pytest.raises(FormatError) as raised:
        field[0]
    beyond(raised)


def test_readers_take_the_limit() -> None:
    text = "IF a.b.c THEN y = 1"
    with pytest.raises(FormatError) as raised:
        cleanrules.rule_from_text(text, max_path_segments=2)
    beyond(raised)
    cleanrules.rule_from_text(text, max_path_segments=3)
    with pytest.raises(FormatError) as raised:
        cleanrules.loads(text, format="text", max_path_segments=2)
    beyond(raised)
    document = {"version": 1, "condition": {"op": "field", "path": long_path(300)}}
    longer = cleanrules.loads(json.dumps(document), max_path_segments=300)
    assert isinstance(longer, Field)
    with pytest.raises(FormatError) as raised:
        cleanrules.DecisionTable(["a.b.c", "THEN y"], [["1", 1]], max_path_segments=2)
    beyond(raised)
    with pytest.raises(ValueError, match="max_path_segments"):
        cleanrules.loads("x", format="text", max_path_segments=0)


def test_a_path_at_the_limit_over_data_as_deep_runs_on_a_small_stack() -> None:
    path = long_path(256)
    rules = RuleSet([Rule("r", Field(*path) == 1, {"y": Field(*path)})])
    # A list nested 255 deep where the path ends, copied whole into the output.
    end: object = 1
    for _ in range(255):
        end = [end]
    data: Any = along(256)
    parent: Any = data
    for segment in path[:-1]:
        parent = parent[segment]
    copied = RuleSet([Rule("r", ~Field(*path).is_null(), {"y": Field(*path)})])
    result: list[int] = []

    def run() -> None:
        result.append(len(rules.evaluate(along(256)).selected))
        parent[path[-1]] = end
        result.append(len(copied.evaluate(data).selected))

    old = threading.stack_size(512 * 1024)
    try:
        thread = threading.Thread(target=run)
        thread.start()
        thread.join()
    finally:
        threading.stack_size(old)
    assert result == [1, 1]


def test_the_summary_tool_describes_a_long_path_to_32_segments() -> None:
    path = long_path(256)
    rules = RuleSet([Rule("r", Field(*path) == 1, {"y": Field(*path)})])
    result = cleanrules.Catalog({"deep": rules}).call("summary", {"ruleset": "deep"})
    assert not result.is_error
    content: Any = result.content
    assert content["summary"]["fields"][0]["path"] == path
    # Down 32 segments, then a place described in words that accepts anything.
    place: Any = content["input_schema"]
    for segment in path[:32]:
        alternatives = place.get("anyOf", [place])
        container = alternatives[0]
        place = (
            container["properties"][segment]
            if isinstance(segment, str)
            else container["prefixItems"][segment]
        )
    assert place == {"description": "fields below are not described (deeper than 32 path segments)"}
