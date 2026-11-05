// After `napi build`: the classes' constructors only throw (their objects come
// from factories, as in the WebAssembly build), so their declarations in
// `binding.d.ts` are made private, and TypeScript rejects `new RuleSet()`.

import fs from 'node:fs'
import path from 'node:path'

const file = path.join(import.meta.dirname, '..', 'binding.d.ts')
const text = fs.readFileSync(file, 'utf8')
fs.writeFileSync(file, text.replace(/^( {2})constructor\(\)$/gm, '$1private constructor()'))
