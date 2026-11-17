// Bundles (spec/bundles.md): reproducible packages of rules, with their
// identifier and revision, their rule set digest, the engine and limits that
// made them, the contract of their input and output, and the digests of their
// files of test cases:
//
//   const bundle = new Bundle(rules, {
//     id: 'acme/discounts',
//     revision: '2026-11-16.1',
//     cases: [{ path: 'cases/vip.json', content: fs.readFileSync('cases/vip.json') }],
//   })
//   await dump(bundle, 'discounts.bundle.json')
//   const again = await load('discounts.bundle.json')
//   again.compare(bundle).relation  // 'same_rules_same_engine'
//
// The core makes, reads, writes, verifies and compares every bundle; this
// module checks the arguments, holds the bundle's document of the native
// layer, and gives what it reads as frozen JavaScript values. It is made once
// per build.

import { registerOpaque } from '../read.js'
import { brand, options, sameBuild, show } from './shared.js'

const INSPECT = Symbol.for('nodejs.util.inspect.custom')
const DENO_INSPECT = Symbol.for('Deno.customInspect')

/** Only the binding makes bundles from the core's. */
const NATIVE = Symbol('cleanrules.bundle')

const BUNDLE_OPTIONS = ['id', 'revision', 'cases', 'omitActions', 'limits']

/** The recorded limits, in the order of `spec/bundles.md`, by the binding's names. */
const LIMIT_NAMES = ['maxSize', 'maxNesting', 'maxDepth', 'maxPathSegments', 'maxTableSize', 'maxValues']

/** `value`, its arrays and objects frozen all the way down. */
function deepFreeze(value) {
  if (value !== null && typeof value === 'object') {
    for (const item of Object.values(value)) deepFreeze(item)
    Object.freeze(value)
  }
  return value
}

/** The bundles of one build (`build` names it). */
function bundles(native, rules, tables, build) {
  const { RuleSet, checkActions, fromNativeRuleSet, ruleSetNative } = rules
  const { DecisionTable, fromNativeTable, tableNative } = tables
  // The native bundle document of each `Bundle`.
  const natives = new WeakMap()

  /** The case files as the native layer takes them: an array of `{ path, content }`. */
  function caseList(cases) {
    if (cases === undefined || cases === null) return []
    if (typeof cases === 'string' || typeof cases[Symbol.iterator] !== 'function') {
      throw new TypeError(`cases must be an array of { path, content }, not ${show(cases)}`)
    }
    return Array.isArray(cases) ? cases : [...cases]
  }

  /** A bundle document of the core made from the constructor's arguments. */
  function make(held, bundleOptions) {
    const given = options(bundleOptions, BUNDLE_OPTIONS, "{ id: 'acme/discounts', revision: '1' }")
    const { id, revision, cases, omitActions = false, limits } = given
    if (typeof (omitActions ?? false) !== 'boolean') {
      throw new TypeError(`omitActions is a boolean, not ${show(omitActions)}`)
    }
    const list = caseList(cases)
    if (held instanceof RuleSet) {
      checkActions(held.rules, '/bundle/ruleset/rules/{}', omitActions ?? false)
      return native.bundleOfRuleset(ruleSetNative(held), id, revision, list, limits)
    }
    if (held instanceof DecisionTable) {
      return native.bundleOfTable(tableNative(held), id, revision, list, limits)
    }
    sameBuild(held, build)
    throw new TypeError(
      `a bundle holds a RuleSet or a DecisionTable, not ${show(held)}; give a single rule as a rule set of that rule: new RuleSet([rule])`,
    )
  }

  /**
   * A reproducible package of rules (`spec/bundles.md`); immutable. Made
   * from a rule set or a decision table with this engine, or read with
   * `loads` and `load`; written with `dumps` and `dump`, and identified by
   * `digest(bundle)`.
   */
  class Bundle {
    #native
    #data
    #rules
    #engine
    #contract
    #cases

    /**
     * @param held The rules: a `RuleSet` or a `DecisionTable`.
     * @param options `id` and `revision` (required; `spec/bundles.md`,
     *   *Identifier and revision*), `cases` (an iterable of `{ path, content
     *   }`, the content a `string` or a `Uint8Array`, whose SHA-256 digest is
     *   recorded), `omitActions` (make a bundle of rules whose actions are
     *   code, without them, instead of refusing) and `limits` (those the
     *   rules run with, which the bundle records: `maxSize`, `maxNesting`,
     *   `maxDepth`, `maxPathSegments`, `maxTableSize`, `maxValues`; the
     *   defaults for those left out).
     */
    constructor(held, bundleOptions) {
      const made = held === NATIVE ? bundleOptions : make(held, bundleOptions)
      this.#native = made
      this.#data = JSON.parse(native.bundleData(made))
      natives.set(this, made)
      Object.freeze(this)
    }

    /** The stable identifier of the rules, such as `'acme/discounts'`. */
    get id() {
      return this.#data[0]
    }

    /** The revision of the rules, such as `'2026-11-16.1'`. */
    get revision() {
      return this.#data[1]
    }

    /**
     * The rules the bundle holds: a `RuleSet` or a `DecisionTable`, as its
     * document holds them. Actions are their names, unbound (`RuleSet.bind`).
     */
    get rules() {
      if (this.#rules === undefined) {
        this.#rules =
          this.#data[2] === 'decision_table'
            ? fromNativeTable(native.bundleTable(this.#native))
            : fromNativeRuleSet(native.bundleRuleset(this.#native))
      }
      return this.#rules
    }

    /** The rule set the bundle gives: the one it holds, or the one its table compiles to. */
    get ruleset() {
      const held = this.rules
      return held instanceof DecisionTable ? held.ruleset : held
    }

    /** The rule set digest of its rules (`spec/json-format.md`, *Rule set digests*): `digest(bundle.ruleset)`. */
    get rulesetDigest() {
      return this.#data[3]
    }

    /**
     * The engine and configuration that made it (frozen): `name`, `version`
     * (exact), `limits` (the six it records, by the binding's names, as the
     * `limits` option takes them) and `isCurrent` (the same name and version
     * as this engine, whatever the limits).
     */
    get engine() {
      if (this.#engine === undefined) {
        const [name, version, values, isCurrent] = this.#data[4]
        const limits = Object.freeze(Object.fromEntries(LIMIT_NAMES.map((key, i) => [key, values[i]])))
        this.#engine = Object.freeze({ name, version, limits, isCurrent })
      }
      return this.#engine
    }

    /**
     * The contract of the rules (`spec/bundles.md`, *Contract*), in its JSON
     * form (frozen): `input`, a JSON Schema of the input, and `output.keys`,
     * the payload keys any rule may output, sorted.
     */
    get contract() {
      this.#contract ??= deepFreeze(this.#data[5])
      return this.#contract
    }

    /** The references to its case files, in order (frozen): each `{ path, digest }`. */
    get cases() {
      this.#cases ??= Object.freeze(this.#data[6].map(([path, digest]) => Object.freeze({ path, digest })))
      return this.#cases
    }

    /**
     * Whether `content` (a `string`, as UTF-8, or a `Uint8Array`) is the case
     * file at `path` the bundle was made with: whether its SHA-256 digest is
     * the one recorded. A `RangeError` for a path the bundle does not refer to.
     */
    caseMatches(path, content) {
      return native.bundleCaseMatches(this.#native, path, content)
    }

    /**
     * Compare with another bundle (`spec/bundles.md`, *Comparing bundles*)
     * (frozen): `relation` (`'conflict'`, `'same_rules_same_engine'`,
     * `'same_rules_other_engine'`, `'other_rules_same_engine'` or
     * `'other_rules_other_engine'`), `identical` (the same digest) and
     * `differences`, what differs, in the order of the specification.
     */
    compare(other) {
      if (!(other instanceof Bundle)) {
        sameBuild(other, build)
        throw new TypeError(`compare takes a Bundle, not ${show(other)}`)
      }
      const { relation, identical, differences } = JSON.parse(this.#native.compareBundle(natives.get(other)))
      return Object.freeze({ relation, identical, differences: Object.freeze(differences) })
    }

    /**
     * The bundle document of the JSON format (`spec/bundles.md`), for
     * `JSON.stringify` (`Expr.toJSON` says how); `dumps(bundle)` writes it
     * exactly.
     */
    toJSON() {
      return JSON.parse(native.stringifiable(this.#native.write('json')))
    }

    [INSPECT]() {
      const cases = this.cases.length === 1 ? '1 case file' : `${this.cases.length} case files`
      return `Bundle(${this.id} ${this.revision}: ${this.#data[2] === 'decision_table' ? 'decision table' : 'rule set'}, ${cases}, ${this.engine.name} ${this.engine.version})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  registerOpaque(Bundle)
  brand(build, Bundle)

  return {
    Bundle,
    /** The `Bundle` of a bundle document of the native layer, which it keeps. */
    fromNativeBundle: (made) => new Bundle(NATIVE, made),
    /** The bundle document of the native layer of a `Bundle`. */
    bundleNative: (bundle) => natives.get(bundle),
  }
}

export { bundles }
