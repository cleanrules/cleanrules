// The WebAssembly module in Deno (the `deno` condition of `package.json`):
// imported as a module source (Deno 2.6 or later), which needs no permission,
// unlike fetching or reading the file. Browsers and Node never load this
// syntax; they use `lib/wasm.js`.

import source module from '../wasm/cleanrules_bg.wasm'

import * as glue from '../wasm/cleanrules.js'
import { ready } from './wasm-ready.js'

glue.initSync({ module })

export default ready(glue)
