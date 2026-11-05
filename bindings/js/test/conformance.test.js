// A handful of conformance vectors as smoke tests, through JSON and through
// native JavaScript data (conformance/README.md). The full runner, over every
// vector, is task F.3.

import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import test from 'node:test'

import cr from './support/cr.js'
import { Num, Obj, parse, write, same } from './support/json.js'

const VECTORS = path.join(import.meta.dirname, '..', '..', '..', 'conformance', 'vectors')
const FILES = [
  'ast/reading.json',
  'ast/missing.json',
  'ast/nodes.json',
  'ast/types.json',
  'rules/policies.json',
  'rules/aggregates.json',
  'rules/payloads.json',
  'rules/strict.json',
  'rules/batch.json',
  'explain/notes.json',
  'explain/summaries.json',
  'format/documents.json',
  'format/limits.json',
  'format/decisions.json',
  'format/yaml.json',
]

/**
 * The fields of an error that vectors compare, as the C and C++ runners do:
 * for reading, those the vector lists; for evaluating, these.
 */
const EVALUATION = ['code', 'rule', 'path', 'types', 'rules', 'row']

function errorTree(error, expected, read) {
  assert.ok(error instanceof cr.CleanRulesError, `not a cleanrules error: ${error}`)
  const form = error.toJSON()
  const fields = read ? expected.entries.map(([k]) => k) : EVALUATION
  const kept = {}
  for (const field of fields) {
    if (field in form) kept[field] = form[field]
  }
  return parse(JSON.stringify(kept))
}

function sameError(actual, expected) {
  const a = actual.entries.slice().sort(([x], [y]) => (x < y ? -1 : 1))
  const e = expected.entries.slice().sort(([x], [y]) => (x < y ? -1 : 1))
  return a.length === e.length && e.every(([k, v], n) => a[n][0] === k && same(a[n][1], v))
}

/** Run `produce` and compare with `expect[key]`, or with `expect.error`. */
function check(id, produce, expect, key, read = false) {
  let actual
  try {
    actual = produce()
  } catch (error) {
    if (!expect.has('error')) throw error
    const got = errorTree(error, expect.get('error'), read)
    assert.ok(sameError(got, expect.get('error')), `${id}: error ${write(got)}, expected ${write(expect.get('error'))}`)
    return
  }
  assert.ok(!expect.has('error'), `${id}: no error, expected ${write(expect.get('error') ?? null)}`)
  assert.ok(same(actual, expect.get(key)), `${id}: ${write(actual)}\nexpected ${write(expect.get(key))}`)
}

const document = (content) => `{"version":1,"ruleset":${write(content)}}`
const contentOf = (text, key) => parse(text).get(key)

function limitsOf(given) {
  const limits = given.get('limits')
  if (limits === undefined) return undefined
  const names = { max_size: 'maxSize', max_nesting: 'maxNesting', max_depth: 'maxDepth', max_table_size: 'maxTableSize' }
  const out = {}
  for (const [k, v] of limits.entries) out[names[k]] = Number(v.text)
  return out
}

/** A value of the format as native JavaScript data, or `undefined` to skip. */
function native(tree) {
  if (tree instanceof Num) {
    const n = Number(tree.text)
    if (tree.isInteger) return Number.isSafeInteger(n) ? n : BigInt(tree.text)
    return Number.isInteger(n) ? cr.float(n) : n
  }
  if (Array.isArray(tree)) return tree.map(native)
  if (!(tree instanceof Obj)) return tree
  if (tree.entries.length === 1) {
    const [[key, inner]] = tree.entries
    const text = inner instanceof Num ? inner.text : inner
    switch (key) {
      case '$int':
        return Number.isSafeInteger(Number(text)) ? Number(text) : BigInt(text)
      case '$float':
        return cr.float(text === 'inf' ? Infinity : text === '-inf' ? -Infinity : Number(text))
      case '$decimal':
        return cr.decimal(text)
      case '$date': {
        const [y, m, d] = text.split('-').map(Number)
        return cr.date(y, m, d)
      }
      case '$datetime': {
        const m = /^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(?:\.(\d{1,6}))?(.*)$/.exec(text)
        const parts = m.slice(1, 7).map(Number)
        const micros = Number((m[7] ?? '').padEnd(6, '0'))
        if (m[8] === '') return cr.datetime(...parts, micros)
        throw new Skip()
      }
      case '$map':
        return objectOf(inner)
      default:
        break
    }
  }
  return objectOf(tree)
}

class Skip extends Error {}

/** A map: a plain object, or a `Map` when its keys are integer-like (objects reorder those). */
function objectOf(tree) {
  const entries = tree.entries.map(([k, v]) => [k, native(v)])
  if (entries.some(([k]) => /^(0|[1-9]\d*)$/.test(k))) return new Map(entries)
  return Object.fromEntries(entries)
}

function runJson(id, given, expect) {
  const kind = given.get('kind')
  switch (kind) {
    case 'read': {
      if (['rules_text', 'table_csv', 'table_grid', 'write'].some((k) => given.has(k))) return false
      if (['csv', 'compiled', 'printed'].some((k) => expect.has(k))) return false
      const yaml = given.has('yaml')
      const text = given.has('text') ? given.get('text') : yaml ? given.get('yaml') : write(given.get('document'))
      const format = yaml ? 'yaml' : 'json'
      const limits = limitsOf(given)
      if (given.has('ruleset')) {
        const ruleset = cr.RuleSet.read(document(given.get('ruleset')))
        check(id, () => parse(ruleset.readDecision(text, { format, limits }).write()), expect, 'document', true)
      } else {
        check(id, () => parse(cr.Document.read(text, { format, limits }).write()), expect, 'document', true)
      }
      return true
    }
    case 'condition':
    case 'value': {
      const node = cr.Node.read(write(given.get(kind)))
      const strict = given.get('strict') === true
      const input = write(given.get('input'))
      check(
        id,
        () =>
          parse(
            kind === 'value'
              ? node.evaluateValueTreeJson(input, { strict })
              : node.evaluateTreeJson(input, { logic: given.get('logic'), strict }),
          ),
        expect,
        'result',
      )
      return true
    }
    case 'evaluate': {
      const ruleset = cr.RuleSet.read(document(given.get('ruleset')))
      if (given.has('inputs')) {
        check(
          id,
          () => ruleset.evaluateManyJson(write(given.get('inputs'))).map((d) => contentOf(d.write(), 'decision')),
          expect,
          'decisions',
        )
      } else {
        check(id, () => contentOf(ruleset.evaluateJson(write(given.get('input'))).write(), 'decision'), expect, 'decision')
      }
      return true
    }
    case 'explain': {
      const ruleset = cr.RuleSet.read(document(given.get('ruleset')))
      const decision = () => ruleset.evaluateJson(write(given.get('input')))
      check(id, () => contentOf(decision().write(), 'decision'), expect, 'decision')
      check(id, () => contentOf(decision().explain('json'), 'explanation'), expect, 'explanation')
      if (expect.has('text')) assert.equal(decision().explain(), expect.get('text'), id)
      return true
    }
    case 'summary': {
      const ruleset = cr.RuleSet.read(document(given.get('ruleset')))
      check(id, () => contentOf(ruleset.summary('json'), 'summary'), expect, 'summary')
      if (expect.has('text')) assert.equal(ruleset.summary('text'), expect.get('text'), id)
      return true
    }
    default:
      return false
  }
}

function runNative(id, given, expect) {
  const kind = given.get('kind')
  let input
  try {
    if (given.has('inputs')) input = given.get('inputs').map(native)
    else if (given.has('input')) input = native(given.get('input'))
    else return false
  } catch (error) {
    if (error instanceof Skip) return false
    throw error
  }
  switch (kind) {
    case 'condition':
    case 'value': {
      const node = cr.Node.read(write(given.get(kind)))
      const strict = given.get('strict') === true
      check(
        id,
        () =>
          parse(
            kind === 'value'
              ? node.evaluateValueTree(input, { strict })
              : node.evaluateTree(input, { logic: given.get('logic'), strict }),
          ),
        expect,
        'result',
      )
      return true
    }
    case 'evaluate': {
      const ruleset = cr.RuleSet.read(document(given.get('ruleset')))
      if (given.has('inputs')) {
        check(id, () => ruleset.evaluateMany(input).map((d) => contentOf(d.write(), 'decision')), expect, 'decisions')
      } else {
        check(id, () => contentOf(ruleset.evaluate(input).write(), 'decision'), expect, 'decision')
      }
      return true
    }
    case 'explain': {
      const ruleset = cr.RuleSet.read(document(given.get('ruleset')))
      check(id, () => contentOf(ruleset.evaluate(input).explain('json'), 'explanation'), expect, 'explanation')
      return true
    }
    default:
      return false
  }
}

const vectors = FILES.flatMap((file) => parse(fs.readFileSync(path.join(VECTORS, file), 'utf8')).get('vectors'))

test('the smoke vectors pass through JSON', () => {
  let ran = 0
  for (const vector of vectors) {
    if (runJson(vector.get('id'), vector.get('given'), vector.get('expect'))) ran += 1
  }
  // Every vector of these files that the smoke runner handles (F.3 runs all).
  assert.ok(ran >= 200, `only ${ran} vectors ran`)
})

test('the smoke vectors pass through native data', () => {
  let ran = 0
  for (const vector of vectors) {
    if (runNative(vector.get('id'), vector.get('given'), vector.get('expect'))) ran += 1
  }
  assert.ok(ran >= 70, `only ${ran} vectors ran`)
})
