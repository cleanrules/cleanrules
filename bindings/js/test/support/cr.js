// Both entry points, for the tests, of the build under test: the Node addon,
// or the WebAssembly module when CLEANRULES_BUILD is `wasm`
// (`npm run test:wasm`): `cleanrules` (errors, markers, versions) and
// `cleanrules/native` (the native classes).

import fs from 'node:fs'
import path from 'node:path'

/** Whether the tests run in Deno (`npm run test:deno`), not in Node. */
export const deno = typeof globalThis.Deno === 'object'

/**
 * Whether the Node addon is built: a run on the WebAssembly build may go
 * without it (CI's WebAssembly job), and Deno loads the WebAssembly build only.
 */
export const napiBuilt =
  !deno &&
  fs
    .readdirSync(path.join(import.meta.dirname, '..', '..'))
    .some((file) => /^cleanrules\..+\.node$/.test(file))

/** The build under test: `napi` or `wasm`. */
export const build = process.env.CLEANRULES_BUILD === 'wasm' ? 'wasm' : 'napi'

// Deno loads the module through its own entries (`import source`).
const suffix = build === 'wasm' ? (deno ? '.deno.js' : '.wasm.js') : '.js'

/** The root entry of the build under test. */
export const root = await import(`../../index${suffix}`)
/** The `native` entry of the build under test. */
export const native = await import(`../../native${suffix}`)

export default { ...root, ...native }
