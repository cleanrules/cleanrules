// `cleanrules/wasm`: the WebAssembly build, initialised by hand, for bundlers
// and CDNs that serve the module from their own URL, and for pages that load
// it when they need it. It has the exports of `cleanrules` and the native
// classes (in `native`, whose `RuleSet` and `Decision` are not the API's),
// plus `init(source)`:
//
//   import { init, F, Rule, RuleSet } from 'cleanrules/wasm'
//   await init(new URL('cleanrules_bg.wasm', import.meta.url))
//
// No top-level await and no runtime module: importing it does nothing. Until
// `init` resolves, what needs the module (the API of the build, `decimal`,
// `native`) throws an `Error` (code `ERR_CLEANRULES_NOT_INITIALISED`) that
// says to call `init` first; `version` and `FORMAT_VERSION` are `undefined`.
// A copy of an export taken before `init` works once it resolves. The errors,
// the marker classes and the classes of results work at once. Calling `init`
// again is harmless.

import * as glue from './wasm/cleanrules.js'
import { api } from './lib/api/index.js'
import { decimal as markerDecimal } from './lib/values.js'
import { ready } from './lib/wasm-ready.js'

export * from './lib/public.js'

const INSPECT = Symbol.for('nodejs.util.inspect.custom')
const DENO_INSPECT = Symbol.for('Deno.customInspect')

/** The `code` of the error of using the entry before `init` resolves. */
const NOT_INITIALISED = 'ERR_CLEANRULES_NOT_INITIALISED'

// The real value of each stand-in, once `init` has set it.
const reals = new Map()

/**
 * A stand-in for the export `name` until `init` resolves. Before, calling
 * it, constructing it, or reading, writing, listing or describing its
 * properties throws an `Error` (code `ERR_CLEANRULES_NOT_INITIALISED`);
 * inspecting it works. After, it forwards everything to the real value, so
 * that a copy taken before `init` (`const { F } = await
 * import('cleanrules/wasm')`) works too; the exports themselves become the
 * real values.
 */
function pending(name) {
  const notReady = () => {
    const error = new Error(
      `cleanrules/wasm is not initialised: call await init(source) before using ${name} ` +
        '(source is the URL, Response, bytes or WebAssembly.Module of cleanrules_bg.wasm)',
    )
    error.code = NOT_INITIALISED
    return error
  }
  const real = () => {
    if (!reals.has(name)) throw notReady()
    return reals.get(name)
  }
  const show = () => `[${name}: call init() first]`
  // A bound function: callable and constructible, with no own property that
  // cannot be configured, so that the proxy may report the real value's.
  const target = function () {}.bind(null)
  const proxy = new Proxy(target, {
    apply(_, self, args) {
      return Reflect.apply(real(), self, args)
    },
    construct(_, args, newTarget) {
      const found = real()
      return Reflect.construct(found, args, newTarget === proxy ? found : newTarget)
    },
    get(_, key) {
      if (!reals.has(name) && (key === INSPECT || key === DENO_INSPECT)) return show
      const found = real()
      return Reflect.get(found, key, found)
    },
    set(_, key, value) {
      const found = real()
      return Reflect.set(found, key, value, found)
    },
    has(_, key) {
      return Reflect.has(real(), key)
    },
    ownKeys() {
      return Reflect.ownKeys(real())
    },
    getOwnPropertyDescriptor(_, key) {
      const descriptor = Reflect.getOwnPropertyDescriptor(real(), key)
      // A proxy cannot report a property it lacks as non-configurable.
      return descriptor === undefined ? undefined : { ...descriptor, configurable: true }
    },
    defineProperty(_, key, descriptor) {
      return Reflect.defineProperty(real(), key, descriptor)
    },
    deleteProperty(_, key) {
      return Reflect.deleteProperty(real(), key)
    },
    getPrototypeOf() {
      return Reflect.getPrototypeOf(real())
    },
  })
  return proxy
}

export let FORMAT_VERSION
export let version
export let native = pending('native')
export let decimal = pending('decimal')
export let Const = pending('Const')
export let Decision = pending('Decision')
export let DecisionTable = pending('DecisionTable')
export let Expr = pending('Expr')
export let F = pending('F')
export let Field = pending('Field')
export let Rule = pending('Rule')
export let RuleSet = pending('RuleSet')
export let and = pending('and')
export let constant = pending('constant')
export let dump = pending('dump')
export let dumps = pending('dumps')
export let evaluateTree = pending('evaluateTree')
export let evaluateValueTree = pending('evaluateValueTree')
export let field = pending('field')
export let load = pending('load')
export let loads = pending('loads')
export let not = pending('not')
export let or = pending('or')
export let ruleFromText = pending('ruleFromText')

/** Make every export the build's, once the module is instantiated. */
function settle(layer) {
  const built = api(layer, 'WebAssembly')
  const values = {
    ...built,
    native: Object.freeze({
      Document: layer.Document,
      RuleSet: layer.RuleSet,
      Decision: layer.Decision,
      Node: layer.Node,
      FORMAT_VERSION: layer.FORMAT_VERSION,
    }),
    decimal: markerDecimal,
  }
  for (const [name, value] of Object.entries(values)) reals.set(name, value)
  ;({ Const, Decision, DecisionTable, Expr, F, Field, Rule, RuleSet, native, decimal } = values)
  ;({ and, constant, dump, dumps, evaluateTree, evaluateValueTree, field, load, loads, not, or, ruleFromText } = values)
  FORMAT_VERSION = layer.FORMAT_VERSION
  version = layer.version()
}

/** Whether `source` is a `file:` URL, as a `URL` or a string. */
function isFileUrl(source) {
  if (typeof URL === 'function' && source instanceof URL) return source.protocol === 'file:'
  return typeof source === 'string' && source.startsWith('file:')
}

/**
 * What wasm-bindgen's loader takes: `source`, or the module next to this file
 * when it is left out; in Node, whose `fetch` reads no `file:` URL, a file is
 * read from disk (through `process.getBuiltinModule`, without an import).
 */
function moduleSource(source) {
  const given = source ?? new URL('./wasm/cleanrules_bg.wasm', import.meta.url)
  const builtin = globalThis.process?.getBuiltinModule
  if (typeof globalThis.Deno !== 'object' && typeof builtin === 'function' && isFileUrl(given)) {
    return builtin('node:fs').readFileSync(new URL(given))
  }
  return given
}

let initialising = null

/**
 * Load and instantiate the WebAssembly module, then make every export ready.
 * `source` is where the module is: a URL (a `URL` or a string), a `Request`,
 * a `Response` (or a promise of one), its bytes (an `ArrayBuffer` or a typed
 * array) or a compiled `WebAssembly.Module`; left out, the file next to the
 * package's glue (`wasm/cleanrules_bg.wasm`). Calling it again returns the
 * same promise (a failed call can be retried); the module of another entry
 * point in the same realm is shared.
 */
export function init(source) {
  if (initialising === null) {
    initialising = (async () => {
      await glue.default({ module_or_path: moduleSource(source) })
      settle(ready(glue))
    })()
    initialising.catch(() => {
      initialising = null
    })
  }
  return initialising
}
