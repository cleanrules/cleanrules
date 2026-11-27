// Test cases and the case runner (spec/tests.md), as Python's
// `cleanrules._cases`:
//
//   const cases = loads(text)  // a Cases
//   const run = rules.test([new CaseFile(cases, { path: 'cases/discounts.json' })])
//   run.allPassed
//   console.log(String(run))  // the core's text: each failure, explained
//
//   for (const result of rules.iterTest(files, { limits: { maxResults: 100 } })) {
//     console.log(result.status, result.id)  // one at a time, as the core gives them
//   }
//
// Three names, the same in every language: a `CaseFileRef` is a bundle's
// reference to a case file (`bundles.js`); a `Cases` is the content of a case
// file document; a `CaseFile` is one file given to a run, its path and its
// cases or the error that keeps it from being read. The core reads, checks,
// writes and runs every case; this module checks the arguments, encodes the
// values and builds the objects of the API of its build.

import { FormatError, fromNative } from '../errors.js'
import { fileSystem } from '../files.js'
import { decode } from '../output.js'
import { encodeData, registerOpaque } from '../read.js'
import { CaseResult, INTERNAL, TestRun, frozenData, testRunSummary } from './results.js'
import { brand, options, sameBuild, show } from './shared.js'

const INSPECT = Symbol.for('nodejs.util.inspect.custom')
const DENO_INSPECT = Symbol.for('Deno.customInspect')

/** Only the binding makes these objects from the core's. */
const NATIVE = Symbol('cleanrules.cases')

/** `CaseRun`'s own way to run the rest, for `test()`. */
const FINISH = Symbol('cleanrules.finish')

const EXPECTATION_OPTIONS = ['selected', 'output', 'outputs', 'unknown', 'error']
const CASE_OPTIONS = ['expect', 'description']
const CASES_OPTIONS = ['description', 'metadata']
const CASE_FILE_OPTIONS = ['path']
const READ_OPTIONS = ['path', 'format']
const TEST_OPTIONS = ['limits', 'cancel', 'signal', 'rulesetDigest']
const LIMIT_OPTIONS = ['maxResults', 'maxSteps', 'maxExplained']

/** Whether the runtime is on Windows, where a path may use `\\`. */
const WINDOWS = globalThis.process?.platform === 'win32' || globalThis.Deno?.build?.os === 'windows'

/** The path of a `file:` URL: Node's own reading where there is one, its decoded path otherwise. */
function filePath(url) {
  const toPath = globalThis.process?.getBuiltinModule?.('node:url')?.fileURLToPath
  return typeof toPath === 'function' ? toPath(url) : decodeURIComponent(url.pathname)
}

/** %IteratorPrototype%, which holds the iterator helpers where the runtime has them. */
const ITERATOR = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))

/** The message of reading a run from its own cancel callback. */
const READ_IN_CANCEL = 'a run cannot be read from its own cancel callback; return true to stop it'

const DATA_HINT = 'a case file holds data: null, booleans, numbers, strings, dates, arrays and objects'

/**
 * The first form in `form` that the core cannot read as data, a NaN or a
 * value whose reading threw, with where it is: `[form, at]`, or `null`.
 * Opaque values and numbers out of range are the core's to refuse, with its
 * own message (an input holds a number out of range as null).
 */
function unreadable(form, root) {
  const pending = [[form, root]]
  while (pending.length > 0) {
    const [item, at] = pending.pop()
    if (item === null || typeof item !== 'object') continue
    switch (item[0]) {
      case 'NaN':
      case 'E':
        return [item, at]
      case 'l':
        for (let i = item[1].length - 1; i >= 0; i -= 1) pending.push([item[1][i], `${at}[${i}]`])
        break
      case 'O':
        for (let i = item[1].length - 1; i >= 0; i -= 1) {
          const [key, value] = item[1][i]
          pending.push([value, `${at}[${JSON.stringify(key)}]`])
        }
        break
      default:
    }
  }
  return null
}

/** `value` in the reader's encoding; a `FormatError` for a NaN or a value whose reading threw. */
function wire(value, at, prefix) {
  const [form, thrown] = encodeData(value)
  const problem = unreadable(form, at)
  if (problem !== null) {
    const [bad, where] = problem
    if (bad[0] === 'NaN') throw new FormatError(`${prefix}${where} is NaN, which is not data`, { hint: DATA_HINT })
    throw new FormatError(`${prefix}reading ${where} threw ${bad[1]}`, { hint: DATA_HINT, cause: thrown[bad[2]] })
  }
  return form
}

/** Rule ids given as an iterable of strings, not one string; `undefined` when left out. */
function ids(value, name) {
  if (value === undefined || value === null) return undefined
  if (typeof value === 'string' || typeof value[Symbol.iterator] !== 'function') {
    throw new TypeError(`${name} is an array of rule ids, such as ['vip'], not ${show(value)}`)
  }
  const list = [...value]
  for (const id of list) {
    if (typeof id !== 'string') throw new TypeError(`${name} holds rule ids as strings, not ${show(id)}`)
  }
  return list
}

/** An object of payload keys: a plain object or a `Map`. */
function isMapping(value) {
  return value instanceof Map || (value !== null && typeof value === 'object' && !Array.isArray(value))
}

/** A text option: a string, or left out (`null`). */
function text(value, name) {
  if (value === undefined || value === null) return null
  if (typeof value !== 'string') throw new TypeError(`${name} is a string, not ${show(value)}`)
  return value
}

/** Strings as UTF-8 bytes; `Uint8Array`s as they are. */
function bytesOf(content, name) {
  if (typeof content === 'string') return new TextEncoder().encode(content)
  if (content instanceof Uint8Array) return content
  throw new TypeError(`${name} is a string or a Uint8Array, not ${show(content)}`)
}

/** A run limit: a whole number from 0, or left out. */
function limit(value, name) {
  if (value === undefined || value === null) return undefined
  if (typeof value !== 'number') throw new TypeError(`limits.${name} is a number, not ${show(value)}`)
  if (!Number.isInteger(value) || value < 0) {
    throw new RangeError(`limits.${name} is ${value}; a limit is a whole number from 0`)
  }
  return value
}

/** The cases of one build (`build` names it). */
function cases(native, rules, tables, bundles, build) {
  const { ruleSetNative } = rules
  const { tableNative } = tables
  const { bundleNative } = bundles

  /** A case file document of the core from the encoding of `cases_new` (`src/common/cases.rs`). */
  const casesNew = (content) => native.casesNew(JSON.stringify(content))

  /** A part of an expectation given as `null`, which only `output` takes. */
  function notNull(given, name) {
    if (given[name] === null) {
      throw new TypeError(`${name} is not checked when it is left out: leave it out instead of giving null`)
    }
  }

  /**
   * What evaluating a case's input must give (`spec/tests.md`,
   * *Expectations*); frozen. A part left out is not checked (`undefined`);
   * an empty expectation checks only that evaluation does not fail.
   */
  class Expectation {
    #json

    /**
     * @param parts `selected` (the effective ids of the rules expected to
     *   be selected, in selection order), `output` (an object whose keys the
     *   output has, with equal values, or `null`: nothing is selected),
     *   `outputs` (one such object per selected rule, in selection order),
     *   `unknown` (the ids of the rules expected to be unknown, in any order;
     *   kept sorted by code point) and `error` (the code evaluation must
     *   raise, alone: `'missing_field'`, `'type_mismatch'` or
     *   `'hit_policy_error'`). Floats that hold integers are given as
     *   `float(x)`. Leave out a part that is not checked: only `output`
     *   takes `null`.
     */
    constructor(parts, read) {
      if (parts === NATIVE) {
        const { json, ...rest } = read
        this.#json = json
        Object.assign(this, rest)
        Object.freeze(this)
        return
      }
      const given = options(parts, EXPECTATION_OPTIONS, "{ selected: ['vip'] }")
      for (const name of ['selected', 'outputs', 'unknown', 'error']) notNull(given, name)
      const content = {}
      const selected = ids(given.selected, 'selected')
      if (selected !== undefined) content.selected = selected
      if (given.output !== undefined) {
        if (given.output !== null && !isMapping(given.output)) {
          throw new TypeError(`output is an object of payload keys or null (nothing selected), not ${show(given.output)}`)
        }
        content.output = given.output === null ? null : wire(given.output, 'output', 'the expected ')
      }
      if (given.outputs !== undefined) {
        if (typeof given.outputs === 'string' || typeof given.outputs[Symbol.iterator] !== 'function') {
          throw new TypeError(`outputs is an array of objects, one per selected rule, not ${show(given.outputs)}`)
        }
        content.outputs = [...given.outputs].map((item, i) => {
          if (!isMapping(item)) throw new TypeError(`outputs holds objects, not ${show(item)}`)
          return wire(item, `outputs[${i}]`, 'the expected ')
        })
      }
      const unknown = ids(given.unknown, 'unknown')
      if (unknown !== undefined) content.unknown = unknown
      if (given.error !== undefined) content.error = text(given.error, 'error')
      const document = casesNew({ items: [{ id: '_', input: ['O', []], expect: content }] })
      try {
        const [, , [[, , , expect, json]]] = JSON.parse(native.casesData(document))
        const { json: fragment, ...rest } = expectationParts(expect, json)
        this.#json = fragment
        Object.assign(this, rest)
      } finally {
        document.free?.()
      }
      Object.freeze(this)
    }

    /** Whether nothing is checked but that evaluation does not fail. */
    get isEmpty() {
      return EXPECTATION_OPTIONS.every((key) => this[key] === undefined)
    }

    /**
     * The expectation as a case file writes it (`spec/tests.md`,
     * *Expectations*), for `JSON.stringify`: floats that hold integers as
     * `{"$float": n}`; `{}` when it checks nothing.
     */
    toJSON() {
      return JSON.parse(this.#json)
    }

    [INSPECT]() {
      const parts = EXPECTATION_OPTIONS.filter((key) => this[key] !== undefined)
      return `Expectation(${parts.join(', ')})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  /** The parts of an expectation as `cases_data` gives them, decoded, and its JSON form as text. */
  function expectationParts(expect, item) {
    return {
      /** The rules expected to be selected, in selection order (frozen), or `undefined`. */
      selected: expect.selected === undefined ? undefined : Object.freeze(expect.selected),
      /** The expected single output (frozen), `null` for nothing selected, or `undefined`. */
      output: !('output' in expect) ? undefined : expect.output === null ? null : frozenData(decode(expect.output, [], true)),
      /** The expected outputs (frozen), or `undefined`. */
      outputs: expect.outputs === undefined ? undefined : Object.freeze(expect.outputs.map((form) => frozenData(decode(form, [], true)))),
      /** The rules expected to be unknown, sorted by code point (frozen), or `undefined`. */
      unknown: expect.unknown === undefined ? undefined : Object.freeze(expect.unknown),
      /** The code of the error expected, or `undefined`. */
      error: expect.error,
      json: JSON.stringify(item.expect ?? {}),
    }
  }

  /** An expectation's content in the encoding of `cases_new`. */
  function expectationWire(expect) {
    const content = {}
    if (expect.selected !== undefined) content.selected = expect.selected
    if (expect.output !== undefined) content.output = expect.output === null ? null : wire(expect.output, 'output', 'the expected ')
    if (expect.outputs !== undefined) content.outputs = expect.outputs.map((item, i) => wire(item, `outputs[${i}]`, 'the expected '))
    if (expect.unknown !== undefined) content.unknown = expect.unknown
    if (expect.error !== undefined) content.error = expect.error
    return content
  }

  /**
   * One test case (`spec/tests.md`, *Cases*): an id, an input and an
   * expectation; frozen.
   */
  class Case {
    #json

    /**
     * @param id Names the case, unique in its file: any non-empty string,
     *   compared exactly.
     * @param input The data the rules evaluate, as native data (`float(x)`
     *   for a float that holds an integer, `decimal`, `date`...). An int out
     *   of range is null, as in a case file read from JSON.
     * @param options `expect`, an `Expectation` (by default, only that
     *   evaluation does not fail), and `description`.
     */
    constructor(id, input, caseOptions) {
      if (id === NATIVE) {
        const { json, ...rest } = input
        this.#json = json
        Object.assign(this, rest)
        Object.freeze(this)
        return
      }
      if (typeof id !== 'string') throw new TypeError(`a case id is a string, not ${show(id)}`)
      const given = options(caseOptions, CASE_OPTIONS, '{ expect: new Expectation({ selected: [\'vip\'] }) }')
      if (given.expect !== undefined && given.expect !== null && !(given.expect instanceof Expectation)) {
        sameBuild(given.expect, build)
        throw new TypeError(`expect is an Expectation, not ${show(given.expect)}`)
      }
      const description = text(given.description, 'description')
      if (input === undefined) throw new TypeError('the input is undefined; pass the data to evaluate (null for none)')
      const item = { id, input: wire(input, 'input', `in the case ${JSON.stringify(id)}, `) }
      if (description !== null) item.description = description
      if (given.expect !== undefined && given.expect !== null) item.expect = expectationWire(given.expect)
      const document = casesNew({ items: [item] })
      try {
        const [, , [data]] = JSON.parse(native.casesData(document))
        const { json, ...rest } = caseParts(data)
        this.#json = json
        Object.assign(this, rest)
      } finally {
        document.free?.()
      }
      Object.freeze(this)
    }

    /**
     * The case as a case file writes it (`spec/tests.md`, *Cases*), for
     * `JSON.stringify`: floats that hold integers as `{"$float": n}`.
     */
    toJSON() {
      return JSON.parse(this.#json)
    }

    [INSPECT]() {
      return `Case(${JSON.stringify(this.id)})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  /** The parts of a case as `cases_data` gives them, decoded, and its JSON form as text. */
  function caseParts([id, description, input, expect, json]) {
    return {
      /** The id. */
      id,
      /** The input as the core holds it, as native data (frozen). */
      input: frozenData(decode(input, [], true)),
      /** The expectation. */
      expect: new Expectation(NATIVE, expectationParts(expect, json)),
      /** The description, or `null`. */
      description,
      json: JSON.stringify(json),
    }
  }

  /** A case's content in the encoding of `cases_new`. */
  function caseWire(item) {
    const content = { id: item.id, input: wire(item.input, 'input', `in the case ${JSON.stringify(item.id)}, `) }
    if (item.description !== null) content.description = item.description
    const expect = expectationWire(item.expect)
    if (Object.keys(expect).length > 0) content.expect = expect
    return content
  }

  // The native case file document of each `Cases`.
  const natives = new WeakMap()

  /**
   * The content of a case file document (`spec/tests.md`, *Case files*):
   * its cases in the order they run, with ids unique; frozen and iterable.
   * Read one with `loads` and `load` (JSON or YAML) and write it with `dumps`
   * and `dump`; `canonicalBytes` and `digest` take it too.
   */
  class Cases {
    #byId

    /**
     * @param items The cases, in order: `Case` objects.
     * @param options `description` and `metadata` (data about the file, as a
     *   rule's metadata).
     */
    constructor(items, casesOptions) {
      let document
      if (items === NATIVE) {
        document = casesOptions
      } else {
        if (items === undefined || items === null) items = []
        if (typeof items === 'string' || typeof items[Symbol.iterator] !== 'function') {
          throw new TypeError(`cases are an array of Case objects, not ${show(items)}`)
        }
        const list = [...items]
        for (const item of list) {
          if (!(item instanceof Case)) {
            sameBuild(item, build)
            throw new TypeError(`Cases holds Case objects, not ${show(item)}`)
          }
        }
        const given = options(casesOptions, CASES_OPTIONS, "{ description: 'discounts' }")
        const content = { items: list.map(caseWire) }
        const description = text(given.description, 'description')
        if (description !== null) content.description = description
        if (given.metadata !== undefined && given.metadata !== null) {
          if (!isMapping(given.metadata)) throw new TypeError(`metadata is an object, not ${show(given.metadata)}`)
          content.metadata = wire(given.metadata, 'metadata', "the case file's ")
        }
        document = casesNew(content)
      }
      natives.set(this, document)
      const [description, metadata, data] = JSON.parse(native.casesData(document))
      /** What the file holds, or `null`. */
      this.description = description
      /** Data about the file (frozen), `{}` when it has none. */
      this.metadata = frozenData(decode(metadata, [], true))
      /** The cases, in order (frozen). */
      this.items = Object.freeze(data.map((item) => new Case(NATIVE, caseParts(item))))
      this.#byId = new Map(this.items.map((item) => [item.id, item]))
      Object.freeze(this)
    }

    /** How many cases there are. */
    get length() {
      return this.items.length
    }

    /** The case with the id `id`, compared exactly, or `undefined`. */
    get(id) {
      return this.#byId.get(id)
    }

    [Symbol.iterator]() {
      return this.items[Symbol.iterator]()
    }

    /**
     * The case file document of the JSON format (`spec/tests.md`), for
     * `JSON.stringify` (`Expr.toJSON` says how): floats that hold integers
     * are written `{"$float": 1}`, as `JSON.stringify` would lose them;
     * `dumps(cases)` writes it exactly.
     */
    toJSON() {
      return JSON.parse(native.stringifiable(natives.get(this).write('json')))
    }

    [INSPECT]() {
      const about = this.description === null ? '' : ` ${JSON.stringify(this.description)}`
      return `Cases(${about.trim()}${about === '' ? '' : ': '}${this.items.length} case${this.items.length === 1 ? '' : 's'})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  // The native case file of each `CaseFile`.
  const files = new WeakMap()

  /**
   * One case file given to a run (`spec/tests.md`, *Running cases*): its
   * path, when it has one, the digest of the bytes it was read from, when
   * it was, and its cases or the error of reading it, which running it
   * gives as its one result; frozen.
   */
  class CaseFile {
    /**
     * @param held The cases, already read or built: a `Cases`.
     * @param options `path`, the file's path, with `/` between segments.
     */
    constructor(held, fileOptions) {
      let made
      let cases = null
      if (held === NATIVE) {
        made = fileOptions
      } else {
        if (!(held instanceof Cases)) {
          sameBuild(held, build)
          throw new TypeError(`a CaseFile holds Cases, not ${show(held)}`)
        }
        const path = text(options(fileOptions, CASE_FILE_OPTIONS, "{ path: 'cases/discounts.json' }").path, 'path')
        made = native.caseFileOf(natives.get(held), path)
        cases = held
      }
      files.set(this, made)
      const [path, digest, error] = JSON.parse(made.data())
      /** The path, or `null`. */
      this.path = path
      /** The SHA-256 digest of the bytes it was read from, or `null`. */
      this.digest = digest
      if (cases === null && error === null) {
        cases = new Cases(NATIVE, made.cases())
      }
      /** The cases, or `null` for a file that cannot be read. */
      this.cases = cases
      /** The error of reading it (a `CleanRulesError`), or `null`. */
      this.error = error === null ? null : fromNative(error, null, null)
      Object.freeze(this)
    }

    /**
     * The case file whose bytes are `content` (a string, as UTF-8, or a
     * `Uint8Array`), read under the default limits: as YAML when `path`
     * ends in `.yaml` or `.yml` (in any case) and as JSON otherwise, unless
     * `format` (`'json'` or `'yaml'`) says. An error reading it is kept in
     * `error`, not thrown: running the file gives it as its one result.
     */
    static read(content, readOptions) {
      const { path, format } = options(readOptions, READ_OPTIONS, "{ path: 'cases/vip.json' }")
      if (format !== undefined && format !== null && format !== 'json' && format !== 'yaml') {
        throw new RangeError(`a case file is JSON or YAML, not ${show(format)}`)
      }
      return new CaseFile(NATIVE, native.caseFileRead(text(path, 'path'), bytesOf(content, 'content'), format ?? null))
    }

    /**
     * The case file at `path` (a string or a `file:` URL), read from disk as
     * `read` reads it: a promise. Its path in the results is the file's
     * path, with `/` between segments. In Node and Deno; browsers have no
     * files (`UnsupportedError`: use `read`). A file that cannot be opened
     * rejects: it is the caller's error, not a result.
     */
    static async load(path) {
      const fs = fileSystem('CaseFile.load')
      const name = typeof path === 'string' ? path : path instanceof URL && path.protocol === 'file:' ? filePath(path) : null
      if (name === null) throw new TypeError(`CaseFile.load takes a path (a string or a file: URL), not ${show(path)}`)
      return CaseFile.read(await fs.read(path), { path: WINDOWS ? name.split('\\').join('/') : name })
    }

    /** The case with the id `id`, when the file was read and has it; otherwise `undefined`. */
    case(id) {
      return this.cases?.get(id)
    }

    [INSPECT]() {
      const where = this.path ?? 'without a path'
      return this.cases === null ? `CaseFile(${where}: unreadable)` : `CaseFile(${where}: ${this.cases.length} case${this.cases.length === 1 ? '' : 's'})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  registerOpaque(Expectation, Case, Cases, CaseFile)
  brand(build, Expectation, Case, Cases, CaseFile)

  /** The native case files of a run: `CaseFile`s and `Cases` (a file without a path). */
  function nativeFiles(given) {
    if (given instanceof CaseFile || given instanceof Cases) given = [given]
    if (given === undefined || given === null || typeof given === 'string' || typeof given[Symbol.iterator] !== 'function') {
      throw new TypeError(`case files are an array of CaseFile or Cases objects, not ${show(given)}`)
    }
    return [...given].map((item) => {
      if (item instanceof CaseFile) return files.get(item)
      if (item instanceof Cases) return native.caseFileOf(natives.get(item), null)
      sameBuild(item, build)
      const hint = typeof item === 'string' ? ': read a path with await CaseFile.load(path)' : ''
      throw new TypeError(`a case file is a CaseFile or Cases, not ${show(item)}${hint}`)
    })
  }

  /** A run's options, checked: `[limits, cancel, rulesetDigest]`. */
  function runOptions(testOptions) {
    const given = options(testOptions, TEST_OPTIONS, '{ limits: { maxResults: 100 } }')
    let limits = {}
    if (given.limits !== undefined && given.limits !== null) {
      limits = options(given.limits, LIMIT_OPTIONS, '{ maxResults: 100 }')
    }
    const checked = Object.fromEntries(LIMIT_OPTIONS.map((name) => [name, limit(limits[name], name)]))
    const { cancel, signal, rulesetDigest = false } = given
    if (cancel !== undefined && cancel !== null && typeof cancel !== 'function') {
      throw new TypeError(`cancel is a function, not ${show(cancel)}`)
    }
    if (signal !== undefined && signal !== null && (typeof signal !== 'object' || typeof signal.aborted !== 'boolean')) {
      throw new TypeError(`signal is an AbortSignal, not ${show(signal)}`)
    }
    if (typeof (rulesetDigest ?? false) !== 'boolean') {
      throw new TypeError(`rulesetDigest is a boolean, not ${show(rulesetDigest)}`)
    }
    const asks = [cancel, signal === undefined || signal === null ? null : () => signal.aborted].filter(
      (ask) => ask !== undefined && ask !== null,
    )
    return [checked, asks.length === 0 ? null : () => asks.some((ask) => Boolean(ask())), rulesetDigest ?? false]
  }

  /**
   * A run in progress (`spec/tests.md`, *Batches and interruption*): an
   * iterator of its results (`CaseResult`), one at a time, in run order,
   * each given before the next case is evaluated; frozen. It ends after its
   * last result, or when a limit or the caller stops it: `cancel` (asked
   * between results, never while a case is evaluated), `signal` (seen only
   * when the caller awaits between results: a synchronous loop never lets
   * it be aborted), `cancel()`, or leaving a `for...of` loop early, which
   * calls `return()` and stops it at once. (In Python, leaving a loop leaves
   * the run where it is, until `test_run()`.) A run that stopped early is
   * partial; `testRun()` gives its test run, made once, which holds every
   * result given. The `cancel` callback must not read the run: while it is
   * asked, `summary`, `remaining`, `isFinished`, `testRun()` and `next()`
   * throw a `TypeError`, and `cancel()` asks the run to stop, as returning
   * true does.
   */
  class CaseRun {
    #native
    #cancel
    #raised = undefined
    #busy = false
    #stop = false
    #report = null

    constructor(token, made, cancel) {
      if (token !== NATIVE) throw new TypeError('CaseRun has no public constructor; use RuleSet.iterTest')
      this.#native = made
      this.#cancel = cancel
      Object.freeze(this)
    }

    /**
     * The answer to the core's question, which never throws in native code:
     * stop when `cancel()` asked it or `cancel` says so; what `cancel` throws
     * is kept, and stops the run.
     */
    #ask = () => {
      if (this.#stop) return true
      try {
        const answer = this.#cancel !== null && Boolean(this.#cancel())
        return answer || this.#stop
      } catch (error) {
        this.#raised = { error }
        return true
      }
    }

    #idle() {
      if (this.#busy) throw new TypeError(READ_IN_CANCEL)
    }

    /** One step of the native run, or all of them, the run marked busy meanwhile. */
    #advance(finish) {
      this.#idle()
      this.#busy = true
      let data
      try {
        // Without a callback, the native run asks nothing of JavaScript.
        const ask = this.#cancel === null ? null : this.#ask
        if (finish) this.#native.finish(ask)
        else data = this.#native.next(ask)
      } finally {
        this.#busy = false
      }
      if (this.#raised !== undefined) {
        const { error } = this.#raised
        this.#raised = undefined
        throw error
      }
      return data
    }

    /** The iterator protocol: `{ value, done }`. What `cancel` throws stops the run and is thrown. */
    next() {
      const data = this.#advance(false)
      if (data === null || data === undefined) return { value: undefined, done: true }
      return { value: new CaseResult(INTERNAL, data), done: false }
    }

    /** Run the rest without giving the results one at a time (`test()`). */
    [FINISH]() {
      this.#advance(true)
    }

    /** Leaving a `for...of` loop early: the run stops, partial (cancelled). */
    return() {
      this.cancel()
      return { value: undefined, done: true }
    }

    [Symbol.iterator]() {
      return this
    }

    /**
     * Stop the run now: it gives no more results, and is partial when a
     * result remained. Called from the run's own `cancel` callback, it stops
     * the run as returning true would.
     */
    cancel() {
      if (this.#busy) {
        this.#stop = true
        return
      }
      this.#native.stop()
    }

    /** The summary of the results given so far (frozen), as `TestRun.summary`. */
    get summary() {
      this.#idle()
      return testRunSummary(JSON.parse(this.#native.summary()))
    }

    /** The results not given yet; 0 once the run is finished. */
    get remaining() {
      this.#idle()
      return this.#native.remaining()
    }

    /** Whether the run gives no more results: complete, or interrupted. */
    get isFinished() {
      this.#idle()
      return this.#native.finished()
    }

    /**
     * The test run: its results and summary, the same object each time. A
     * run that is not finished is stopped first, and is then partial
     * (`cancelled`): a run is never reported complete before its last result.
     */
    testRun() {
      this.#idle()
      if (this.#report === null) this.#report = new TestRun(INTERNAL, this.#native.testRun())
      return this.#report
    }

    [INSPECT]() {
      if (this.#busy) return 'CaseRun(running)'
      const state = this.isFinished ? 'finished' : `${this.remaining} remaining`
      return `CaseRun(${this.summary.total} results, ${state})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  // The iterator helpers (`run.take(10)`...), where the runtime has them.
  Object.setPrototypeOf(CaseRun.prototype, ITERATOR)
  registerOpaque(CaseRun)
  brand(build, CaseRun)

  /** A run of `files` against `held` (`'ruleset'`, `'table'` or `'bundle'`). */
  function iterTest(kind, held, given, testOptions) {
    const [limits, cancel, rulesetDigest] = runOptions(testOptions)
    const args = [limits.maxResults ?? null, limits.maxSteps ?? null, limits.maxExplained ?? null, rulesetDigest]
    let made
    if (kind === 'bundle') {
      made = native.runBundle(bundleNative(held), ...args)
      made.addBundleFiles(bundleLoader(held, given))
    } else {
      const list = nativeFiles(given)
      made = kind === 'ruleset' ? native.runRuleset(ruleSetNative(held), ...args) : native.runTable(tableNative(held), ...args)
      for (const file of list) made.add(file)
    }
    return new CaseRun(NATIVE, made, cancel)
  }

  /** Every case of a run, then its test run. */
  function test(kind, held, given, testOptions) {
    const run = iterTest(kind, held, given, testOptions)
    run[FINISH]()
    return run.testRun()
  }

  /**
   * What loads the case files of `bundle` for the core, which picks the
   * bound and checks each file's digest: `read` gives each `CaseFileRef`'s
   * bytes, synchronously.
   */
  function bundleLoader(bundle, read) {
    if (typeof read !== 'function') {
      throw new TypeError(`a bundle's case files are read by a function given each CaseFileRef, not ${show(read)}`)
    }
    const refs = new Map(bundle.cases.map((ref) => [ref.path, ref]))
    return (path) => {
      const content = read(refs.get(path))
      if (content !== null && typeof content === 'object' && typeof content.then === 'function') {
        throw new TypeError(`the reader gave a promise for ${path}: it reads synchronously; read the files first`)
      }
      return bytesOf(content, `the content of ${path}`)
    }
  }

  return {
    Case,
    CaseFile,
    CaseRun,
    Cases,
    Expectation,
    /** The `Cases` of a case file document of the native layer, which it keeps. */
    fromNativeCases: (document) => new Cases(NATIVE, document),
    /** The case file document of the native layer of a `Cases`. */
    casesNative: (held) => natives.get(held),
    iterTest,
    test,
  }
}

export { cases }
