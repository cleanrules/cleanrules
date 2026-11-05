// Registers the JavaScript half of the binding with a build of the engine,
// the Node addon (`lib/napi.js`) or the WebAssembly module (`lib/wasm.js`,
// `lib/wasm.deno.js`): the reader of native data, the error classes and the
// calls to actions.

import { actions, call } from './actions.js'
import { fromNative } from './errors.js'
import { read, useTypes } from './read.js'
import { setDecimalCheck } from './values.js'

/**
 * Register the hooks with `native`. `types` are the runtime's type checks
 * (Node's `util.types`), or `null` where there are none.
 */
export function install(native, types) {
  // The native classes, and those the idiomatic API uses internally.
  const classes = [
    native.Document,
    native.RuleSet,
    native.Decision,
    native.Node,
    native.NodeList,
    native.RuleList,
    native.NativeRule,
    native.NativeTable,
    native.NativeGrid,
    native.NativeTree,
  ]
  // The engine's own objects passed as data are opaque.
  useTypes(types, classes)
  setDecimalCheck(native.decimalProblem)
  native.setHooks({ read, error: fromNative, actions, call })

  // `using` works with both builds: the addon's objects need no release, so
  // their `[Symbol.dispose]()` does nothing (the module's frees the object).
  if (typeof Symbol.dispose === 'symbol') {
    for (const Class of classes) {
      if (!(Symbol.dispose in Class.prototype)) {
        Object.defineProperty(Class.prototype, Symbol.dispose, {
          value() {},
          writable: true,
          configurable: true,
        })
      }
    }
  }
}
