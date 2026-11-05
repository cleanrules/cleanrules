// The example in docs/examples runs and prints what it says.

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'
import { pathToFileURL } from 'node:url'

import { deno, napiBuilt, root } from './support/cr.js'

// The example is CommonJS for Node, on the native addon.
test('docs/examples/discounts.js', { skip: !napiBuilt }, () => {
  const examples = path.join(import.meta.dirname, '..', '..', '..', 'docs', 'examples')
  const output = execFileSync(process.execPath, [path.join(examples, 'discounts.js'), path.join(examples, 'discounts.rules')], {
    encoding: 'utf8',
  })
  assert.match(output, /^output: \{"discount":\{"\$decimal":"0\.15"\}\}/)
  assert.match(output, /HIT POLICY priority: selected vip/)
  assert.match(output, /same decision from JSON: true/)
  assert.match(output, /notify_sales for vip/)
  assert.match(output, /missing_field in rule adult: customer\.age/)
})

// The ES module example for Deno, on the WebAssembly build.
test('docs/examples/discounts.deno.js', { skip: !deno }, async () => {
  const examples = path.join(import.meta.dirname, '..', '..', '..', 'docs', 'examples')
  const { main } = await import(pathToFileURL(path.join(examples, 'discounts.deno.js')).href)
  const output = main(fs.readFileSync(path.join(examples, 'discounts.rules'), 'utf8')).join('\n')
  assert.match(output, /^output: \{"discount":\{"\$decimal":"0\.15"\}\}/)
  assert.match(output, /HIT POLICY priority: selected vip/)
  assert.match(output, /same decision from JSON: true/)
  assert.match(output, /notify_sales for vip/)
  assert.match(output, /missing_field in rule adult: customer\.age/)
})

// The idiomatic API example, on the build under test (Node addon, WebAssembly
// in Node, WebAssembly in Deno), and run directly on Node's addon.
test('docs/examples/discounts.mjs', async () => {
  const examples = path.join(import.meta.dirname, '..', '..', '..', 'docs', 'examples')
  const { main } = await import(pathToFileURL(path.join(examples, 'discounts.mjs')).href)
  const check = (output) => {
    assert.match(output, /^discount: 0\.15\nfired: vip, loyal\n/)
    assert.match(output, /HIT POLICY priority: selected vip/)
    assert.match(output, /fired, but vip has a higher priority \(10 > 5\)/)
    assert.match(output, /vip: order\.total > 1000 is false; needs gt, short by 100\.00/)
    assert.match(output, /discount DO notify_sales/)
    assert.match(output, /notify_sales for vip: 0\.15/)
    assert.match(output, /missing_field in rule adult: customer\.age/)
  }
  check(main(root).join('\n'))
  if (napiBuilt) {
    check(execFileSync(process.execPath, [path.join(examples, 'discounts.mjs')], { encoding: 'utf8' }))
  }
})

// The documents and tables example, on the build under test (it reads
// risk.csv with `load`, so Node and Deno only), and run directly on Node's
// addon.
test('docs/examples/risk.mjs', async () => {
  const examples = path.join(import.meta.dirname, '..', '..', '..', 'docs', 'examples')
  const { main } = await import(pathToFileURL(path.join(examples, 'risk.mjs')).href)
  const check = (output) => {
    assert.match(output, /^risk: medium, discount: 0\.05\nHIT POLICY priority: selected big\n/)
    assert.match(output, /same table: true\nrows: new, vip, big, default\n/)
    assert.match(output, /version: 1\ndecision_table:\n/)
    assert.match(output, /vip: unknown; order\.total > 1000 is unknown \(missing_field\)/)
    assert.match(output, /notify for vip: customer\.years > 5 is true$/)
  }
  check((await main(root)).join('\n'))
  if (napiBuilt) {
    check(execFileSync(process.execPath, [path.join(examples, 'risk.mjs')], { encoding: 'utf8' }).trim())
  }
})
