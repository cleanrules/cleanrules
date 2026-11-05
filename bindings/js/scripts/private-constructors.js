// After `napi build`: the classes' constructors only throw (their objects come
// from factories, as in the WebAssembly build), so their declarations in
// `binding.d.ts` are made private, and TypeScript rejects `new RuleSet()`.

import fs from 'node:fs'
import path from 'node:path'

const file = path.join(import.meta.dirname, '..', 'binding.d.ts')
const text = fs.readFileSync(file, 'utf8')
// The idiomatic API's list builders (`src/napi/build.rs`) are made with `new`.
const PUBLIC = new Set(['NodeList', 'RuleList'])
let current = null
const lines = text.split('\n').map((line) => {
  const declared = /^export declare class (\w+)/.exec(line)
  if (declared) current = declared[1]
  return !PUBLIC.has(current) && /^ {2}constructor\(\)$/.test(line) ? line.replace('constructor()', 'private constructor()') : line
})
fs.writeFileSync(file, lines.join('\n'))
