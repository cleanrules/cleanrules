// `cleanrules/native` on the WebAssembly module: the same exports as
// `native.js`, which uses the Node addon.

import native from './lib/wasm.js'

export const { Document, RuleSet, Decision, Node, FORMAT_VERSION } = native
