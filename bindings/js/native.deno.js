// `cleanrules/native` on the WebAssembly module in Deno: the same exports as
// `native.js`, which uses the Node addon.

import native from './lib/wasm.deno.js'

export const { Document, RuleSet, Decision, Node, FORMAT_VERSION } = native
