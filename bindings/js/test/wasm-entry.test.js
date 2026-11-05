// `cleanrules/wasm` (task F.2b): the WebAssembly build initialised by hand.
// Each test file runs in its own process in Node, so nothing has initialised
// this entry before; in Deno, where the files share one, nothing else imports
// it.

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createRequire } from 'node:module'
import fs from 'node:fs'
import path from 'node:path'
import { describe, test } from 'node:test'
import { pathToFileURL } from 'node:url'

import { deno, root } from './support/cr.js'

const JS = path.join(import.meta.dirname, '..')
const built = fs.existsSync(path.join(JS, 'wasm', 'cleanrules_bg.wasm'))
const skip = !built && 'npm run build:wasm'
const ENTRY = pathToFileURL(path.join(JS, 'wasm.js')).href

/** What needs the module: it throws until `init` resolves. */
const PENDING = [
  'Const',
  'Decision',
  'DecisionTable',
  'Expr',
  'F',
  'Field',
  'Rule',
  'RuleSet',
  'and',
  'constant',
  'decimal',
  'dump',
  'dumps',
  'evaluateTree',
  'evaluateValueTree',
  'field',
  'load',
  'loads',
  'native',
  'not',
  'or',
  'ruleFromText',
]

const notReady = (name) => (error) => {
  assert.ok(error instanceof Error && error.constructor === Error, String(error))
  assert.equal(error.code, 'ERR_CLEANRULES_NOT_INITIALISED')
  assert.match(error.message, new RegExp(`call await init\\(source\\) before using ${name} `))
  return true
}

describe('cleanrules/wasm', { skip }, () => {
  test('before init, what needs the module throws; then init makes every export ready', async () => {
    const wasm = await import(ENTRY)
    // The root's exports, the native classes and init.
    assert.deepEqual(Object.keys(wasm).sort(), [...Object.keys(root), 'init', 'native'].sort())
    for (const name of PENDING) {
      const value = wasm[name]
      assert.throws(() => value(), notReady(name), name)
      assert.throws(() => new value(), notReady(name), name)
      assert.throws(() => value.anything, notReady(name), name)
      assert.throws(() => {
        value.x = 1
      }, notReady(name))
      assert.equal(value[Symbol.for('nodejs.util.inspect.custom')](), `[${name}: call init() first]`)
      assert.throws(() => value[Symbol.toStringTag], notReady(name), name)
      assert.throws(() => 'x' in value, notReady(name), name)
      assert.throws(() => Object.keys(value), notReady(name), name)
      assert.throws(() => Object.getOwnPropertyDescriptor(value, 'x'), notReady(name), name)
      assert.throws(() => Object.defineProperty(value, 'x', { value: 1 }), notReady(name), name)
      assert.throws(() => delete value.x, notReady(name), name)
      assert.throws(() => Object.getPrototypeOf(value), notReady(name), name)
    }
    assert.throws(() => ({}) instanceof wasm.Rule, notReady('Rule'))
    assert.throws(() => wasm.F.x.gt(1), notReady('F'))
    assert.equal(wasm.version, undefined)
    assert.equal(wasm.FORMAT_VERSION, undefined)
    // What needs no module works at once.
    assert.equal(wasm.date(2026, 1, 2).toString(), '2026-01-02')
    assert.ok(new wasm.MissingFieldError('m') instanceof wasm.CleanRulesError)
    assert.equal(new wasm.TableOutput('y', [1]).name, 'y')

    // Copies taken before init: destructured from the namespace, and (Node)
    // from require.
    const { F: earlyF, Rule: EarlyRule, RuleSet: EarlyRuleSet, native: earlyNative, decimal: earlyDecimal, loads: earlyLoads } = wasm
    const required = deno ? null : createRequire(import.meta.url)(path.join(JS, 'wasm.js'))
    const { F: requiredF, RuleSet: RequiredRuleSet } = required ?? wasm

    // Left out, the source is the module next to the glue (read from disk in
    // Node, fetched in Deno).
    const first = wasm.init()
    assert.equal(wasm.init(new Uint8Array(0)), first)
    assert.equal(await first, undefined)
    assert.equal(wasm.version, root.version)
    assert.equal(wasm.FORMAT_VERSION, 1)
    const rules = new wasm.RuleSet([new wasm.Rule('a', wasm.F.x.gt(1), { v: wasm.F.x })])
    assert.deepEqual(rules.evaluate({ x: 2 }).output, { v: 2 })
    assert.equal(String(wasm.decimal('0.10')), '0.10')
    assert.equal(wasm.native.RuleSet.read('RULE a IF x > 1 THEN v = x', { format: 'text' }).evaluate({ x: 2 }).outputJson(), '{"v":2}')
    assert.deepEqual(Object.keys(wasm.native).sort(), ['Decision', 'Document', 'FORMAT_VERSION', 'Node', 'RuleSet'])
    assert.ok(Object.isFrozen(wasm.native))
    assert.equal(String(wasm.loads('{"version": 1, "condition": {"op": "field", "path": ["x"]}}')), 'x')
    // The exports are the real values; copies taken before forward to them.
    assert.notEqual(wasm.F, earlyF)
    assert.equal(String(earlyF.x.gt(1)), 'x > 1')
    const early = new EarlyRuleSet([new EarlyRule('a', earlyF.x.gt(1), { v: earlyF.x })])
    assert.ok(early instanceof wasm.RuleSet && early instanceof EarlyRuleSet)
    assert.deepEqual(early.evaluate({ x: 2 }).output, { v: 2 })
    assert.equal(earlyNative.RuleSet, wasm.native.RuleSet)
    assert.equal(earlyNative.RuleSet.read('RULE a IF x > 1', { format: 'text' }).length, 1)
    assert.equal(String(earlyDecimal('0.10')), '0.10')
    assert.equal(String(earlyLoads('{"version": 1, "condition": {"op": "field", "path": ["x"]}}')), 'x')
    assert.equal(String(requiredF.y.lt(2)), 'y < 2')
    assert.ok(new RequiredRuleSet([]) instanceof wasm.RuleSet)
    assert.deepEqual(Object.keys(earlyNative).sort(), Object.keys(wasm.native).sort())
    assert.ok('RuleSet' in earlyNative && Object.getPrototypeOf(EarlyRuleSet) === Object.getPrototypeOf(wasm.RuleSet))
    assert.equal(Object.getOwnPropertyDescriptor(earlyNative, 'RuleSet').value, wasm.native.RuleSet)
    assert.equal(Reflect.defineProperty(earlyNative, 'x', { value: 1 }), false)
    assert.equal(Reflect.deleteProperty(earlyNative, 'RuleSet'), false)
    assert.throws(() => {
      earlyNative.x = 1
    }, TypeError)
    // A second call is harmless, and the build is shared with `cleanrules` on WebAssembly.
    await wasm.init()
    const entry = deno ? 'index.deno.js' : 'index.wasm.js'
    const shared = await import(pathToFileURL(path.join(JS, entry)).href)
    assert.equal(shared.RuleSet, wasm.RuleSet)
    assert.ok(new shared.Rule('b', shared.F.y) instanceof wasm.Rule)
  })

  // No top-level await: Node can require it (require(esm) refuses a graph that
  // awaits), and importing it loads nothing.
  test('it can be required, and importing it loads nothing', { skip: deno }, () => {
    const required = createRequire(import.meta.url)(path.join(JS, 'wasm.js'))
    assert.equal(typeof required.init, 'function')
  })

  test('a copy taken with require before init works after it', { skip: deno }, () => {
    const code = `
      const fs = require('node:fs')
      const { F, RuleSet, Rule, init } = require(${JSON.stringify(path.join(JS, 'wasm.js'))})
      let before = null
      try { F.x } catch (error) { before = error.code }
      init(fs.readFileSync(${JSON.stringify(path.join(JS, 'wasm', 'cleanrules_bg.wasm'))})).then(() => {
        const decision = new RuleSet([new Rule('a', F.x.gt(1))]).evaluate({ x: 2 })
        console.log(JSON.stringify([before, decision.fired]))
      })
    `
    const output = execFileSync(process.execPath, ['--no-warnings', '-e', code], { encoding: 'utf8' })
    assert.deepEqual(JSON.parse(output), ['ERR_CLEANRULES_NOT_INITIALISED', ['a']])
  })

  // Each source in its own process: an entry initialises once.
  const sources = {
    bytes: 'fs.readFileSync(file)',
    'an ArrayBuffer': 'new Uint8Array(fs.readFileSync(file)).buffer',
    'a WebAssembly.Module': 'new WebAssembly.Module(fs.readFileSync(file))',
    'a Response': "new Response(fs.readFileSync(file), { headers: { 'content-type': 'application/wasm' } })",
    'a promise of a Response': "Promise.resolve(new Response(fs.readFileSync(file), { headers: { 'content-type': 'application/wasm' } }))",
    'a file: URL': 'url',
    'a file: URL as a string': 'url.href',
  }
  for (const [what, source] of Object.entries(sources)) {
    test(`init takes ${what}`, { skip: deno }, () => {
      const code = `
        import fs from 'node:fs'
        const wasm = await import(${JSON.stringify(ENTRY)})
        const url = new URL('./wasm/cleanrules_bg.wasm', ${JSON.stringify(ENTRY)})
        const file = url
        await wasm.init(${source})
        console.log(String(wasm.F.x.gt(1)))
      `
      const output = execFileSync(process.execPath, ['--input-type=module', '-e', code], { encoding: 'utf8' })
      assert.equal(output.trim(), 'x > 1')
    })
  }

  test('a failed init rejects, and can be tried again', { skip: deno }, () => {
    const code = `
      import fs from 'node:fs'
      const wasm = await import(${JSON.stringify(ENTRY)})
      let failed = false
      try { await wasm.init(new Uint8Array([0, 1, 2])) } catch (error) { failed = error instanceof Error }
      await wasm.init(fs.readFileSync(new URL('./wasm/cleanrules_bg.wasm', ${JSON.stringify(ENTRY)})))
      console.log(JSON.stringify([failed, String(wasm.F.x.gt(1))]))
    `
    const output = execFileSync(process.execPath, ['--input-type=module', '-e', code], { encoding: 'utf8' })
    assert.deepEqual(JSON.parse(output), [true, 'x > 1'])
  })

})
