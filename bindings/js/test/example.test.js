'use strict'

// The example in docs/examples runs and prints what it says.

const assert = require('node:assert/strict')
const { execFileSync } = require('node:child_process')
const path = require('node:path')
const { test } = require('node:test')

test('docs/examples/discounts.js', () => {
  const examples = path.join(__dirname, '..', '..', '..', 'docs', 'examples')
  const output = execFileSync(process.execPath, [path.join(examples, 'discounts.js'), path.join(examples, 'discounts.rules')], {
    encoding: 'utf8',
  })
  assert.match(output, /^output: \{"discount":\{"\$decimal":"0\.15"\}\}/)
  assert.match(output, /HIT POLICY priority: selected vip/)
  assert.match(output, /same decision from JSON: true/)
  assert.match(output, /notify_sales for vip/)
  assert.match(output, /missing_field in rule adult: customer\.age/)
})
