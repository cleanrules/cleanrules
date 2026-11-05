// The WebAssembly module (`wasm/`, written by wasm-bindgen with `--target
// web`), instantiated once per module instance (a worker loads its own). It
// imports no runtime module, so it loads in browsers and Node alike: the
// `browser` and `default` conditions of `package.json` pick the entries that
// import it. Deno has its own loader (`lib/wasm.deno.js`).

import * as glue from '../wasm/cleanrules.js'
import { ready } from './wasm-ready.js'

const url = new URL('../wasm/cleanrules_bg.wasm', import.meta.url)
// Node's `fetch` does not read `file:` URLs: there the module is read from
// disk, through `process.getBuiltinModule`, which needs no import. Browsers
// fetch it.
const builtin = globalThis.process?.getBuiltinModule
if (url.protocol === 'file:' && typeof builtin === 'function') {
  glue.initSync({ module: builtin('node:fs').readFileSync(url) })
} else {
  await glue.default({ module_or_path: url })
}

export default ready(glue)
