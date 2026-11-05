// The WebAssembly entries load in browsers: their module graph has only
// relative ES module imports (so no `node:` module), and uses no Node API
// (`require`, a bare `process` or `Buffer`); `lib/wasm.js` reads the file
// through `globalThis.process?.getBuiltinModule` only where it exists. There is no browser runner here: Deno, which
// runs the whole suite on these entries (`npm run test:deno`), and this static
// check stand in for one.

import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'

const JS = path.join(import.meta.dirname, '..')
const built = fs.existsSync(path.join(JS, 'wasm', 'cleanrules.js'))

const IMPORT = /\b(?:import|export)\b[^'"`;]*?\bfrom\s*['"]([^'"]+)['"]|\bimport\s*['"]([^'"]+)['"]|\bimport\(\s*['"]([^'"]+)['"]\s*\)/g

/** Every module the entry imports, and itself, by path. */
function graph(entry, seen = new Map()) {
  const file = path.resolve(entry)
  if (seen.has(file)) return seen
  const source = fs.readFileSync(file, 'utf8')
  seen.set(file, source)
  for (const match of source.matchAll(IMPORT)) {
    const specifier = match[1] ?? match[2] ?? match[3]
    assert.match(specifier, /^\.\.?\//, `${path.relative(JS, file)} imports ${specifier}`)
    graph(path.join(path.dirname(file), specifier), seen)
  }
  return seen
}

test('the WebAssembly entries import nothing from Node', { skip: !built && 'npm run build:wasm' }, () => {
  for (const entry of ['index.wasm.js', 'native.wasm.js']) {
    const modules = graph(path.join(JS, entry))
    assert.ok(modules.size >= 8, `${entry}: ${modules.size} modules`)
    for (const [file, source] of modules) {
      const name = path.relative(JS, file)
      // Comments may name Node APIs; code may not.
      const code = source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:])\/\/.*$/gm, '$1')
      assert.doesNotMatch(code, /\brequire\(/, `${name} calls require`)
      // Deno's loader (`import source`) is for the `deno` condition only.
      assert.doesNotMatch(code, /\bimport\s+source\b/, `${name} uses a source-phase import`)
      assert.doesNotMatch(code, /(?<![.\w])(process|Buffer|__dirname)\b/, `${name} uses a Node global`)
    }
  }
})

test('the wasm-bindgen glue is the web target, loaded by URL', { skip: !built && 'npm run build:wasm' }, () => {
  const glue = fs.readFileSync(path.join(JS, 'wasm', 'cleanrules.js'), 'utf8')
  assert.match(glue, /export \{[^}]*\binitSync\b/)
  assert.match(glue, /import\.meta\.url/)
})
