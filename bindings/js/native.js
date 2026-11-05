// `cleanrules/native` on the Node addon: the native layer (task F.1a), the
// classes of the engine with their JavaScript half registered. The idiomatic
// API of phase F.2 sits on top of it; errors and marker values are in
// `cleanrules`. `native.wasm.js` is the same on the WebAssembly module.

import native from './lib/napi.js'

export const { Document, RuleSet, Decision, Node, FORMAT_VERSION } = native
