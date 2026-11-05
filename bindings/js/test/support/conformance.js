// The JavaScript conformance runner (conformance/README.md, *Runners*): every
// vector of conformance/vectors runs twice, as in each binding.
//
// - `json:<id>`: documents, inputs and results cross as JSON text through the
//   native layer (`cleanrules/native`): `Document`, `RuleSet`, `Node` and
//   `Decision`, with `read`, `write`, `readDecision`, `evaluateJson`,
//   `evaluateManyJson`, `evaluateTreeJson`, `explain` and `summary`; decisions
//   are also checked with the native layer's readers (`ruleName`, `firedAt`,
//   `truth`, `selected`, `ruleOutputJson`, `outputsJson`, `outputJson`).
// - `native:<id>`: the idiomatic API of the root entry (`cleanrules`).
//   Conditions, rules and rule sets are built in code (`field`, `constant`,
//   `and`, `or`, `not`, the methods of `Expr`, `Rule`, `RuleSet`), checked
//   against the core's reading of the same JSON (`dumps`), and inputs are
//   native JavaScript data converted from the vector (`bigint` beyond 2⁵³,
//   `float(x)` for floats holding integers, `decimal`, `date`, `datetime`,
//   `Date`, `Temporal` and maps as `Map` where a plain object would lose
//   their order or keys). What comes back is compared as the values given
//   back to JavaScript (`spec/data-model.md`, *Values given back*): decisions
//   through `results`, `selected`, `outputs`, `output` and `aggregated`,
//   result trees, explanations and summaries through their classes. The same
//   native data also goes through the native layer (`RuleSet.evaluate`,
//   `Node.evaluateTree`), whose JSON is compared exactly, so the reading of
//   native data is checked to the last digit. `read` vectors go through
//   `loads`, `dumps`, `ruleFromText`, `DecisionTable` (grids) and
//   `RuleSet.readDecision`.
//
// The runner only drives the library and compares: results are parsed with
// ./json.js and compared with the vectors as trees, never with the library's
// own JSON parser or equality.

import { Num, Obj, jsOrder, parse, same, tree, write } from './json.js'

/** Vectors are only ever added: the count when this runner was written. */
export const MINIMUM_VECTORS = 2508

/** Why a vector does not run one way: each is counted and listed. */
export const SKIPS = {
  grid: 'grids are native data: cleanrules/native reads documents only, so table_grid vectors run in the native route (DecisionTable)',
}

/** The class of each error code (`spec/errors.md`, *Codes*). */
const CLASSES = {
  format_error: 'FormatError',
  unsupported_version: 'FormatError',
  limit_exceeded: 'FormatError',
  decision_mismatch: 'FormatError',
  expression_error: 'ExpressionError',
  rule_definition_error: 'RuleDefinitionError',
  rule_syntax_error: 'RuleSyntaxError',
  registry_error: 'RegistryError',
  rule_not_found: 'RuleNotFoundError',
  evaluation_error: 'EvaluationError',
  missing_field: 'MissingFieldError',
  type_mismatch: 'TypeMismatchError',
  hit_policy_error: 'HitPolicyError',
  action_error: 'ActionError',
  unsupported: 'UnsupportedError',
}

/** The fields of an evaluation error that are compared (`spec/errors.md`, *Comparing errors*). */
const EVALUATION = ['code', 'rule', 'path', 'types', 'rules', 'row']
/** The context fields an error may carry, as `toJSON()` names them. */
const CONTEXT = ['pointer', 'line', 'column', 'name', 'rule', 'path', 'types', 'rules', 'row']
/** The vector's limits, by the names of the native layer. */
const LIMITS = { max_size: 'maxSize', max_nesting: 'maxNesting', max_depth: 'maxDepth', max_table_size: 'maxTableSize' }

const MAX_SAFE = 2n ** 53n - 1n

/** A vector the runner cannot run as written, or an answer that breaks the API's promises. */
class VectorError extends Error {}

/** A step's outcome: `{ result }`, or `{ error }` for a `CleanRulesError`. */
function attempt(R, step) {
  try {
    return { result: step() }
  } catch (error) {
    if (error instanceof R.CleanRulesError) return { error }
    throw error
  }
}

/** Members of `o` named in `keys`, in `o`'s order. */
const only = (o, keys) => new Obj(o.entries.filter(([k]) => keys.includes(k)))

function sameMembers(actual, wanted) {
  return (
    actual.entries.length === wanted.entries.length &&
    wanted.entries.every(([k, v]) => actual.has(k) && same(actual.get(k), v))
  )
}

const need = (o, key) => {
  if (!(o instanceof Obj) || !o.has(key)) throw new VectorError(`no \`${key}\` in ${write(o)}`)
  return o.get(key)
}

/** The content of a document the library wrote, such as its `decision`. */
const content = (text, key) => need(parse(text), key)

/** An int of the format (`Num` or `{"$int": ...}`) as a `number` or a `bigint`. */
function intOf(text) {
  const n = BigInt(text)
  return n >= -MAX_SAFE && n <= MAX_SAFE ? Number(n) : n
}

/** Days from 1970-01-01 to a day of the proleptic Gregorian calendar (Hinnant's days_from_civil). */
function daysFromCivil(y, m, d) {
  const year = m <= 2 ? y - 1 : y
  const era = Math.floor(year / 400)
  const yoe = year - era * 400
  const doy = Math.floor((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5) + d - 1
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy
  return era * 146097 + doe - 719468
}

const DATETIME = /^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(?:\.(\d{1,6}))?(Z|[+-]\d\d:\d\d(?::\d\d)?)?(?:\[([^\]]+)\])?$/

/** A `$datetime` text in parts: local fields, microseconds, offset in seconds (or `null`), zone. */
function datetimeParts(text) {
  const m = DATETIME.exec(text)
  if (m === null) throw new VectorError(`the datetime ${text} cannot be read`)
  const [year, month, day, hour, minute, second] = m.slice(1, 7).map(Number)
  const microsecond = Number((m[7] ?? '').padEnd(6, '0'))
  let offset = null
  if (m[8] === 'Z') offset = 0
  else if (m[8] !== undefined) {
    const [h, mi, s = '0'] = m[8].slice(1).split(':')
    offset = (m[8][0] === '-' ? -1 : 1) * (Number(h) * 3600 + Number(mi) * 60 + Number(s))
  }
  return { year, month, day, hour, minute, second, microsecond, offset, zone: m[9] ?? null, mark: m[8] }
}

/** The instant of an aware datetime, in microseconds since the epoch. */
function epochMicros(p) {
  const seconds = BigInt(daysFromCivil(p.year, p.month, p.day)) * 86400n + BigInt(p.hour * 3600 + p.minute * 60 + p.second - p.offset)
  return seconds * 1000000n + BigInt(p.microsecond)
}

const pad = (n, width) => String(n).padStart(width, '0')

/** A datetime's text as the JSON format writes it (`spec/json-format.md`, *Dates and datetimes*). */
function datetimeText({ year, month, day, hour, minute, second, microsecond }, offset, zone) {
  let text = `${pad(year, 4)}-${pad(month, 2)}-${pad(day, 2)}T${pad(hour, 2)}:${pad(minute, 2)}:${pad(second, 2)}`
  if (microsecond !== 0) text += `.${pad(microsecond, 6)}`
  if (offset === null) return text
  if (offset === 0 && zone === null) return `${text}Z`
  const size = Math.abs(offset)
  text += `${offset < 0 ? '-' : '+'}${pad(Math.floor(size / 3600), 2)}:${pad(Math.floor((size % 3600) / 60), 2)}`
  if (size % 60 !== 0) text += `:${pad(size % 60, 2)}`
  return zone === null ? text : `${text}[${zone}]`
}

/** The text of a finite float, with a fraction or an exponent. */
function floatText(x) {
  if (Object.is(x, -0)) return '-0.0'
  const text = String(x)
  return /[.e]/.test(text) ? text : `${text}.0`
}

/**
 * What `JSON.stringify` writes of a `toJSON()` object for a tree of the
 * format (`spec/json-format.md`, *Values*: a writer that cannot write `1.0`
 * uses `{"$float": 1}`): each float that holds an integer as `{"$float": n}`,
 * `n` as JavaScript writes the number (so a zero loses its sign).
 */
function stringified(tree) {
  if (Array.isArray(tree)) return tree.map(stringified)
  if (tree instanceof Obj) return new Obj(tree.entries.map(([k, v]) => [k, stringified(v)]))
  if (!(tree instanceof Num) || tree.isInteger) return tree
  const x = Number(tree.text)
  return Number.isInteger(x) || Object.is(x, -0) ? new Obj([['$float', new Num(String(x))]]) : tree
}

/** A tree with each float zero made positive: what `JSON.stringify` keeps of a `-0.0`. */
function unsignedZeros(tree) {
  if (Array.isArray(tree)) return tree.map(unsignedZeros)
  if (tree instanceof Obj) return new Obj(tree.entries.map(([k, v]) => [k, unsignedZeros(v)]))
  return tree instanceof Num && !tree.isInteger && Object.is(Number(tree.text), -0) ? new Num('0.0') : tree
}

/**
 * The runner over one build: `R` is its root entry (`cleanrules`) and `N` its
 * native layer (`cleanrules/native`).
 */
export function conformance(R, N) {
  const Temporal = globalThis.Temporal
  /** The vectors whose native input could not be represented exactly, by why. */
  const fallbacks = new Map()

  /** The vectors whose native data used each JavaScript form that has no JSON equivalent. */
  const forms = new Map()
  let current = ''

  function fallback(reason) {
    const ids = fallbacks.get(reason) ?? new Set()
    ids.add(current)
    fallbacks.set(reason, ids)
  }

  /** Count the form of a native value made from the vector. */
  function noteForm(value) {
    let form = null
    if (typeof value === 'bigint') form = 'bigint'
    else if (value instanceof Map) form = 'Map'
    else if (value instanceof Date) form = 'Date'
    else if (value instanceof R.ZonedDateTime) form = 'ZonedDateTime'
    else if (value instanceof R.Float) form = 'float(x)'
    else if (value instanceof R.Decimal) form = 'decimal(text)'
    else if (value instanceof R.LocalDate) form = 'date(...)'
    else if (value instanceof R.NaiveDateTime) form = 'datetime(...)'
    else if (value !== null && typeof value === 'object' && String(value[Symbol.toStringTag]).startsWith('Temporal.')) {
      form = value[Symbol.toStringTag]
    }
    if (form !== null) {
      const ids = forms.get(form) ?? new Set()
      ids.add(current)
      forms.set(form, ids)
    }
    return value
  }

  // --- Errors ----------------------------------------------------------------

  function checkClass(error) {
    const name = CLASSES[error.code]
    if (name !== undefined && !(error instanceof R[name])) {
      throw new VectorError(`the error ${error.code} is a ${error.constructor.name}, not a ${name}`)
    }
    if (error.code === 'missing_field' || error.code === 'type_mismatch' || error.code === 'hit_policy_error') {
      if (!(error instanceof R.EvaluationError)) throw new VectorError(`${error.code} is not an EvaluationError`)
    }
  }

  /** An error's JSON form, as the core writes it (`toJSON()`). */
  function jsonForm(error) {
    checkClass(error)
    const form = error.toJSON()
    return tree(Object.fromEntries(['code', ...CONTEXT].filter((key) => key in form).map((key) => [key, form[key]])))
  }

  /** An error's code and context fields, as the idiomatic API gives them (`code`, `context`). */
  function contextForm(error) {
    checkClass(error)
    const entries = [['code', error.code]]
    for (const key of CONTEXT) {
      const value = error.context[key]
      if (value === undefined || value === null || (Array.isArray(value) && value.length === 0)) continue
      entries.push([key, tree(value)])
    }
    return new Obj(entries)
  }

  function compareError(actual, wanted, read) {
    const keys = read ? wanted.entries.map(([k]) => k) : EVALUATION
    const got = only(actual, keys)
    const want = only(wanted, keys)
    return sameMembers(got, want) ? '' : `error ${write(got)}, expected ${write(want)}`
  }

  /**
   * Compare what a step gave with the vector's `key`, or with its error.
   * `form` makes the error's tree; `mode` is the comparison's (`json.js`).
   */
  function compare(outcome, expect, key, { read = false, form = jsonForm, mode = {}, view = (x) => x } = {}) {
    const error = expect.get('error')
    if (outcome.error === undefined && error === undefined) {
      const want = view(need(expect, key))
      if (same(outcome.result, want, mode)) return ''
      return `\n  got  ${write(outcome.result)}\n  want ${write(want)}`
    }
    if (outcome.error === undefined) return `got ${write(outcome.result)}, expected the error ${write(error)}`
    if (error === undefined) return `unexpected error ${write(form(outcome.error))}: ${outcome.error.message}`
    return compareError(form(outcome.error), error, read)
  }

  const compareText = (what, got, want) => (got === want ? '' : `${what} ${JSON.stringify(got)}, expected ${JSON.stringify(want)}`)

  function limitsOf(given) {
    const limits = given.get('limits')
    if (limits === undefined) return undefined
    const out = {}
    for (const [k, v] of limits.entries) {
      if (LIMITS[k] === undefined) throw new VectorError(`unknown limit ${k}`)
      out[LIMITS[k]] = Number(v.text)
    }
    return out
  }

  const rulesetDocument = (given) => `{"version":1,"ruleset":${write(need(given, 'ruleset'))}}`

  /** The text a `read` vector reads, and its format. */
  function sourceOf(given) {
    if (given.has('text')) return [given.get('text'), 'json']
    if (given.has('document')) return [write(given.get('document')), 'json']
    if (given.has('yaml')) return [given.get('yaml'), 'yaml']
    if (given.has('rules_text')) return [given.get('rules_text'), 'text']
    if (given.has('table_csv')) return [given.get('table_csv'), 'csv']
    if (given.has('table_grid')) return [null, null]
    throw new VectorError('nothing to read')
  }

  // --- The JSON route: the native layer ------------------------------------------

  /** A decision of the native layer read with its readers, against the expected decision (C++'s `check_readers`). */
  function checkNativeReaders(d, expected) {
    const results = need(expected, 'results')
    if (d.length !== results.length) return `length ${d.length}, expected ${results.length}`
    const outputs = []
    for (const [i, result] of results.entries()) {
      const at = `rule ${i}: `
      if (d.ruleName(i) !== need(result, 'rule')) return `${at}ruleName() ${d.ruleName(i)}`
      if (d.firedAt(i) !== need(result, 'fired')) return `${at}firedAt() differs`
      const truth = need(need(result, 'when'), 'truth')
      const want = truth === null ? 'unknown' : String(truth)
      if (d.truth(i) !== want) return `${at}truth() ${d.truth(i)}, expected ${want}`
      const then = result.get('then')
      const output = then === undefined ? null : new Obj(then.entries.map(([k, t]) => [k, need(t, 'value')]))
      outputs.push(output)
      const got = d.ruleOutputJson(i)
      if (output === null ? got !== null : got === null || !same(parse(got), output)) {
        return `${at}ruleOutputJson() ${got}, expected ${output === null ? 'null' : write(output)}`
      }
    }
    const positions = d.selected()
    const names = positions.map((p) => need(results[p], 'rule'))
    if (!same(names, need(expected, 'selected'))) return `selected() ${JSON.stringify(names)}`
    const wantOutputs = positions.map((p) => outputs[p] ?? new Obj([]))
    if (!same(parse(d.outputsJson()), wantOutputs)) return `outputsJson() ${d.outputsJson()}, expected ${write(wantOutputs)}`
    const policy = need(expected, 'hit_policy')
    let want
    if (expected.has('aggregated')) want = expected.get('aggregated')
    else if (['first', 'priority', 'unique', 'any'].includes(policy)) want = wantOutputs[0] ?? null
    else {
      const outcome = attempt(R, () => d.outputJson())
      if (outcome.error?.code !== 'hit_policy_error') return `outputJson() under ${policy}: expected a hit_policy_error`
      return ''
    }
    const got = d.outputJson()
    if (want === null ? got !== null : got === null || !same(parse(got), want)) {
      return `outputJson() ${got}, expected ${want === null ? 'null' : write(want)}`
    }
    return ''
  }

  function readJson(given, expect) {
    if (given.has('table_grid')) return { skip: 'grid' }
    const [source, format] = sourceOf(given)
    const limits = limitsOf(given)
    const read = { read: true }
    if (given.has('ruleset')) {
      const rules = N.RuleSet.read(rulesetDocument(given))
      return compare(
        attempt(R, () => parse(rules.readDecision(source, { format, limits }).write())),
        expect,
        'document',
        read,
      )
    }
    const outcome = attempt(R, () => N.Document.read(source, { format, limits }))
    if (outcome.error !== undefined) return compare(outcome, expect, 'document', read)
    const document = outcome.result
    if (expect.has('error')) {
      // Read, but not writable in the form the vector names.
      const target = given.get('write')
      if (target === undefined) return `read a document, expected the error ${write(expect.get('error'))}`
      return compare(attempt(R, () => document.write(target)), expect, 'document', read)
    }
    const issues = []
    if (expect.has('document')) issues.push(compare(attempt(R, () => parse(document.write())), expect, 'document', read))
    if (expect.has('printed')) issues.push(compareText('printed', document.write('text'), expect.get('printed')))
    if (expect.has('csv')) issues.push(compareText('wrote the CSV', document.write('csv'), expect.get('csv')))
    if (expect.has('compiled')) {
      issues.push(
        compare(attempt(R, () => content(N.RuleSet.fromDocument(document).write(), 'ruleset')), expect, 'compiled', read),
      )
    }
    return issues.filter(Boolean).join('; ')
  }

  function runJson(vector) {
    const given = need(vector, 'given')
    const expect = need(vector, 'expect')
    const kind = need(given, 'kind')
    switch (kind) {
      case 'read':
        return readJson(given, expect)
      case 'condition':
      case 'value': {
        const node = N.Node.read(write(need(given, kind)))
        const strict = given.get('strict') === true
        const input = write(need(given, 'input'))
        return compare(
          attempt(R, () =>
            parse(
              kind === 'value'
                ? node.evaluateValueTreeJson(input, { strict })
                : node.evaluateTreeJson(input, { logic: given.get('logic'), strict }),
            ),
          ),
          expect,
          'result',
        )
      }
      case 'evaluate':
      case 'explain': {
        const rules = N.RuleSet.read(rulesetDocument(given))
        if (given.has('inputs')) {
          let batch
          const issue = compare(
            attempt(R, () => {
              batch = rules.evaluateManyJson(write(given.get('inputs')))
              return batch.map((d) => content(d.write(), 'decision'))
            }),
            expect,
            'decisions',
          )
          if (issue !== '' || batch === undefined) return issue
          for (const [i, wanted] of need(expect, 'decisions').entries()) {
            const found = checkNativeReaders(batch[i], wanted)
            if (found !== '') return `decision ${i}: ${found}`
          }
          return ''
        }
        let decision
        const issue = compare(
          attempt(R, () => {
            decision = rules.evaluateJson(write(need(given, 'input')))
            return content(decision.write(), 'decision')
          }),
          expect,
          'decision',
        )
        if (issue !== '' || decision === undefined) return issue
        const readers = checkNativeReaders(decision, need(expect, 'decision'))
        if (readers !== '' || kind === 'evaluate') return readers
        const explained = compare(attempt(R, () => content(decision.explain('json'), 'explanation')), expect, 'explanation')
        if (explained !== '' || !expect.has('text')) return explained
        return compareText('explained', decision.explain(), expect.get('text'))
      }
      case 'summary': {
        const rules = N.RuleSet.read(rulesetDocument(given))
        const issue = compare(attempt(R, () => content(rules.summary('json'), 'summary')), expect, 'summary')
        if (issue !== '' || !expect.has('text')) return issue
        return compareText('summarised', rules.summary('text'), expect.get('text'))
      }
      default:
        throw new VectorError(`unknown kind ${kind}`)
    }
  }

  // --- Native values ------------------------------------------------------------

  /** An aware datetime made by the engine from its text: a `ZonedDateTime`, which reads back as the same value. */
  function engineDatetime(text) {
    const condition = `{"version":1,"condition":{"op":"eq","left":{"op":"field","path":["x"]},"right":{"op":"const","value":{"$datetime":${JSON.stringify(text)}}}}}`
    const value = R.loads(condition).children()[1].value
    if (!(value instanceof R.ZonedDateTime)) throw new VectorError(`the engine gave back ${value} for ${text}`)
    return value
  }

  /**
   * A `$datetime` as native data: `datetime(...)` when naive; when aware, a
   * `Date` in `Z` to the millisecond, else `Temporal.Instant`,
   * `Temporal.ZonedDateTime` in its zone or at its offset, and where neither
   * represents it exactly (no Temporal, an offset with seconds, a zone the
   * runtime does not know or places elsewhere), the `ZonedDateTime` the
   * engine gives back for it, counted as a fallback.
   */
  function datetimeOf(text) {
    const p = datetimeParts(text)
    if (p.offset === null) return R.datetime(p.year, p.month, p.day, p.hour, p.minute, p.second, p.microsecond)
    const utc = p.zone === null && p.offset === 0
    if (utc && p.microsecond % 1000 === 0) {
      const date = new Date(0)
      date.setUTCFullYear(p.year, p.month - 1, p.day)
      date.setUTCHours(p.hour, p.minute, p.second, p.microsecond / 1000)
      return date
    }
    if (Temporal === undefined) {
      fallback(
        p.zone === null
          ? 'an aware datetime with microseconds or an offset, without Temporal (a Date holds milliseconds, in Z)'
          : 'a datetime in a named zone, without Temporal',
      )
      return engineDatetime(text)
    }
    if (utc) return Temporal.Instant.from(text)
    if (p.zone === null && p.offset % 60 !== 0) {
      fallback('an offset with seconds, which Temporal zones cannot hold')
      return engineDatetime(text)
    }
    try {
      return Temporal.ZonedDateTime.from(p.zone === null ? `${text}[${p.mark}]` : text)
    } catch {
      fallback("a zone the runtime's Temporal does not know or places elsewhere")
      return engineDatetime(text)
    }
  }

  /** A map: a plain object, or a `Map` where a plain object would reorder its keys or lose one. */
  function mapOf(o) {
    const entries = o.entries.map(([k, v]) => [k, nativeOf(v)])
    if (entries.some(([k]) => /^(?:0|[1-9]\d*)$/.test(k) || k === '__proto__' || k === 'constructor')) return new Map(entries)
    return Object.fromEntries(entries)
  }

  /** A value of the JSON format (`spec/json-format.md`, *Values*) as native JavaScript data (`spec/data-model.md`). */
  function nativeOf(v) {
    return noteForm(convert(v))
  }

  function convert(v) {
    if (v instanceof Num) {
      if (v.isInteger) return intOf(v.text)
      const x = Number(v.text)
      // A float holding an integer would read as an int.
      return Number.isInteger(x) ? R.float(x) : x
    }
    if (Array.isArray(v)) return v.map(nativeOf)
    if (!(v instanceof Obj)) return v
    if (!v.entries.some(([k]) => k.startsWith('$'))) return mapOf(v)
    if (v.entries.length !== 1) throw new VectorError(`a typed value with other members: ${write(v)}`)
    const [[tag, inner]] = v.entries
    const text = inner instanceof Num ? inner.text : inner
    switch (tag) {
      case '$int':
        return intOf(text)
      case '$float':
        return R.float(text === 'inf' ? Infinity : text === '-inf' ? -Infinity : Number(text))
      case '$decimal':
        return R.decimal(text)
      case '$date': {
        const [year, month, day] = text.split('-').map(Number)
        return R.date(year, month, day)
      }
      case '$datetime':
        return datetimeOf(text)
      case '$map':
        return mapOf(inner)
      default:
        throw new VectorError(`no native form for ${write(v)}`)
    }
  }

  // --- Values given back ----------------------------------------------------------

  /**
   * A value given back to JavaScript (`spec/data-model.md`, *Values given
   * back*) as a value of the JSON format, by the table read backwards; every
   * class is checked, and an aware datetime's fields against its instant.
   */
  function given(value) {
    if (value === null || typeof value === 'boolean' || typeof value === 'string') return value
    if (typeof value === 'number') {
      if (!Number.isFinite(value)) {
        if (Number.isNaN(value)) throw new VectorError('NaN given back')
        return new Obj([['$float', value > 0 ? 'inf' : '-inf']])
      }
      if (Number.isInteger(value) && !Object.is(value, -0)) return new Num(BigInt(value).toString(), true)
      return new Num(floatText(value))
    }
    if (typeof value === 'bigint') {
      if (value >= -MAX_SAFE && value <= MAX_SAFE) throw new VectorError(`the safe int ${value} given back as a bigint`)
      // The canonical form of an int beyond 2⁵³ (`spec/json-format.md`, *Values*).
      return new Obj([['$int', value.toString()]])
    }
    if (value instanceof R.Decimal) return new Obj([['$decimal', value.text]])
    if (value instanceof R.LocalDate) return new Obj([['$date', `${pad(value.year, 4)}-${pad(value.month, 2)}-${pad(value.day, 2)}`]])
    if (value instanceof R.NaiveDateTime) return new Obj([['$datetime', datetimeText(value, null, null)]])
    if (value instanceof R.ZonedDateTime) {
      const text = datetimeText(value, value.offsetSeconds, value.timeZone)
      const instant = epochMicros({ ...value, offset: value.offsetSeconds })
      if (instant !== value.epochMicroseconds) {
        throw new VectorError(`the ZonedDateTime ${text} has the instant ${value.epochMicroseconds}, not ${instant}`)
      }
      if (String(value) !== text) throw new VectorError(`the ZonedDateTime ${text} writes itself ${value}`)
      return new Obj([['$datetime', text]])
    }
    if (Array.isArray(value)) return value.map(given)
    const proto = Object.getPrototypeOf(value)
    if (proto === Object.prototype || proto === null) {
      const object = new Obj(Object.keys(value).map((k) => [k, given(value[k])]))
      return object.entries.some(([k]) => k.startsWith('$')) ? new Obj([['$map', object]]) : object
    }
    return new Obj([['$opaque', value?.constructor?.name ?? typeof value]])
  }

  const truthOf = (truth) => ({ true: true, false: false, unknown: null })[truth]
  const pathOf = (path) => path.map((segment) => (typeof segment === 'number' ? new Num(String(segment)) : segment))

  function reasonOf(reason) {
    const entries = [['code', reason.code]]
    if (reason.paths.length > 0) entries.push(['paths', reason.paths.map(pathOf)])
    if (reason.types.length > 0) entries.push(['types', [...reason.types]])
    return new Obj(entries)
  }

  /** A `ResultTree` in the JSON format (`spec/json-format.md`, *Result trees*). */
  function resultTree(t) {
    const entries = [['op', t.node.tag]]
    if (t.node.tag === 'field') entries.push(['path', pathOf(t.node.path)])
    if (t.truth !== null) entries.push(['truth', truthOf(t.truth)])
    else entries.push(['value', given(t.value)])
    if (t.reason !== null) entries.push(['reason', reasonOf(t.reason)])
    if (t.children.length > 0) entries.push(['children', t.children.map(resultTree)])
    return new Obj(entries)
  }

  /** The condition of a node, as the core writes it (`dumps`). */
  const conditionOf = (node) => content(R.dumps(node), 'condition')

  function gapOf(gap) {
    if (gap !== null && typeof gap === 'object' && !(gap instanceof R.Decimal)) {
      const [[key, n]] = Object.entries(gap)
      return new Obj([[key, given(n)]])
    }
    return given(gap)
  }

  function requirementOf(item) {
    if (item instanceof R.CauseGroup) return new Obj([[item.kind, item.items.map(requirementOf)]])
    const holds = item.truth === 'true' ? !item.negated : item.truth === 'false' ? item.negated : false
    if (item.holds !== holds) throw new VectorError(`a cause with ${item.truth}, negated ${item.negated} says holds ${item.holds}`)
    const entries = [
      ['condition', conditionOf(item.node)],
      ['truth', truthOf(item.truth)],
      ['negated', item.negated],
      ['values', item.values.map(([path, value]) => new Obj([['path', pathOf(path)], ['value', given(value)]]))],
    ]
    if (item.reason !== null) entries.push(['reason', reasonOf(item.reason)])
    const c = item.counterfactual
    if (c !== null) {
      const parts = [['path', pathOf(c.path)], ['op', c.op], ['target', given(c.target)]]
      if (c.gap !== null) parts.push(['gap', gapOf(c.gap)])
      entries.push(['counterfactual', new Obj(parts)])
    }
    return new Obj(entries)
  }

  /** An `Explanation` in the JSON format, without the causes' `at` (the idiomatic API has no decision document to point into). */
  function explanationOf(decision, explanation) {
    const rules = explanation.rules.map((rule) => {
      const entries = [['rule', rule.rule], ['status', rule.status]]
      if (rule.note !== null) {
        const note = [['kind', rule.note.kind], ['winner', rule.note.winner]]
        if (rule.note.priority !== null) note.push(['priority', given(rule.note.priority)], ['winner_priority', given(rule.note.winnerPriority)])
        entries.push(['note', new Obj(note)])
      }
      if (rule.achievable !== null) entries.push(['achievable', rule.achievable])
      entries.push(['requirement', requirementOf(rule.requirement)])
      if (rule.causes.length === 0) throw new VectorError(`the rule ${rule.rule} has no causes`)
      return new Obj(entries)
    })
    return new Obj([
      ['hit_policy', decision.hitPolicy],
      ['logic', decision.ruleset.logic],
      ['selected', [...explanation.selected]],
      ['rules', rules],
    ])
  }

  /** The expected explanation without the causes' `at`, its maps in a plain object's order. */
  function withoutAt(o) {
    if (Array.isArray(o)) return o.map(withoutAt)
    if (!(o instanceof Obj)) return o
    if (o.has('condition') && o.has('at')) return o.without('at')
    return new Obj(o.entries.map(([k, v]) => [k, k === 'values' || k === 'counterfactual' ? v : withoutAt(v)]))
  }

  /** A `Summary` in the JSON format (`spec/json-format.md`, *Summaries*). */
  function summaryOf(s) {
    const entries = [['hit_policy', s.hitPolicy]]
    if (s.aggregate !== null) entries.push(['aggregate', s.aggregate])
    entries.push(['logic', s.logic])
    if (s.strict) entries.push(['strict', true])
    entries.push([
      'rules',
      s.rules.map((r) => {
        const rule = [['name', r.name], ['priority', given(r.priority)], ['outputs', [...r.outputs]]]
        if (r.action) rule.push(['action', true])
        if (r.actionName !== null) rule.push(['do', r.actionName])
        return new Obj(rule)
      }),
    ])
    entries.push([
      'fields',
      s.fields.map(
        (f) =>
          new Obj([
            ['path', pathOf(f.path)],
            ['conditions', [...f.conditions]],
            ['payloads', f.payloads.map(([rule, key]) => new Obj([['rule', rule], ['key', key]]))],
          ]),
      ),
    ])
    entries.push(['outputs', [...s.outputs]])
    // A plain object of tags: in the spec's order, sorted by code point.
    const tags = Object.keys(s.tags).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))
    entries.push(['tags', tags.map((tag) => new Obj([['tag', tag], ['rules', [...s.tags[tag]]]]))])
    return new Obj(entries)
  }

  /** Whether a value given back matches the expected value of the format. */
  const sameGiven = (value, expected, recorded = false) => same(given(value), jsOrder(expected), { given: true, recorded })

  /** A `Decision` of the idiomatic API against the expected decision: its results, selection and outputs. */
  function checkDecision(ruleset, d, expected, recorded = false) {
    const results = need(expected, 'results')
    if (d.ruleset !== ruleset) return 'decision.ruleset is another rule set'
    if (d.hitPolicy !== need(expected, 'hit_policy')) return `hitPolicy ${d.hitPolicy}`
    if (d.results.length !== results.length) return `${d.results.length} results, expected ${results.length}`
    const outputs = []
    for (const [i, result] of results.entries()) {
      const got = d.results[i]
      const at = `results[${i}]: `
      if (got.rule !== ruleset.rules[i] || got.rule.name !== need(result, 'rule')) return `${at}rule ${got.rule.name}`
      if (got.fired !== need(result, 'fired')) return `${at}fired ${got.fired}`
      const truth = need(need(result, 'when'), 'truth')
      if (truthOf(got.truth) !== truth) return `${at}truth ${got.truth}, expected ${truth}`
      const then = result.get('then')
      const output = then === undefined ? null : new Obj(then.entries.map(([k, t]) => [k, need(t, 'value')]))
      outputs.push(output)
      if (output === null ? got.output !== null : got.output === null || !sameGiven(got.output, output, recorded)) {
        return `${at}output ${got.output === null ? 'null' : write(given(got.output))}, expected ${output === null ? 'null' : write(output)}`
      }
    }
    const selected = need(expected, 'selected')
    const names = d.selected.map((r) => r.rule.name)
    if (!same(names, selected)) return `selected ${JSON.stringify(names)}`
    if (d.selected.some((r) => !d.results.includes(r))) return 'selected holds results that are not in results'
    const fired = results.filter((r) => need(r, 'fired')).map((r) => need(r, 'rule'))
    if (!same([...d.fired], fired)) return `fired ${JSON.stringify(d.fired)}`
    const positions = d.selected.map((r) => d.results.indexOf(r))
    const wantOutputs = positions.map((p) => outputs[p] ?? new Obj([]))
    if (d.outputs.length !== wantOutputs.length || !d.outputs.every((o, k) => sameGiven(o, wantOutputs[k], recorded))) {
      return `outputs ${write(d.outputs.map(given))}, expected ${write(wantOutputs)}`
    }
    const policy = need(expected, 'hit_policy')
    let want
    if (expected.has('aggregated')) want = expected.get('aggregated')
    else if (['first', 'priority', 'unique', 'any'].includes(policy)) want = wantOutputs[0] ?? null
    else {
      const outcome = attempt(R, () => d.output)
      if (!(outcome.error instanceof R.HitPolicyError)) return `output under ${policy}: expected a HitPolicyError`
      return d.aggregated === null ? '' : 'aggregated is not null without an aggregate'
    }
    const output = d.output
    if (want === null ? output !== null : output === null || !sameGiven(output, want, recorded)) {
      return `output ${output === null ? 'null' : write(given(output))}, expected ${want === null ? 'null' : write(want)}`
    }
    const aggregated = expected.has('aggregated') ? output : null
    if (d.aggregated !== aggregated) return 'aggregated differs from output'
    return ''
  }

  // --- Building in code -----------------------------------------------------------

  const ARITHMETIC = ['eq', 'ne', 'lt', 'le', 'gt', 'ge', 'add', 'sub', 'mul', 'div']
  const STRING_TESTS = { starts_with: 'startsWith', ends_with: 'endsWith', contains: 'contains' }

  /** A condition or value node of the format, built with the idiomatic API. */
  function build(node) {
    const op = need(node, 'op')
    if (op === 'field') return R.field(...need(node, 'path').map((s) => (s instanceof Num ? Number(s.text) : s)))
    if (op === 'const') return R.constant(nativeOf(need(node, 'value')))
    if (ARITHMETIC.includes(op)) return build(need(node, 'left'))[op](build(need(node, 'right')))
    if (op === 'and' || op === 'or') return R[op](...need(node, 'operands').map(build))
    const operand = build(need(node, 'operand'))
    switch (op) {
      case 'not':
        return R.not(operand)
      case 'in':
        return operand.isIn(need(node, 'values').map(nativeOf))
      case 'between': {
        const closed = node.get('closed')
        return operand.between(build(need(node, 'low')), build(need(node, 'high')), closed === undefined ? undefined : { closed })
      }
      case 'is_null':
        return operand.isNull()
      case 'matches':
        return operand.matches(need(node, 'pattern'))
      case 'abs':
        return operand.abs()
      case 'neg':
        return operand.neg()
      default:
        if (STRING_TESTS[op] !== undefined) return operand[STRING_TESTS[op]](need(node, 'text'))
        throw new VectorError(`unknown op ${op}`)
    }
  }

  /** A rule set of the format, built with `Rule` and `RuleSet`. */
  function buildRuleSet(content) {
    const rules = need(content, 'rules').map((rule) => {
      const then = rule.get('then')
      const options = {}
      if (rule.has('priority')) options.priority = nativeOf(rule.get('priority'))
      if (rule.has('tags')) options.tags = rule.get('tags')
      if (rule.has('description')) options.description = rule.get('description')
      if (rule.has('metadata')) options.metadata = nativeOf(rule.get('metadata'))
      if (rule.has('do')) options.do = rule.get('do')
      return new R.Rule(
        need(rule, 'name'),
        build(need(rule, 'when')),
        then === undefined ? undefined : new Map(then.entries.map(([k, n]) => [k, build(n)])),
        options,
      )
    })
    const options = {}
    for (const [key, name] of [
      ['hit_policy', 'hitPolicy'],
      ['aggregate', 'aggregate'],
      ['logic', 'logic'],
      ['strict', 'strict'],
    ]) {
      if (content.has(key)) options[name] = content.get(key)
    }
    return new R.RuleSet(rules, options)
  }

  /** The rule set built in code must be the one the core reads from the vector: same document. */
  function checkBuilt(ruleset, given) {
    const built = content(R.dumps(ruleset), 'ruleset')
    const read = content(N.RuleSet.read(rulesetDocument(given)).write(), 'ruleset')
    if (!same(built, read)) return `the rule set built in code writes\n  ${write(built)}\nthe core reads\n  ${write(read)}`
    return checkReload(ruleset)
  }

  function checkBuiltNode(node, json) {
    if (node.kind === 'value') return ''
    const built = conditionOf(node)
    const read = content(N.Document.read(`{"version":1,"condition":${write(json)}}`).write(), 'condition')
    if (!same(built, read)) return `the condition built in code writes\n  ${write(built)}\nthe core reads\n  ${write(read)}`
    return checkReload(node)
  }

  /**
   * `toJSON()` for `JSON.stringify`: what it writes of `object` is the
   * expected tree, each float that holds an integer typed (`stringified`).
   * `key` picks the content of a document.
   */
  function checkStringify(what, object, expected, key) {
    let got = parse(JSON.stringify(object))
    if (key !== undefined) got = need(got, key)
    // JSON.parse makes plain objects, which list integer-like keys first.
    const want = jsOrder(stringified(expected))
    return same(got, want) ? '' : `JSON.stringify of the ${what}\n  got  ${write(got)}\n  want ${write(want)}`
  }

  /**
   * `loads(JSON.stringify(value))` is `value`, but for what plain objects
   * cannot hold: the sign of a float zero (`JSON.stringify(-0)` is `0`) and
   * the order of integer-like keys, which come first.
   */
  function checkReload(value) {
    const text = JSON.stringify(value)
    const again = R.loads(value instanceof R.Expr ? `{"version":1,"condition":${text}}` : text)
    const got = parse(R.dumps(again))
    const want = jsOrder(unsignedZeros(parse(R.dumps(value))))
    return same(got, want) ? '' : `loads(JSON.stringify(x)) writes\n  ${write(got)}\nnot\n  ${write(want)}`
  }

  /**
   * A decision through `JSON.stringify`: the expected decision, and read back
   * with `readDecision`, the same decision.
   */
  function checkDecisionText(ruleset, decision, expected) {
    const written = checkStringify('Decision', decision, expected, 'decision')
    if (written !== '') return written
    const read = attempt(R, () => ruleset.readDecision(JSON.stringify(decision)))
    // A payload whose integer-like keys a plain object moves first is no longer
    // the rule's: `then` keeps the payload's order (`spec/json-format.md`,
    // *Reading a decision*), so the reader refuses it.
    const moved = need(expected, 'results').some((r) => {
      const then = r.get('then')
      return then !== undefined && !same(jsOrder(then), then)
    })
    if (moved) {
      return read.error?.code === 'decision_mismatch' ? '' : 'readDecision(JSON.stringify(decision)) with a payload reordered: expected a decision_mismatch'
    }
    if (read.error !== undefined) return `readDecision(JSON.stringify(decision)): ${read.error.code}: ${read.error.message}`
    const issue = checkDecision(ruleset, read.result, expected, true)
    return issue === '' ? '' : `readDecision(JSON.stringify(decision)): ${issue}`
  }

  // --- The native route: the idiomatic API --------------------------------------------

  const idiomatic = { form: contextForm }
  const givenMode = { form: contextForm, mode: { given: true }, view: jsOrder }

  function readNative(given, expect) {
    const [source, format] = sourceOf(given)
    const limits = limitsOf(given)
    const read = { read: true, form: contextForm }
    if (given.has('ruleset')) {
      const ruleset = buildRuleSet(need(given, 'ruleset'))
      const built = checkBuilt(ruleset, given)
      if (built !== '') return built
      const outcome = attempt(R, () => ruleset.readDecision(source, { format, limits }))
      if (outcome.error !== undefined || expect.has('error')) return compare(outcome, expect, 'document', read)
      const expected = need(need(expect, 'document'), 'decision')
      return checkDecision(ruleset, outcome.result, expected, true) || checkDecisionText(ruleset, outcome.result, expected)
    }
    let outcome
    if (given.has('table_grid')) {
      const grid = need(given, 'table_grid')
      const options = { limits }
      for (const [key, name] of [
        ['hit_policy', 'hitPolicy'],
        ['aggregate', 'aggregate'],
        ['logic', 'logic'],
        ['strict', 'strict'],
      ]) {
        if (grid.has(key)) options[name] = grid.get(key)
      }
      const cell = (c) => (typeof c === 'string' ? c : nativeOf(c))
      outcome = attempt(R, () => new R.DecisionTable(need(grid, 'header').map(cell), need(grid, 'rows').map((row) => row.map(cell)), options))
    } else {
      outcome = attempt(R, () => R.loads(source, { format, limits }))
    }
    const issues = []
    if (given.has('rules_text')) issues.push(fromText(source, limits, expect))
    if (outcome.error !== undefined) {
      issues.push(compare(outcome, expect, 'document', read))
      return issues.filter(Boolean).join('; ')
    }
    const loaded = outcome.result
    if (expect.has('error')) {
      const target = given.get('write')
      if (target === undefined) return `read a ${loaded.constructor.name}, expected the error ${write(expect.get('error'))}`
      issues.push(compare(attempt(R, () => R.dumps(loaded, { format: target })), expect, 'document', read))
      return issues.filter(Boolean).join('; ')
    }
    if (expect.has('document')) {
      issues.push(compare(attempt(R, () => parse(R.dumps(loaded))), expect, 'document', read))
      issues.push(checkReload(loaded))
    }
    if (expect.has('printed')) issues.push(compareText('printed', R.dumps(loaded, { format: 'text' }), expect.get('printed')))
    if (expect.has('csv')) issues.push(compareText('wrote the CSV', R.dumps(loaded, { format: 'csv' }), expect.get('csv')))
    if (expect.has('compiled')) {
      if (!(loaded instanceof R.DecisionTable)) return `read a ${loaded.constructor.name}, expected a DecisionTable`
      issues.push(compare(attempt(R, () => content(R.dumps(loaded.ruleset), 'ruleset')), expect, 'compiled', read))
    }
    return issues.filter(Boolean).join('; ')
  }

  /**
   * `ruleFromText` on a rule text: the rule `loads` reads, a
   * `rule_syntax_error` for a condition or a rule set (`spec/text.md`), and
   * an error where `loads` fails.
   */
  function fromText(text, limits, expect) {
    const outcome = attempt(R, () => R.ruleFromText(text, { limits }))
    const document = expect.get('document')
    if (expect.has('error') || document === undefined) {
      if (expect.has('error') && outcome.error === undefined) return `ruleFromText read a rule, expected the error ${write(expect.get('error'))}`
      return ''
    }
    if (document.has('rule')) {
      if (outcome.error !== undefined) return `ruleFromText: unexpected error ${write(contextForm(outcome.error))}`
      const written = parse(R.dumps(outcome.result))
      return same(written, document) ? '' : `ruleFromText wrote ${write(written)}`
    }
    if (outcome.error?.code !== 'rule_syntax_error') {
      return `ruleFromText of a ${document.has('condition') ? 'condition' : 'rule set'}: expected a rule_syntax_error`
    }
    return ''
  }

  function inputOf(given) {
    return given.has('inputs') ? need(given, 'inputs').map(nativeOf) : nativeOf(need(given, 'input'))
  }

  function runNative(vector) {
    const given = need(vector, 'given')
    const expect = need(vector, 'expect')
    const kind = need(given, 'kind')
    if (kind === 'read') return readNative(given, expect)
    if (kind === 'summary') {
      const ruleset = buildRuleSet(need(given, 'ruleset'))
      const built = checkBuilt(ruleset, given)
      if (built !== '') return built
      const issue =
        compare(attempt(R, () => summaryOf(ruleset.summary())), expect, 'summary', idiomatic) ||
        checkStringify('Summary', ruleset.summary(), need(expect, 'summary'), 'summary')
      if (issue !== '' || !expect.has('text')) return issue
      return compareText('summarised', String(ruleset.summary()), expect.get('text'))
    }
    if (kind === 'condition' || kind === 'value') {
      const json = need(given, kind)
      const strict = given.get('strict') === true
      const logic = given.get('logic')
      const node = build(json)
      const built = checkBuiltNode(node, json)
      if (built !== '') return built
      const input = inputOf(given)
      let tree
      const evaluated = compare(
        attempt(R, () => {
          tree = kind === 'value' ? R.evaluateValueTree(node, input, { strict }) : R.evaluateTree(node, input, { logic, strict })
          return resultTree(tree)
        }),
        expect,
        'result',
        givenMode,
      )
      if (evaluated !== '') return evaluated
      if (tree !== undefined) {
        const text = checkStringify('ResultTree', tree, need(expect, 'result'))
        if (text !== '') return text
      }
      // The same native data through the native layer, compared exactly.
      const layer = N.Node.read(write(json))
      const exact = compare(
        attempt(R, () =>
          parse(kind === 'value' ? layer.evaluateValueTree(input, { strict }) : layer.evaluateTree(input, { logic, strict })),
        ),
        expect,
        'result',
      )
      return exact === '' ? '' : `cleanrules/native: ${exact}`
    }
    if (kind !== 'evaluate' && kind !== 'explain') throw new VectorError(`unknown kind ${kind}`)
    const ruleset = buildRuleSet(need(given, 'ruleset'))
    const built = checkBuilt(ruleset, given)
    if (built !== '') return built
    const input = inputOf(given)
    const layer = N.RuleSet.read(rulesetDocument(given))
    const failing = expect.has('error')
    if (given.has('inputs')) {
      const outcome = attempt(R, () => ruleset.evaluateMany(input))
      if (outcome.error !== undefined || failing) {
        const issue = compare(outcome, expect, 'decisions', idiomatic)
        if (issue !== '') return issue
      } else {
        const decisions = need(expect, 'decisions')
        if (outcome.result.length !== decisions.length) return `${outcome.result.length} decisions, expected ${decisions.length}`
        for (const [i, d] of outcome.result.entries()) {
          const issue = checkDecision(ruleset, d, decisions[i]) || checkDecisionText(ruleset, d, decisions[i])
          if (issue !== '') return `decision ${i}: ${issue}`
        }
      }
      // The same native data through the native layer, compared exactly.
      const exact = compare(
        attempt(R, () => layer.evaluateMany(input).map((d) => content(d.write(), 'decision'))),
        expect,
        'decisions',
      )
      return exact === '' ? '' : `cleanrules/native: ${exact}`
    }
    const outcome = attempt(R, () => ruleset.evaluate(input))
    if (outcome.error !== undefined || failing) {
      const issue = compare(outcome, expect, 'decision', idiomatic)
      if (issue !== '') return issue
    } else {
      const decision = outcome.result
      const issue = checkDecision(ruleset, decision, need(expect, 'decision')) || checkDecisionText(ruleset, decision, need(expect, 'decision'))
      if (issue !== '') return issue
      if (kind === 'explain') {
        const explanation = decision.explain()
        const text = checkStringify('Explanation', explanation, need(expect, 'explanation'), 'explanation')
        if (text !== '') return text
        const explained = compare(attempt(R, () => explanationOf(decision, explanation)), expect, 'explanation', {
          ...givenMode,
          view: (e) => jsOrder(withoutAt(e)),
        })
        if (explained !== '') return explained
        if (expect.has('text')) {
          const text = compareText('explained', String(explanation), expect.get('text'))
          if (text !== '') return text
        }
      }
    }
    // The same native data through the native layer, compared exactly.
    let exact
    const written = compare(
      attempt(R, () => {
        exact = layer.evaluate(input)
        return content(exact.write(), 'decision')
      }),
      expect,
      'decision',
    )
    if (written !== '' || kind === 'evaluate' || failing) return written === '' ? '' : `cleanrules/native: ${written}`
    const explained = compare(attempt(R, () => content(exact.explain('json'), 'explanation')), expect, 'explanation')
    return explained === '' ? '' : `cleanrules/native: ${explained}`
  }

  /**
   * Run `vector` one way (`'json'` or `'native'`): `''` when it passes, a
   * message when it fails, or `{ skip }` with a key of `SKIPS`.
   */
  function run(vector, route) {
    current = `${route}:${vector.get('id')}`
    try {
      return route === 'json' ? runJson(vector) : runNative(vector)
    } catch (error) {
      if (error instanceof R.CleanRulesError) return `uncaught ${error.constructor.name} (${error.code}): ${error.message}`
      return `${error instanceof VectorError ? '' : `${error?.constructor?.name}: `}${error?.message ?? error}`
    }
  }

  return { run, fallbacks, forms }
}

/**
 * The comparison itself, on cases the vectors may not reach (as the C++
 * runner checks its own): the cases it gets wrong.
 */
export function checkComparison() {
  const cases = [
    ['18446744073709551617', '18446744073709551617', true], // beyond 64 bits
    ['18446744073709551617', '18446744073709551616', false],
    ['9007199254740993', '9007199254740992', false], // beyond 2^53, one apart
    ['-0', '0', true], // the int 0
    ['1', '1.0', false],
    ['100.0', '1e2', true],
    ['0.1', '0.10000000000000001', true], // the same double
    ['-0.0', '0.0', false],
    ['[1,2]', '[2,1]', false],
    ['{"a":1,"b":2}', '{"b":2,"a":1}', false],
    ['{"a":1}', '{"a":1,"b":2}', false],
    ['{"$opaque":"socket"}', '{"$opaque":"bytes"}', true],
    ['{"$opaque":"socket"}', 'null', false],
    ['"é😀"', '"\\u00e9\\ud83d\\ude00"', true],
    ['true', '1', false],
    ['null', 'false', false],
    ['{"code":"null","detail":"a"}', '{"code":"null"}', false], // detail only inside a reason
    ['{"reason":{"code":"null","detail":"a"}}', '{"reason":{"code":"null"}}', true],
  ]
  return cases
    .filter(([a, e, want]) => same(parse(a), parse(e)) !== want)
    .map(([a, e, want]) => `comparison: ${a} ${want ? '==' : '!='} ${e} does not hold`)
}
