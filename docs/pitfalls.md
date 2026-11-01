# Common pitfalls

cleanrules conditions are **data**: `F.age >= 18` does not compare anything, it
builds a node that the cleanrules core evaluates later. This is what lets one rule be
explained, serialized, vectorized and audited. Polars and SQLAlchemy work the
same way. It also means some everyday Python idioms behave differently. Each
pitfall below either raises a clear `ExpressionError` or has a simple
alternative.

## Use `&`, `|` and `~`, not `and`, `or` and `not`

```python
(F.age >= 18) & (F.country == "ES")  # yes
(F.age >= 18) and (F.country == "ES")  # ExpressionError
```

Python's `and`, `or`, `not` and `if` ask for a real `True` or `False`, which a
node cannot give. cleanrules raises instead of guessing.

**Parenthesize comparisons.** `&` and `|` bind more tightly than `>` and `==`
in Python, so `F.a > 1 & F.b < 2` means `F.a > (1 & F.b) < 2` (an error).

**Constants need `Const`.** `cond & True` raises: a Python bool next to `&` or
`|` almost always comes from a comparison that Python evaluated itself (see
*Field names that clash* below). Write `cond & Const(True)` if you really mean
a constant.

## No chained comparisons

```python
F.x.between(0, 10, closed="none")  # yes
0 < F.x < 10  # ExpressionError
```

Python expands `0 < F.x < 10` to `(0 < F.x) and (F.x < 10)`, which runs into the
`and` problem above.

## Test for missing values with `is_null()`

```python
F.email.is_null()  # yes
F.email == None  # ExpressionError, with a hint
```

Missing values are `UNKNOWN` under three-valued logic, so `x == None` would
never be true.

## Compare nodes with `same_as`, not `==`

`==` builds a node, so it cannot tell you whether two nodes are equal:

| Instead of | Use |
|---|---|
| `node1 == node2` | `node1.same_as(node2)` |
| `node in list_of_nodes` | `any(node.same_as(n) for n in list_of_nodes)` |
| `{node: ...}` keyed by structure | `{node.structural_key(): ...}` |
| `assert node1 == node2` in tests | `assert node1.same_as(node2)` |

Nodes hash **by identity**. Sets and dicts of node objects work as long as you
look up the very same objects.

- `node in some_list` only works when no other node comes before `node` in
  the list. `a in [b, a]` raises even though `a` is there.
- `list.index`, `list.remove`, `list.count` and `==` between lists or tuples of
  nodes compare with `==`, so avoid them on nodes.
- `WeakKeyDictionary` does not work with nodes.

Nodes are immutable: `copy.copy` and `copy.deepcopy` return the node itself,
and pickling keeps the tree (in the JSON format).
- `sorted`, `min`, `max` and `list.sort` on nodes raise, because `<` builds a
  node too.

## Nodes are not collections

`list(F.orders)`, `for o in F.orders:`, `[*F.x]` and `"vip" in F.tags` raise
`ExpressionError`. Without that error, Python would iterate `F.orders[0]`,
`F.orders[1]`, ... forever. Index explicitly (`F.orders[0].total`), and test
membership against constants with `F.x.is_in([...])`.

`F.tags.contains("vip")` is **not** list membership either: `contains`,
`starts_with` and `ends_with` work on strings and take literal text, not a
pattern. polars' `str.contains`, by contrast, treats its argument as a regex
by default. Use `matches` for patterns.

## Field names that clash with node attributes

Any public attribute of a `Field` keeps its meaning after a dot: `path`, `tag`,
`kind`, `children`, `walk`, `same_as`, `structural_key`, `field_paths`,
and the methods
`is_in`, `between`, `is_null`, `matches`, `starts_with`, `ends_with`,
`contains` and `abs`. So `F.customer.kind` is the node's kind (`"any"`), not a
field called `kind`. A comparison on it is plain Python:

- `(F.p > 5) & (F.product.tag == "sale")` raises, because `&` receives a
  Python `False`;
- `F.a == F.b.kind` is **silent**: it builds `F.a == 'any'`.

Use item access for such names:

```python
F.customer["kind"]  # the field "kind"
F["class"]  # Python keywords
F["_id"]  # names starting with an underscore
F["key with spaces"]
```

The first segment after `F` is always a field, so `F.kind` and `F.path` do work.

Calling a field path raises with a suggestion. `F.x.isin([1])` answers "did you
mean `.is_in(...)`?", and the same works for `startswith`, `isnull` and
`is_between`.

## No polars-style namespaces

Methods live on the field itself: `F.name.starts_with("A")`, not
`F.name.str.starts_with("A")`. After the first segment, the names `str`, `dt`,
`list`, `arr`, `struct` and `cat` raise `AttributeError`. A real field with
one of those names is reached as `F.x["str"]`.

## numpy and pandas values

numpy scalars work on either side: `np.int64(5) < F.x` builds `F.x > 5`.
Arrays, Series and DataFrames are not constants. `np.array([1, 2]) == F.x`
and `df["a"] > F.x` raise instead of comparing element by element. To check
whole columns, use vectorized validation (phase I of the roadmap).

## fuzzytool terms go on the right

Until fuzzytool 0.8, `score["poor"] & (F.x > 2)` builds a fuzzytool node
instead of a cleanrules one. Put the cleanrules condition first,
`(F.x > 2) & score["poor"]`, or wrap the term with `fz(...)` (phase J).
