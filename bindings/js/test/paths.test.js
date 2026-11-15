// The `max_path_segments` limit on the paths of fields (spec/json-format.md,
// *Errors and limits*), and the depth of the data read along them.

import assert from 'node:assert/strict'
import path from 'node:path'
import { describe, test } from 'node:test'
import { pathToFileURL } from 'node:url'
import { Worker } from 'node:worker_threads'

import { build, deno, root } from './support/cr.js'

const { FormatError, Rule, RuleSet, field, ruleFromText } = root

/** A path of `n` segments, keys and indexes in turns. */
const longPath = (n) => Array.from({ length: n }, (_, i) => (i % 2 === 0 ? 'k' : 0))

/** Data as deep as `longPath(n)`, with 1 at its end. */
function along(n) {
  let data = 1
  for (let i = n - 1; i >= 0; i -= 1) {
    data = i % 2 === 0 ? { k: data } : [data]
  }
  return data
}

/** Whether `call` throws the core's `limit_exceeded` for `max_path_segments`. */
function beyond(call) {
  assert.throws(call, (error) => {
    assert.ok(error instanceof FormatError, String(error))
    assert.equal(error.code, 'limit_exceeded')
    assert.equal(error.context.name, 'max_path_segments')
    assert.ok(error.hint)
    return true
  })
}

describe('max_path_segments', () => {
  test('a field has at most 256 segments, the default', () => {
    field(...longPath(256))
    beyond(() => field(...longPath(257)))
  })

  test('readers take the limit', () => {
    beyond(() => ruleFromText('IF a.b.c THEN y = 1', { limits: { maxPathSegments: 2 } }))
    ruleFromText('IF a.b.c THEN y = 1', { limits: { maxPathSegments: 3 } })
    assert.throws(() => ruleFromText('IF a THEN y = 1', { limits: { maxPathSegments: -1 } }), RangeError)
  })

  test('a path at the limit over data as deep is evaluated', () => {
    const at = longPath(256)
    const rules = new RuleSet([new Rule('r', field(...at).eq(1), { y: field(...at) })])
    assert.equal(rules.evaluate(along(256)).selected.length, 1)
    // A value converted whole where the path ends adds up to 256 containers.
    let end = 1
    for (let i = 0; i < 255; i += 1) end = [end]
    const copy = new RuleSet([new Rule('r', field(...at).isNull().not(), { y: field(...at) })])
    const data = along(256)
    let parent = data
    for (const segment of at.slice(0, -1)) parent = parent[segment]
    parent[at.at(-1)] = end
    const decision = copy.evaluate(data)
    assert.equal(decision.selected.length, 1)
    let depth = 0
    for (let value = decision.output.y; Array.isArray(value); value = value[0]) depth += 1
    assert.equal(depth, 255)
  })

  // The worst case at the default limits, 511 levels: a path of 256 segments
  // and a list 255 deep converted whole where it ends, given back as the
  // payload. Reading the data, the encodings and the output use explicit
  // stacks, so the depth costs no stack: on macOS arm64 the worker needs
  // about 0.3 MiB just to load the package, with a path of 1 segment or of
  // 256. 0.5 MiB is an eighth of Node's default worker stack.
  test('and in a worker thread with a small stack', { skip: deno }, async () => {
    const js = path.join(import.meta.dirname, '..')
    const url = pathToFileURL(path.join(js, build === 'wasm' ? 'index.wasm.js' : 'index.js')).href
    const code = `
      const { parentPort } = require('node:worker_threads')
      import(${JSON.stringify(url)}).then(({ Rule, RuleSet, field }) => {
        const at = Array.from({ length: 256 }, (_, i) => (i % 2 === 0 ? 'k' : 0))
        let data = 1
        for (let i = 0; i < 255; i += 1) data = [data]
        for (let i = 255; i >= 0; i -= 1) data = i % 2 === 0 ? { k: data } : [data]
        const rules = new RuleSet([new Rule('r', field(...at).isNull().not(), { y: field(...at) })])
        const decision = rules.evaluate(data)
        let depth = 0
        for (let value = decision.output.y; Array.isArray(value); value = value[0]) depth += 1
        parentPort.postMessage([decision.selected.length, depth])
      })
    `
    const worker = new Worker(code, { eval: true, resourceLimits: { stackSizeMb: 0.5 } })
    const result = await new Promise((resolve, reject) => {
      worker.once('message', resolve)
      worker.once('error', reject)
    })
    await worker.terminate()
    assert.deepEqual(result, [1, 255])
  })
})
