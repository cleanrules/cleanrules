// cleanrules for Node, on the native addon: what is final today, the errors,
// the marker values and the versions. The native classes are in
// `cleanrules/native`; the idiomatic TypeScript API arrives here in phase F.2.
// `index.wasm.js` is the same on the WebAssembly module.

import native from './lib/napi.js'

export * from './lib/public.js'
export const FORMAT_VERSION = native.FORMAT_VERSION
export const version = native.version()
