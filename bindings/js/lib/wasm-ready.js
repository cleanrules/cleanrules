// The WebAssembly module once instantiated (`lib/wasm.js` in browsers and
// Node, `lib/wasm.deno.js` in Deno): its classes as the native layer, with
// the JavaScript half registered.

import { install } from './install.js'

/** The idiomatic API's internal functions and classes, the same as the Node addon's. */
const INTERNAL = [
  'NativeRule',
  'NodeList',
  'RuleList',
  'actionErrorData',
  'decisionData',
  'explanationData',
  'nodeArith',
  'nodeBetween',
  'nodeChildren',
  'nodeCompare',
  'nodeConnective',
  'nodeConstant',
  'nodeDetails',
  'nodeField',
  'nodeIsIn',
  'nodeMatches',
  'nodeSameAs',
  'nodeStringTest',
  'nodeUnary',
  'ruleDetails',
  'ruleNew',
  'ruleSetNew',
  'summaryWith',
]

/**
 * wasm-bindgen's `free()` releases an object's Rust side, after which any use
 * fails with "null pointer passed to rust". Here a second `free()` (or
 * `[Symbol.dispose]()`) does nothing, and using a freed object is a clear
 * `TypeError`.
 */
function guardFreed(Class) {
  const prototype = Class.prototype
  const freed = () => new TypeError(`this ${Class.name} was released with free() or [Symbol.dispose](); it cannot be used`)
  for (const name of Object.getOwnPropertyNames(prototype)) {
    if (name === 'constructor' || name === '__destroy_into_raw') continue
    const descriptor = Object.getOwnPropertyDescriptor(prototype, name)
    if (name === 'free') {
      const free = descriptor.value
      descriptor.value = function () {
        if (this.__wbg_ptr) free.call(this)
      }
    } else if (typeof descriptor.value === 'function') {
      const method = descriptor.value
      descriptor.value = {
        [name](...args) {
          if (!this.__wbg_ptr) throw freed()
          return method.apply(this, args)
        },
      }[name]
    } else if (descriptor.get) {
      const get = descriptor.get
      descriptor.get = {
        [name]() {
          if (!this.__wbg_ptr) throw freed()
          return get.call(this)
        },
      }[name]
    }
    Object.defineProperty(prototype, name, descriptor)
  }
  if (typeof Symbol.dispose === 'symbol') {
    Object.defineProperty(prototype, Symbol.dispose, { value: prototype.free, writable: true, configurable: true })
  }
}

export { INTERNAL }

/** The native layer of the instantiated module `glue`, ready to use. */
export function ready(glue) {
  const native = {
    Document: glue.Document,
    RuleSet: glue.RuleSet,
    Decision: glue.Decision,
    Node: glue.Node,
    FORMAT_VERSION: glue.formatVersion(),
    version: glue.version,
    decimalProblem: glue.decimalProblem,
    setHooks: glue.setHooks,
  }
  // The idiomatic API's internal functions and classes (`src/wasm/build.rs`).
  for (const name of INTERNAL) {
    native[name] = glue[name]
  }
  for (const Class of [native.Document, native.RuleSet, native.Decision, native.Node, native.NodeList, native.RuleList, native.NativeRule]) {
    guardFreed(Class)
  }
  // wasm-bindgen takes an engine object only by reference, with its own
  // message: the argument is checked here, with the engine's message (the
  // Node addon checks it in Rust).
  const fromDocument = native.RuleSet.fromDocument
  native.RuleSet.fromDocument = {
    fromDocument(document) {
      if (!(document instanceof native.Document)) throw glue.notADocument(document)
      // A released document throws its TypeError here.
      void document.kind
      return fromDocument.call(this, document)
    },
  }.fromDocument
  // Node's (or Deno's) type checks where the runtime has them, without
  // importing anything.
  const builtin = globalThis.process?.getBuiltinModule
  install(native, typeof builtin === 'function' ? builtin('node:util').types : null)
  return native
}
