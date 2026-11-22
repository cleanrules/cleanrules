// cleanrules on the WebAssembly module (browsers and other runtimes):
// the same exports as `index.js`, which uses the Node addon.

import { api } from './lib/api/index.js'
import native from './lib/wasm.js'

export * from './lib/public.js'
export const FORMAT_VERSION = native.FORMAT_VERSION
export const version = native.version()
export const {
  Bundle,
  CaseFile,
  Const,
  Decision,
  DecisionTable,
  Expr,
  F,
  Field,
  Rule,
  RuleSet,
  and,
  canonicalBytes,
  constant,
  digest,
  dump,
  dumps,
  evaluateTree,
  evaluateValueTree,
  field,
  load,
  loads,
  not,
  or,
  ruleFromText,
} = api(native, 'WebAssembly')
