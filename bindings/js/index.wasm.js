// cleanrules on the WebAssembly module (browsers and other runtimes):
// the same exports as `index.js`, which uses the Node addon.

import native from './lib/wasm.js'

export * from './lib/public.js'
export const FORMAT_VERSION = native.FORMAT_VERSION
export const version = native.version()
