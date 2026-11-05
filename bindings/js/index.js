// cleanrules for Node, on the native addon: the idiomatic API (conditions,
// rules, rule sets, decisions, explanations, summaries, result trees,
// decision tables and documents), the errors, the marker values and the
// versions. The native classes are in
// `cleanrules/native`. `index.wasm.js` is the same on the WebAssembly module.

import { api } from './lib/api/index.js'
import native from './lib/napi.js'

export * from './lib/public.js'
export const FORMAT_VERSION = native.FORMAT_VERSION
export const version = native.version()
export const {
  Const,
  Decision,
  DecisionTable,
  Expr,
  F,
  Field,
  Rule,
  RuleSet,
  and,
  constant,
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
} = api(native, 'Node addon')
