// The example in docs/examples runs and prints what it says.

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'
import { pathToFileURL } from 'node:url'

import { deno, napiBuilt } from './support/cr.js'

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
