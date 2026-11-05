// `import` under `node --conditions=browser`: the WebAssembly build, which
// needs no addon (and is what runs where no platform package fits).

import { createRequire } from 'node:module'

import * as api from 'cleanrules'
import * as native from 'cleanrules/native'

const { checkNoAddon, checkResolved, evaluate } = createRequire(import.meta.url)('./common.cjs')

checkResolved(import.meta.resolve('cleanrules'), 'index.wasm.js', 'import cleanrules (browser)')
checkResolved(import.meta.resolve('cleanrules/native'), 'native.wasm.js', 'import cleanrules/native (browser)')
evaluate(api, native, 'browser')
checkNoAddon('browser')
console.log(`--conditions=browser: cleanrules ${api.version} on WebAssembly`)
