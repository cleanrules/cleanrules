# Bundles

**Status:** draft, format version `1` (task R.5). JSON Schema:
[`schemas/v1/bundle.schema.json`](schemas/v1/bundle.schema.json).

A **bundle** is a reproducible package of rules: the rules themselves, who
names them and which revision they are, the digest that identifies them, the
exact engine and configuration that ran them, the contract of their input and
output, and the files of test cases that go with them. A user who tests and
compares two versions of their rules keeps one bundle per version, and can
then say exactly what was run: the same bundle on the same engine gives the
same decisions, explanations and errors (`CLAUDE.md`, invariant 7).

Comparing two bundles answers two different questions, which a comparison of
results (task T.4) keeps apart:

- **the same rules under another engine**: same rule set digest, another
  engine version or configuration. Any change in the results comes from the
  engine;
- **other rules under the same engine**: another rule set digest, the same
  engine and configuration. Any change in the results comes from the rules.

A bundle is a document of the JSON format (`json-format.md`, *Documents*) with
the content key `"bundle"`, added to version 1 (*Versioning* there): older
readers reject it with `format_error`. It is read and written in JSON and YAML;
the rule syntax and CSV cannot hold one (`unsupported`).

```json
{
  "version": 1,
  "bundle": {
    "id": "acme/discounts",
    "revision": "2026-11-16.1",
    "ruleset": {
      "hit_policy": "first",
      "logic": "boolean",
      "rules": [
        { "name": "vip", "when": { "op": "gt", "left": { "op": "field", "path": ["years"] }, "right": { "op": "const", "value": 5 } },
          "then": { "discount": { "op": "const", "value": { "$decimal": "0.15" } } } }
      ]
    },
    "ruleset_digest": "sha256:5a0e590cb8f3bd57392ede84c0d0c4596cf9374fefcbcc6b28ed61421878f312",
    "engine": {
      "name": "cleanrules",
      "version": "0.4.0",
      "limits": { "max_size": 16777216, "max_nesting": 1024, "max_depth": 256, "max_path_segments": 256, "max_table_size": 2097152, "max_values": 1000000 }
    },
    "contract": {
      "input": {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
          "years": { "description": "conditions of vip", "anyOf": [{ "type": "number" }, { "type": "null" }] }
        }
      },
      "output": { "keys": ["discount"] }
    },
    "cases": [
      { "path": "cases/discounts.json", "digest": "sha256:37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570" }
    ]
  }
}
```

## Keys

| Key | Required | JSON type |
|---|---|---|
| `id` | yes | string: the stable identifier of the rules (*Identifier and revision*) |
| `revision` | yes | string: one immutable revision of them (*Identifier and revision*) |
| `ruleset` | one of the two | the content of a rule set document (`json-format.md`, *Rule sets*) |
| `decision_table` | one of the two | the content of a decision table document (`tables.md`, *JSON*) |
| `ruleset_digest` | yes | string: the rule set digest of the rules it holds (*Rules and their digest*) |
| `engine` | yes | object: the engine and its configuration (*Engine*) |
| `contract` | yes | object: the contract of the input and the output (*Contract*) |
| `cases` | no, default `[]` | array of references to files of test cases (*Cases*) |

A bundle holds its rules whole, not a reference to a file: a bundle is
reproducible on its own, and its digest covers the rules. It holds exactly one
of `ruleset` and `decision_table`; neither, or both, is a `format_error` at
`/bundle`. A single rule is given as a rule set of that rule.

Writers write the keys in the order of the table, omit `cases` when it is
empty, and write every member of `engine` and `contract`.

## Identifier and revision

`id` names the rules across their revisions, such as `acme/discounts` or
`loan-approval`: one to 128 characters, made of one or more segments separated
by `/`, each a lowercase ASCII letter or digit followed by lowercase letters,
digits, `.`, `_` and `-` (`[a-z0-9][a-z0-9._-]*`). So an identifier is a file
name, a path in a URL and a registry key as it is, without escaping, on file
systems that ignore case too.

`revision` names one revision of those rules, such as `3`, `2026-11-16.1`,
`v1.2.0` or a version control hash: one to 128 characters, an ASCII letter or
digit followed by letters, digits, `.`, `_`, `+` and `-`
(`[A-Za-z0-9][A-Za-z0-9._+-]*`). Revisions are labels compared exactly: the
format gives them no order, and `v2` is not "after" `v1`.

Anything else is a `format_error` at the key. Both are compared exactly and
never normalized.

**Who assigns them.** The author of the bundle, whoever makes it: a person, a
release script, a registry. The core never invents an identifier or a
revision, and never reads a clock or a random source to make one (invariant
7): making a bundle twice from the same rules, identifier, revision, cases and
configuration gives the same bytes and the same digest.

**What "immutable" means.** A revision, once given to anyone, always names the
same rules: every bundle with the same `id` and `revision` has the same
`ruleset_digest`. The engine and the cases may change between bundles of one
revision (running the same rules under a newer engine is the first comparison
above). The core cannot enforce this across files, since it keeps no registry;
instead, any two bundles with the same `id` and `revision` and different rule
set digests are a **conflict** that every comparison reports (*Comparing
bundles*), and a registry or a comparator refuses. To change the rules, make a
new revision.

## Rules and their digest

The rules are the rule set under `ruleset`, or the decision table under
`decision_table`, read exactly as in their own documents, with the same keys,
construction rules, errors and limits, located under `/bundle/ruleset` or
`/bundle/decision_table`. The rule set a bundle gives (`json-format.md`, *A rule
set from a document*) is that rule set, or the one the table compiles to.

`ruleset_digest` is the rule set digest of that rule set (`json-format.md`,
*Rule set digests*): the digest of `{"ruleset":…,"version":1}`, so for a table,
of the rule set it compiles to. It is stated so that tools without an engine
can compare and index bundles, and a reader verifies it: one that is not the
digest of the rules the bundle holds is a `decision_mismatch` at
`/bundle/ruleset_digest`. A malformed digest is a `format_error` there
(`json-format.md`, *Digests*).

*Rationale.* `decision_mismatch` is the code that already says "this record
does not match the rules it is read with", for the same check on a decision's
`ruleset_digest` (`json-format.md`, *In decisions*); it is a `FormatError` in
every language, and reusing it needs no new error class, constant or sentinel.
The pointer says which record failed.

Decisions made by these rules can carry the same digest (`ruleset_digest` in
`json-format.md`, *Decisions*), so a log of decisions is tied to the bundle
that made them by a string comparison.

## Engine

| Key | Required | JSON type |
|---|---|---|
| `name` | yes | string: the engine's name, `[a-z0-9][a-z0-9._-]*`, at most 128 characters; `"cleanrules"` for this one |
| `version` | yes | string: the engine's exact version, a [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html) version such as `0.4.0` or `1.0.0-rc.1+build.5`, at most 128 characters |
| `limits` | yes | object: the limits it ran with (below) |

`version` is the version of the core (the `cleanrules` crate, which every
package of cleanrules shares). It is compared exactly, build metadata
included: reproducing a run means the same build. The format version is not
repeated: it is the document's own `version`, which a reader checks first.

`limits` records the configuration that can change a result, every key
required:

| Key | Limit (`json-format.md`, *Errors and limits*) |
|---|---|
| `max_size` | the largest JSON document, in bytes, such as an input given as JSON |
| `max_nesting` | the deepest nesting of arrays and objects |
| `max_depth` | the deepest condition or payload value |
| `max_path_segments` | the longest field path |
| `max_table_size` | the most cells and nodes of a decision table (`tables.md`, *Limits*) |
| `max_values` | the most values read from one input's native data (`data-model.md`, *Reading native data*) |

Each is an integer literal from 1 to 2⁵³ − 1; anything else is a
`format_error` at the limit. Writers write the keys in the order of the table.

*Why these, and only these.* What else decides a result is already in the
rules, under their digest: the hit policy, the aggregate, the logic and strict
mode are settings of the rule set (`json-format.md`, *Rule sets*), and
repeating them here would let a bundle contradict itself. The limits are not:
an input that one configuration evaluates, another refuses with
`limit_exceeded`. The limits of the YAML parser, of the rule syntax and of
diagrams bound the reading of texts and the drawing of pictures, not
decisions, and are left out. A later version adds a key here only for
configuration that changes results, omitted when it holds its default
(*Versioning*).

**Another engine.** A reader reads a bundle made by any engine and version,
and reports it (the `engine` of a bundle in Rust, Python and JavaScript, with
whether it is this engine, and *Comparing bundles*);
it never refuses one for being made elsewhere. Only the format version can
refuse a bundle: an unknown one is `unsupported_version`, as for every
document. Running the bundle's cases under another engine is the first
comparison above, and saying whether the results stayed the same is the work
of the comparison of results (T.4), not of the reader.

The engine and configuration of a bundle are the ones it was made with: an
engine that makes a bundle writes its own name and version, and the limits it
was given (the defaults when it was given none). Builds that are not releases
share the version of their source (`0.0.0` before the first release), so a
bundle made by one development build names every other one as the same
engine, and its contract is verified against theirs (*Contract*).

## Contract

| Key | Required | JSON type |
|---|---|---|
| `input` | yes | object: the input schema of the rules (`agents.md`, *Input schemas*) |
| `output` | yes | object: the outputs of the rules (below) |

`input` is the input schema the engine that made the bundle builds from the
rules' summary, as the `summary` tool gives it: a JSON Schema (draft 2020-12)
of the input the rules read. `output` holds `keys`, the payload keys any rule
may output (the summary's outputs, `explain.md`, *Summaries*), as an array of
strings without repetition, sorted by Unicode code point when written. The
types of the outputs are reserved for a later version: a payload value may be
a field or arithmetic, whose type the rules alone do not fix.

`input` is JSON, not a data value: it is kept as written, with these
restrictions, so that it has one canonical form: every number in it is an
integer literal from −(2⁵³ − 1) to 2⁵³ − 1 (input schemas only hold counts,
such as `minItems`), and anything else is a `format_error` at the number.

**Derived, stored and verified.** The contract is a function of the rules: an
engine derives it, and it is stored so that tools without an engine (an
editor, a registry, a client that validates its requests) can read it, and so
that comparing two bundles shows when the input contract changed. A reader
verifies it when the bundle names **this** engine, the same `name` and
`version`: it derives the contract from the rules and compares (JSON values,
objects without member order); a difference is a `decision_mismatch` at
`/bundle/contract/input` or `/bundle/contract/output`, input first, since
this engine could not have made that bundle. A bundle of another engine, or of
another version of this one, keeps its contract as recorded: the wording of
the descriptions in an input schema is behaviour of the core that may change
between versions (`agents.md`, *Versioning*), and a reader that refused such a
bundle would refuse to reproduce it.

*Rationale.* Deriving the contract on demand only would leave consumers
without an engine with nothing to read; a contract declared by hand could
contradict the rules. Verifying against the reader's own derivation catches a
contract edited by hand or left stale, wherever that check is meaningful.

## Cases

Each item of `cases` refers to one file of test cases:

| Key | Required | JSON type |
|---|---|---|
| `path` | yes | string: the file's path, relative to the directory of the bundle |
| `digest` | yes | string: the SHA-256 digest of the file's bytes, written as digests are (`json-format.md`, *Digests*) |

A `path` is one to 1024 characters: segments separated by `/`, each made of
lowercase ASCII letters, digits, `.`, `_` and `-`, and none ending with `.`
(so none is `.` or `..`). So it stays inside the bundle's directory, two
paths are two files on file systems that ignore case too, and Windows does
not drop a final dot. (Windows also reserves a few names, such as `con.json`;
a bundle meant for Windows avoids them.) Two items with the same `path` are a
`format_error` at the second one's `path`. The order of the items is kept: it
is the order in which they are run.

A case file holds test cases and their expectations (`tests.md`), in JSON
or YAML. The bundle does not read it: the digest covers the file's bytes as
stored, whatever their format and whether or not they read as a case file,
so a reader that has the file can check that it is the one the bundle was
made with, without reading it; reading and running the cases is the work of
the case runner, which checks each file against its digest first
(`tests.md`, *Case files of a bundle*). A reader of a bundle never opens files: the core
has no file system, and checking a file against its digest is the work of the
caller that has it
(`case_digest_matches` on a bundle in Rust and Python, `caseDigestMatches` in
JavaScript, and `digest_matches` or `digestMatches` on its `CaseFileRef`, the
reference to one case file, in Rust, Python and JavaScript; the C ABI and the
beta bindings give a bundle as a document only). Bytes as stored include line endings: a
version control system that rewrites them on checkout (`core.autocrlf` in
git) changes the file's digest, so case files are stored as they are (in
git, `-text` or `eol=lf` in `.gitattributes`).

## Digest of a bundle

A bundle has canonical bytes and a digest like every document
(`json-format.md`, *Canonical bytes*, *Digests*): those of
`{"bundle":…,"version":1}`, with the rules in their canonical form, `input`
with its members sorted and `output.keys` sorted. The digest of a bundle
identifies the whole package: two bundles with the same digest are the same
rules, identifier, revision, engine, configuration, contract and cases. It is
not stored in the bundle (a document never holds its own digest); a file name
or a registry may use it.

A bundle has two digests, then: its own, and its `ruleset_digest`, that of its
rules. Two bundles that hold the same rules (a decision table and the rule set
it compiles to, say) have the same `ruleset_digest`.

## Comparing bundles

Comparing two bundles, `a` and `b`, gives:

| Member | JSON type |
|---|---|
| `relation` | one of the relations below |
| `identical` | boolean: whether the two bundles have the same digest |
| `differences` | array of what differs, in the order below |

The **relation** looks only at the rules and the engine:

| Relation | When |
|---|---|
| `conflict` | the same `id` and `revision`, and different rule set digests: one revision names two sets of rules (*Identifier and revision*). It takes precedence over the others |
| `same_rules_same_engine` | the same rule set digest, and the same engine: `name`, `version` and every limit |
| `same_rules_other_engine` | the same rule set digest, and another engine or configuration |
| `other_rules_same_engine` | another rule set digest, and the same engine |
| `other_rules_other_engine` | another rule set digest, and another engine |

The **differences** name each part that differs, as these strings in this
order: `id`, `revision`, `rules` (the rule set digests differ), `source` (the
rule set digests are equal, but the documents that give them differ: a
decision table and the rule set it compiles to, or two tables that compile to
the same rule set), `engine.name`, `engine.version`, `engine.limits.<key>` for
each limit in the order of its table, `contract.input`, `contract.output` and
`cases` (the items differ, or their order). They are empty exactly when the
bundles are identical.

So `same_rules_other_engine` is the first comparison of this document and
`other_rules_same_engine` the second; `same_rules_same_engine` with only
`cases`, `id` or `revision` in the differences runs the same rules on the same
engine. Comparing is symmetric (comparing `b` with `a` gives the same result)
and uses nothing but the two bundles: not the engine that compares them.

"The same rules" means the same rule set digest, which covers every byte of
the rules' canonical form (`json-format.md`, *Digests*): names, descriptions,
tags, metadata and the names of actions too. Editing only a description, or
renaming a rule, makes "other rules", although the rules decide exactly as
before; the digest says that the documents differ, not that the decisions do.
Whether two versions decide alike on their cases is the work of the
comparison of results (T.4); to keep a renamed rule the same rule there, give
it an `id` (`rules.md`, *Identity*).

```json
{ "relation": "same_rules_other_engine", "identical": false, "differences": ["engine.version", "contract.input"] }
```

## Making a bundle

An engine makes a bundle from rules (a rule set, or a decision table), an
identifier, a revision, the bytes of each case file with its path, and limits:

1. it checks the paths, the identifier and the revision, as a reader does,
   and that no path is given twice (`format_error`; an implementation may
   check them in another order, and vectors compare only the code);
2. it writes the rules as their document would (so a rule set with an action
   given as code fails with `unsupported`, unless the caller passes
   `omit_actions`, as for digests: `json-format.md`, *Rule set digests*);
3. it records the rule set digest, its own name and version, the limits, the
   contract it derives, and the digest of each case file's bytes.

A limit of 0 (which a reader would refuse) or above 2⁵³ − 1 cannot be
recorded, and is `unsupported`.

## Reading

A bundle is read as every document is (`json-format.md`, *Errors and limits*):
size, nesting and syntax; `version`; then the shape in pre-order, the keys of
each object in the order of its table and then its unknown keys (a decision
table is read where it stands, as `tables.md` reads one); then the
construction rules of a rule set; then, in this order:

1. `ruleset_digest` against the rules (`decision_mismatch`);
2. when the bundle names this engine, `contract.input`, then `contract.output`
   against the contract the reader derives (`decision_mismatch`).

The shape errors include those of the key tables above: an unknown or missing
key, a wrong JSON type, an identifier, revision, engine name or version, path
or digest that does not have its form, a limit or a number out of range, a
repeated output key or case path (`format_error`). Reading never opens the
case files.
