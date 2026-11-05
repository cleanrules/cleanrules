// Types of `cleanrules/wasm` (`wasm.js`): the WebAssembly build initialised by
// hand. Its exports are those of `cleanrules` (`index.d.ts`), plus `native`,
// the native classes of `cleanrules/native`, and `init`. What needs the
// module (the API of the build, `decimal`, `native`) throws a
// `NotInitialisedError` until `init` resolves, and `version` and
// `FORMAT_VERSION` are `undefined` until then.

import type * as NativeLayer from './native.js'

export * from './index.js'

/** The version of the package; `undefined` until `init` resolves. */
export declare const version: string | undefined

/** The version of the JSON format; `undefined` until `init` resolves. */
export declare const FORMAT_VERSION: number | undefined

/**
 * The error of using what needs the module before `init` resolves: a plain
 * `Error` (a loading mistake, not one of the engine's error codes) with this
 * `code`.
 */
export interface NotInitialisedError extends Error {
  readonly code: 'ERR_CLEANRULES_NOT_INITIALISED'
}

/** The native classes, as `cleanrules/native` exports them (ready once `init` resolves). */
export declare const native: typeof NativeLayer

/** A global class's instances where the TypeScript libraries in use declare it; otherwise `never`. */
type GlobalInstance<Name extends string> = typeof globalThis extends {
  [K in Name]: abstract new (...args: never[]) => infer Instance
}
  ? Instance
  : never

/** `WebAssembly.Module` where the TypeScript libraries in use declare it; otherwise `never`. */
type WebAssemblyModule = typeof globalThis extends {
  WebAssembly: { Module: abstract new (...args: never[]) => infer Module }
}
  ? Module
  : never

/**
 * Where the module (`cleanrules_bg.wasm`) is: a URL (a string or a `URL`), a
 * `Request`, a `Response` or a promise of one, its bytes, or a compiled
 * `WebAssembly.Module`.
 */
export type InitSource =
  | string
  | GlobalInstance<'URL'>
  | GlobalInstance<'Request'>
  | GlobalInstance<'Response'>
  | PromiseLike<GlobalInstance<'Response'>>
  | ArrayBuffer
  | ArrayBufferView
  | WebAssemblyModule

/**
 * Load and instantiate the WebAssembly module, then make every export ready.
 * Left out, `source` is the file next to the package's glue
 * (`wasm/cleanrules_bg.wasm`). Calling it again returns the same promise.
 *
 * @example
 * import { init, F, Rule, RuleSet } from 'cleanrules/wasm'
 * await init(new URL('cleanrules_bg.wasm', import.meta.url))
 */
export declare function init(source?: InitSource): Promise<void>
