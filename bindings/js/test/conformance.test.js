// The conformance vectors (conformance/README.md) on the build under test
// (the Node addon, or the WebAssembly module with `npm run test:wasm` and in
// Deno): every vector of conformance/vectors, through JSON (`json:<id>`, the
// native layer) and through native JavaScript data (`native:<id>`, the
// idiomatic API). test/support/conformance.js says what each route calls and
// compares. Every failure is reported with its id, what came back and what
// the vector expects; the vectors a route cannot run, and the native inputs
// it cannot represent exactly, are counted and listed.

import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import test from 'node:test'

import { build, deno, native, root } from './support/cr.js'
import { MINIMUM_VECTORS, SKIPS, checkComparison, conformance } from './support/conformance.js'
import { Obj, parse, write } from './support/json.js'

const VECTORS = path.join(import.meta.dirname, '..', '..', '..', 'conformance', 'vectors')
const ROUTES = ['json', 'native']

/** The vector files, in the order of their paths. */
function vectorFiles(dir) {
  const found = []
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name)
    if (entry.isDirectory()) found.push(...vectorFiles(file))
    else if (entry.name.endsWith('.json')) found.push(file)
  }
  return found.sort()
}

const runner = conformance(root, native)
const files = vectorFiles(VECTORS).map((file) => [
  path.relative(VECTORS, file).split(path.sep).join('/'),
  parse(fs.readFileSync(file, 'utf8')).get('vectors'),
])
/** Every vector, by id. */
const vectors = new Map(files.flatMap(([, list]) => list.map((vector) => [vector.get('id'), vector])))
const counts = Object.fromEntries(ROUTES.map((route) => [route, { passed: 0, failed: 0, skipped: new Map() }]))
let ran = 0
const runtime = deno ? `Deno ${globalThis.Deno.version.deno}` : `Node ${process.versions.node}`

/** Print a line of the report (`t.diagnostic` where the runner has it). */
function report(t, line) {
  if (typeof t.diagnostic === 'function') t.diagnostic(line)
  else console.log(line)
}

test('the comparison holds on its own cases', () => {
  assert.deepEqual(checkComparison(), [])
})

for (const [name, list] of files) {
  test(`vectors in ${name}`, () => {
    const failures = []
    for (const vector of list) {
      const id = vector.get('id')
      ran += 1
      for (const route of ROUTES) {
        const outcome = runner.run(vector, route)
        if (outcome === '') {
          counts[route].passed += 1
        } else if (typeof outcome === 'object') {
          const ids = counts[route].skipped.get(outcome.skip) ?? []
          counts[route].skipped.set(outcome.skip, [...ids, id])
        } else {
          counts[route].failed += 1
          failures.push(`${route}:${id}: ${outcome}`)
        }
      }
    }
    assert.ok(failures.length === 0, `${failures.length} failed:\n${failures.join('\n')}`)
  })
}

test('every vector ran both ways, but those listed with their reason', (t) => {
  assert.ok(vectors.size >= MINIMUM_VECTORS, `only ${vectors.size} vectors, expected at least ${MINIMUM_VECTORS}`)
  assert.equal(vectors.size, files.reduce((n, [, list]) => n + list.length, 0), 'vector ids repeat')
  assert.equal(ran, vectors.size, 'every vector file ran')
  for (const route of ROUTES) {
    const { passed, failed, skipped } = counts[route]
    const skips = [...skipped.values()].reduce((n, ids) => n + ids.length, 0)
    report(t, `${route} (${build}, ${runtime}): ${passed} of ${vectors.size} passed, ${failed} failed, ${skips} skipped`)
    for (const [why, ids] of skipped) report(t, `  ${ids.length} skipped: ${SKIPS[why]}: ${ids.join(', ')}`)
    assert.equal(passed + failed + skips, vectors.size)
  }
  const forms = [...runner.forms].map(([form, ids]) => `${form} ${ids.size}`)
  report(t, `native: vectors using each JavaScript form (inputs, constants, metadata, grid cells): ${forms.join(', ')}`)
  for (const [why, ids] of runner.fallbacks) {
    report(t, `native: ${ids.size} vectors whose input no native form holds exactly, ${why}; given as the ZonedDateTime the engine gives back for its text: ${[...ids].join(', ')}`)
  }
  // The only vectors a route skips: grids, and the limits of native data,
  // have no JSON route; conditions under a graded logic run in neither
  // until task J.1b.
  const grids = [...vectors.values()].filter((v) => v.get('given').has('table_grid')).map((v) => v.get('id'))
  const limited = [...vectors.values()]
    .filter((v) => v.get('given').get('kind') === 'evaluate' && v.get('given').has('limits'))
    .map((v) => v.get('id'))
  const graded = [...vectors.values()]
    .filter((v) => v.get('given').has('logic') && v.get('given').get('logic') !== 'boolean')
    .map((v) => v.get('id'))
  assert.deepEqual(
    [...counts.json.skipped.keys()].sort(),
    [
      ...(grids.length > 0 ? ['grid'] : []),
      ...(limited.length > 0 ? ['native'] : []),
      ...(graded.length > 0 ? ['graded'] : []),
    ].sort(),
  )
  assert.deepEqual(counts.json.skipped.get('grid') ?? [], grids)
  assert.deepEqual(counts.json.skipped.get('native') ?? [], limited)
  assert.deepEqual(counts.json.skipped.get('graded') ?? [], graded)
  assert.deepEqual([...counts.native.skipped.keys()], graded.length > 0 ? ['graded'] : [])
  assert.deepEqual(counts.native.skipped.get('graded') ?? [], graded)
})

/** `vector` with `from` replaced by `to` in its expected output only. */
function mutated(id, from, to) {
  const vector = vectors.get(id)
  assert.ok(vector !== undefined, `no vector ${id}`)
  const expect = write(vector.get('expect'))
  assert.ok(expect.includes(from), `${id}: no ${from} in ${expect}`)
  const entries = vector.entries.map(([k, v]) => [k, k === 'expect' ? parse(expect.replace(from, to)) : v])
  return new Obj(entries)
}

test('a wrong expected output fails both ways', () => {
  const wrong = [
    // A value, a truth value, an int that should be a float, an order.
    ['ast/nodes/div-int', '"value":3.5', '"value":3.25'],
    ['ast/missing/missing-field', '"truth":null', '"truth":false'],
    ['rules/batch/decisions', '"value":2}', '"value":2.0}'],
    ['explain/summaries/pricing', '"priority":10', '"priority":11'],
    ['explain/summaries/pricing', '"outputs":["discount","total"]', '"outputs":["total","discount"]'],
    // What the idiomatic API does not give (a cause's `at`) is still checked
    // by the native route, through the native layer.
    ['explain/counterfactuals/at-the-limit', '"at":"/decision/results/0/when"', '"at":"/decision/results/0"'],
    ['explain/counterfactuals/at-the-limit', '"gap":0', '"gap":1'],
    ['ast/missing/strict-missing', '"path":["age"]', '"path":["years"]'],
    ['format/conditions/and-flattened', '"path":["c"]', '"path":["d"]'],
    ['format/tables/basic', 'loyal,> 2,-,0.05', 'loyal,> 3,-,0.05'],
  ]
  for (const [id, from, to] of wrong) {
    const vector = mutated(id, from, to)
    for (const route of ROUTES) {
      assert.notEqual(runner.run(vector, route), '', `${route}:${id} passes with ${to} for ${from}`)
    }
  }
})
