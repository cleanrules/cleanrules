# Data model

**Status:** draft, normative for the core and every binding.

The core evaluates rules on **values** (`Value`), a small data model that every
language can produce. A binding's job is to turn its native data into values,
exactly and without guessing. It never evaluates anything itself (`CLAUDE.md`,
invariant 2). This document defines the values, how a binding reads native
data, and what each native type becomes.

## Values

| Kind | Holds | Type group (`ast.md`, *Types*) |
|---|---|---|
| null | nothing; may carry a reason (below) | — |
| boolean | `true` or `false` | `boolean` |
| int | a signed 64-bit integer | `number` |
| float | an IEEE 754 binary64, never NaN | `number` |
| decimal | an exact decimal of up to 38 significant digits, or ±infinity | `number` |
| string | a sequence of Unicode scalar values | `string` |
| date | a day of the proleptic Gregorian calendar, years 1 to 9999 | `date` |
| naive datetime | a date and a time of day, to the microsecond | `naive_datetime` |
| aware datetime | an instant, to the microsecond, with a fixed UTC offset or an IANA time zone | `aware_datetime` |
| list | an ordered sequence of values | `other` |
| map | string keys mapped to values, in a fixed order | `other` |
| opaque | a native value with no equivalent here | `other` |

Their JSON encoding is in `json-format.md` (*Values*).

### Decimals

A decimal is a **coefficient** `c`, an integer with `|c| < 10³⁸`, and a
**scale** `s` from 0 to 38: its value is `c × 10⁻ˢ`. This is the range of
Arrow's `Decimal128` and of `NUMERIC(38, s)` in databases. Two decimals with the
same value and different scales (`1.0` and `1.00`) are equal, and the scale is
kept for display and JSON. The two infinities are separate values.

A native decimal **fits** when some `(c, s)` in that range has the same value.
Its scale is then the native one, lowered only as far as needed by dropping
trailing zeros; a native positive exponent (`1E+3` in Python or Java) gives
`s = 0` and `c = 1000`. So `1E+37` fits, `1E+38` does not, and `1.` followed by
40 zeros fits as `1` with scale 37 (with scale 38 its coefficient would be
10³⁸). A decimal that does not fit is **never
rounded**:

- in a constant, it is an `ExpressionError` (`format_error` in JSON);
- in data, it reads as null with reason `decimal_overflow` (`ast.md`).

Converting a decimal to float, for arithmetic and aggregates (`ast.md`), is
**correctly rounded**: to the nearest binary64, ties to even, as Python's
`float(Decimal)` does. Exact differences (counterfactual gaps, `explain.md`)
that do not fit give no gap. The core implements this type itself: it needs
only exact comparison, exact subtraction, conversion to float and display.

### Floats

A float is never NaN: a NaN in data is null (`ast.md`, *Missing values*), and
in a constant it is an error. `-0.0` is kept, and equals `0.0`. A narrower
float (`float32`) is widened exactly. A wider or non-binary real number
(`numpy.longdouble`, Python `fractions.Fraction`) is rounded to the nearest
binary64.

### Datetimes

Datetimes have microsecond precision. A native value with a finer precision is
**truncated** toward the past to the microsecond, as `numpy.datetime64` values
are in Python.

A native date or datetime outside years 1 to 9999 is opaque. An aware datetime
is opaque too when its UTC instant falls outside those years, or its offset is
not a whole number of seconds within ±18 hours (Python allows sub-second
offsets).

**Time zones.** A binding sends an aware datetime as its UTC instant, computed
with the native library, plus the zone's IANA name when it has one. The core
uses its bundled time zone database (`json-format.md`, *Time zones*): if it does
not know the name spelled exactly as it spells it (names are case-sensitive), or
gives that zone a different offset at that instant, the value keeps the native
offset as a fixed offset instead. Native UTC singletons
(Python `timezone.utc`, Go `time.UTC`, Java `ZoneOffset.UTC`, JavaScript `Date`
and `Temporal.Instant`) are the fixed offset `Z`, never the zone `UTC`.

### Lists and maps

Map keys are strings. A map keeps an order, because outputs keep the order of
what they pass through. A native map whose order is defined keeps it; one
without a defined order is sorted by key, by Unicode code point (not by UTF-16
code unit), so the result never depends on hashing (invariant 7).

A list or map **read from native data** stays a reference to the native object
(*Reading native data*). Its elements are converted only when they are needed:
to compare it under `ANY` (*Equality*), and to write it to JSON. An entry of
a native map whose key is not a string is left out, and an element that cannot
be converted is opaque; neither makes the whole value opaque. A list has no
missing elements: where a native sequence has none (a hole in a JavaScript
array), the element is null.

### Opaque values

*Not checked by vectors: opaque values only come from native data, so each
binding's tests check them.*

A native value with no equivalent is **opaque**: bytes, a time of day without a
date, a duration, a complex number, a set (it has no order), a string with a
lone surrogate (possible in Python, JavaScript and Java), and anything else the
tables below do not list. An opaque value:

- is not null, so `is_null` is `FALSE` for it;
- belongs to the `other` group, so comparing it is a type mismatch;
- in an output, gives back the original native object to the host language;
- is written to JSON, in decisions and explanations only, as
  `{"$opaque": "<type name>"}` (`json-format.md`). The type name is
  informative and not compared by conformance vectors.

### Equality

Under `ANY`, payload values are compared as `rules.md` (*`ANY` equality*) says.
For the `other` group:

- two lists are equal when they have the same length and their elements are
  equal in order, by this same equality;
- two maps are equal when they have the same keys, whatever their order, and
  equal values under each key;
- two opaque values are equal when they are the **same native object**, which
  the binding identifies with a token.

### Nulls with a reason

A null produced while reading data may carry the reason it is null, which
evaluation then reports (`ast.md`, *Results and reasons*):

| Reason | When |
|---|---|
| `int_overflow` | a native integer outside the signed 64-bit range |
| `decimal_overflow` | a native decimal that does not fit (*Decimals*) |
| `read_error` | reading the native value raised an exception; the detail names it |

Every other null (a native null, a NaN) has the reason `null`.

### Constants

The conversions below also apply to native constants in conditions and
payloads (`F.x == 2**70` in Python). There, a result that would be null with
reason `int_overflow`, `decimal_overflow` or `read_error`, or opaque, is an
`ExpressionError` instead. A NaN constant and a datetime finer than a
microsecond are rejected too (`ast.md`, *Construction rules*). A native null is
the null constant.

### Fuzzy degrees

*Not checked by vectors yet: graded logics arrive in phase J.*

Under a graded logic (phase J), a number in `[0, 1]` (int, float or decimal) in
the data is a degree (`ast.md`, *Truth values*). Data never carries intervals;
if it ever must, that will be a new kind of value.

## Reading native data

*Not checked by vectors: each binding's own tests and its native conformance
run check it.*

The core reads a path through a value as `ast.md` (*Reading data*) says. Values
built directly (in Rust, through the C ABI, or from JSON) are read that way.

A binding given native data does not convert the whole input. It reads **only
the paths the rule set reads** (the field paths of its conditions and
payloads, `explain.md`, *Summaries*), walking the native structure segment by
segment with the reading rules of `ast.md` in that language's terms, and
converts only what it finds at the end of each path. Each native object is read
at most once per input, so `customer` and `customer.age` always see the same
object even when a getter is not deterministic.

The result is a **sparse value tree**, which the binding hands to the core and
from which the core derives every truth value, reason and path:

- a key that is absent, or an index beyond a list's length, is missing; a list
  keeps its length, with holes at the indices no path reads;
- at the end of a path, a scalar value, a reference to a native list or map
  (with its kind), an opaque value, or a null with a reason;
- a **record**, a map whose entries also have positions (Python namedtuples
  and pandas rows), so that a string segment reads a key and an integer
  segment a position of the same node;
- for `read_error`, the native exception is kept, so that strict mode can raise
  `EvaluationError` chained to it, in the core's evaluation order.

Optionally, a binding also records a map's available keys, for the hint of a
`missing_field` error in strict mode. The C ABI's value builder (phase E)
exposes these pieces: nulls with a reason (a `read_error` with its message),
records, lists whose unread positions are nulls, and opaque values with a type
name and a token that the C program chooses. The program keeps its objects; the
core never holds a C pointer, and an output gives the token back. Equal tokens
mean the same object (*Equality*). C has no native object to wrap, so the
builder makes no opaque value on its own: a date or datetime outside years 1 to
9999, or an offset beyond ±18 hours, is an invalid argument, and the program
chooses what to build instead.

### Safety

Every binding follows the same rules, adapted to its language:

- **Public data only.** A string segment reads a map key, a record's
  component, or an object's **public data attribute**. It never reads a private
  member (Python names starting with `_`, JavaScript `#` fields and symbols,
  Java non-public members, Go unexported fields), a method, a class, a module or
  a prototype, and never an attribute of a scalar (a string, a number, a date)
  or of the runtime's own objects that expose the program (Python frames,
  tracebacks, code objects, generators and coroutines, whose public attributes
  lead to its globals and locals). A rule loaded from a file can never reach
  into the process.
- **No side effects.** Looking up a key never inserts one (Python
  `defaultdict`, `__missing__`). A getter or property that raises gives a null
  with reason `read_error`, except the error the language uses to mean "no such
  attribute" (Python's `AttributeError`), which counts as missing.
- **Integer segments** index sequences with a length (lists, tuples, arrays,
  slices, namedtuples, pandas rows) with `0 <= i < len`, never strings, bytes or
  maps.
- **Absent is not null.** An absent key or attribute is missing; a key holding
  the language's null is null.

### Conversions by language

Each row says what a native value at the end of a path becomes, and how a
segment reads through it. Anything not listed is opaque.

#### Python

In Python, a string segment reads:

- on a `Mapping`, the value under that key if `key in obj` (checked first, so
  `__missing__` is never triggered, and subclasses are read through their own
  `__getitem__`);
- on an object whose type has `keys()`, such as a pandas row, the value under
  that key if `key in obj.keys()` (a record);
- on a namedtuple-like record (a sequence with `_fields`, such as SQLAlchemy
  rows or `itertuples()`), the attribute, with the restrictions of the next
  bullet (`row._fields` and `row.count` are missing);
- on any other object, the attribute if it is public data: never a name
  starting with `_`, a method, a class or a module, never an attribute of a
  plain sequence, and never one of a scalar (`bool`, numbers, `str`, `bytes`,
  `bytearray`, `complex`, `Decimal`, dates, times, `timedelta`), a class, a
  module, a function or method, or one of the interpreter's objects (frames,
  tracebacks, code objects, generators, coroutines, async generators and
  cells). So `F.d.days` on a `timedelta` is missing, and so is
  `F.rec.exc_info[2].tb_frame` on a logging record. These values are opaque.
- a numpy array, through its `tolist()`: indexing a `numpy.matrix` gives a
  matrix again.

Pydantic models are read by field name, not alias, and their extra and computed
fields are readable. An integer segment indexes anything with `__len__` and
`__getitem__` that is not a string, bytes, a mapping, a scalar, a class or a
module, records included.

| Native | Value |
|---|---|
| `None`, and any value not equal to itself (`float('nan')`, `Decimal('NaN')`, `numpy.nan`, `numpy.datetime64('NaT')`, `numpy.timedelta64('NaT')`), `pandas.NA`, `pandas.NaT` | null |
| `bool`, `numpy.bool_` | boolean |
| `int` and integer-likes (`numpy.int64`, `IntEnum`) | int, or null with `int_overflow` |
| `float`, and other real numbers | float (*Floats*) |
| `Decimal` (and subclasses) | decimal, or null with `decimal_overflow` |
| `str` (and subclasses, such as `StrEnum`) | string |
| `datetime.date` | date |
| `datetime.datetime` whose `tzinfo` is `None` or gives no offset, `numpy.datetime64` | naive datetime |
| `datetime.datetime` with `zoneinfo.ZoneInfo` or a `pytz` zone | aware datetime in that zone (*Time zones*) |
| `datetime.datetime` with any other `tzinfo` | aware datetime with its offset at that instant; if `utcoffset()` raises, null with `read_error` |
| any `Sequence` except `str`, `bytes` and namedtuples, `numpy` arrays | list |
| `Mapping`, objects with `keys()` such as pandas rows, namedtuples, dataclasses, attrs classes, Pydantic models (by field name, including extra and computed fields), other objects | map; a segment reads a key, a field or a public attribute (properties included), and an integer indexes a namedtuple or a pandas row |

#### JavaScript and TypeScript

| Native | Value |
|---|---|
| `null`, an Invalid `Date` | null; `undefined` is missing |
| `boolean` | boolean |
| `number` that is an integer within ±(2⁵³ − 1) | int (use `cr.float(x)` for a float) |
| any other `number`, `-0` and infinities included | float; `NaN` is null |
| `bigint` | int, or null with `int_overflow` |
| `cr.decimal("0.10")` (JavaScript has no decimal) | decimal |
| `string` | string |
| `cr.date(...)`, `Temporal.PlainDate` | date |
| `cr.datetime(...)`, `Temporal.PlainDateTime` | naive datetime |
| `Date`, `Temporal.Instant` | aware datetime in `Z` |
| `Temporal.ZonedDateTime` | aware datetime in its zone, or with a fixed offset when the zone id is an offset such as `+05:30` |
| `cr.ZonedDateTime` (as the binding gives it back, below) | aware datetime: its instant, in its zone or with its offset |
| `Array`, typed arrays except `Uint8Array` (and `Buffer`) | list; holes and `undefined` elements are null (a list cannot hold a missing element) |
| `Uint8Array`, `Buffer`, `ArrayBuffer`, `DataView` | opaque, like bytes in other languages |
| `Map` | map, in insertion order; an entry whose key is not a string is left out |
| objects of the runtime: `Error`, `Set`, `WeakMap`, `WeakSet`, `WeakRef`, `FinalizationRegistry`, `Promise`, `RegExp`, boxed primitives, iterators, generators, module namespaces, and the binding's own objects (rule sets, decisions...) | opaque, never read through; recognised by brand (a built-in method or getter that works across realms) or by `Symbol.toStringTag` |
| plain objects and class instances | map of own enumerable string-keyed properties (checked with `Object.hasOwn`), plus getters defined by the object's classes, never by `Object.prototype`; `constructor` and `__proto__` are always missing |

Temporal values are recognised by `Symbol.toStringTag`, so polyfills work;
those in a non-ISO calendar are opaque, and nanoseconds are truncated.
JavaScript orders an object's integer-like keys first, so a map passed through
from a JavaScript object may come out in a different order than the same data
in Python; equality ignores that order.

**Values given back.** Outputs, payloads, aggregates, metadata, constants (a
constant node's value, a decision table's output values), the values in an
explanation (the values a cause read, a counterfactual's target) and in a
result tree come back to JavaScript as follows, the same in Node, Deno and
browsers:

| Value | JavaScript |
|---|---|
| null, whatever its reason | `null` |
| boolean | `boolean` |
| int within ±(2⁵³ − 1) | `number`; any other int is a `bigint` |
| float | `number` (`-0` and infinities included) |
| decimal | `cr.Decimal`, holding the canonical text (`json-format.md`, *Numbers*), `inf` and `-inf` included |
| string | `string` |
| date | `cr.LocalDate` |
| naive datetime | `cr.NaiveDateTime` |
| aware datetime | `cr.ZonedDateTime`: the local date and time, the offset in seconds, the IANA zone name or `null` for a fixed offset, and the instant; it converts to `Date` and, where the runtime has it, to `Temporal.ZonedDateTime` in its IANA zone or at its offset. Temporal's offset zones have whole minutes, so an offset with seconds is a `RangeError`; in a named zone, the Temporal value's local fields follow the runtime's zone database |
| list | `Array` |
| map | a plain object whose own enumerable properties are the map's entries (a key such as `__proto__` is an own property, never the prototype) |
| opaque | the native object it was read from, the same object; `null` in a decision read from a document (`readDecision`), which keeps only the type's name |

A counterfactual's gap is a `number`, `bigint` or `cr.Decimal`, like the int,
float or decimal above; between dates it is a frozen `{ days }`, and between
datetimes a frozen `{ microseconds }`, each holding an int by the rule above.
Priorities in notes, summaries and a decision table's rows are ints by the
same rule.

These classes are frozen, and payloads, outputs, the lists and maps of a
result tree and a table output's values are frozen shallowly (the arrays and
objects inside them are not). Each value reads back as the same
value (the table above), so an output can be fed to another evaluation, with
three exceptions: a plain object never yields the keys `__proto__` and
`constructor` (pass such a map as a `Map`); an opaque value given back as
`null` reads as null; and a float that holds an integer comes back as a
`number`, which reads back as an int (wrap it in `cr.float(x)` where the
difference matters). JavaScript orders an object's integer-like keys first, so
a map given back may list them in another order than the core; equality ignores
that order.

#### Java and Scala

| Native | Value |
|---|---|
| `null`, `Optional.empty()`, Scala `None` | null; `Optional.of(x)` and `Some(x)` are read as `x` |
| `Boolean` | boolean |
| `Byte`, `Short`, `Integer`, `Long`, `BigInteger`, Scala `BigInt` | int, or null with `int_overflow` |
| `Float` (widened), `Double` | float |
| `BigDecimal`, Scala `BigDecimal` | decimal, or null with `decimal_overflow` |
| `String`, `CharSequence` | string |
| `LocalDate`, `java.sql.Date` | date |
| `LocalDateTime` | naive datetime |
| `OffsetDateTime` | aware datetime with that offset |
| `Instant`, `java.util.Date`, `java.sql.Timestamp` | aware datetime in `Z` |
| `ZonedDateTime` | aware datetime in its zone, or with a fixed offset when the zone is a `ZoneOffset` |
| `List`, arrays except `byte[]`, Scala `Seq` | list |
| `java.util.Map`, Scala `Map`, with `String` keys | map: `SequencedMap` and `SortedMap` (Scala `SeqMap` and `SortedMap`) keep their order, every other map is sorted |
| records, Scala case classes | map of their components |
| other objects | map of their public fields and public JavaBean accessors (`getAge()` and `isActive()` read as `age` and `active`) |

#### Go

| Native | Value |
|---|---|
| `nil` pointer, slice, map or interface | null |
| `bool` | boolean |
| signed and unsigned integers, `*big.Int` | int, or null with `int_overflow` |
| `float32` (widened), `float64` | float |
| `cr.Decimal` (Go has no standard decimal) | decimal |
| `string` | string |
| `cr.Date` | date |
| `cr.DateTime` (Go has no naive datetime) | naive datetime |
| `time.Time` | aware datetime: `time.UTC` is `Z`, a location loaded by name is that zone, anything else (`time.Local`, `time.FixedZone`) a fixed offset |
| slices and arrays except `[]byte` | list |
| `map[string]T` | map, sorted by key; maps with other key types are opaque |
| structs | map of their exported fields, including those promoted from embedded structs, named by a `cleanrules:"name"` tag, else a `json` tag, else the field name; `cleanrules:"-"` skips a field |

Pointers are followed, and named types are matched by their `reflect.Kind`. A
Go zero value is a value, not a missing one.

#### C, C++ and Rust

C and C++ build values through the C ABI's value builder or from JSON, and C++
adds conversions from `std::` types (with `std::unordered_map` sorted). Rust
builds values with `Value`'s constructors and a `ToValue` trait,
implemented for the standard types and, behind Cargo features, for `chrono`,
`jiff` and `rust_decimal`. There is no conversion through `serde`, because those
crates serialize dates and decimals as strings. A Rust `f64::NAN` is null, and
`i128` or `u128` outside 64 bits is null with `int_overflow`.

## Batches

*Not checked by vectors yet: the vectorised mode arrives in phase I.*

In the vectorised mode (phase I), a batch is an Arrow record batch. Its columns
map to values as above: `Decimal128` to decimal, `Date32` to date, `Timestamp`
without a zone to naive datetimes and with one to aware datetimes, nested
`Struct` and `List` columns to maps and lists, and Arrow nulls and float NaN to
null. Phase I completes the mapping: `Decimal32`, `Decimal64` and `Decimal256`,
negative scales, `Date64`, zone strings that are offsets, and dictionary-encoded
strings. Columns with no equivalent, such as `Binary` or `Duration`, are
rejected when the batch's schema is checked, because there is no native object
to give back.
