// The idiomatic API of one build (the Node addon or the WebAssembly module):
// conditions, rules, rule sets, decisions, result trees, decision tables and
// documents over its native layer. Each entry point makes it for its build,
// once per native layer (`cleanrules` and `cleanrules/wasm` share it in one
// realm); the classes of what decisions, explanations and summaries give back
// (`results.js`) and the errors and markers are shared.

import { documents } from './documents.js'
import { expressions } from './expr.js'
import { rules } from './rules.js'
import { tables } from './tables.js'
import { trees } from './trees.js'

/** The names of the API of a build, as each entry point exports them. */
export const NAMES = Object.freeze([
  'Const',
  'Decision',
  'DecisionTable',
  'Expr',
  'F',
  'Field',
  'Rule',
  'RuleSet',
  'and',
  'constant',
  'dump',
  'dumps',
  'evaluateTree',
  'evaluateValueTree',
  'field',
  'load',
  'loads',
  'not',
  'or',
  'ruleFromText',
])

const made = new WeakMap()

/**
 * The idiomatic API over `native`, the native layer of one build; `build`
 * names it in messages (`'Node addon'` or `'WebAssembly'`).
 */
export function api(native, build) {
  let found = made.get(native)
  if (found === undefined) {
    const built = expressions(native, build)
    const treeApi = trees(native, built, build)
    const ruleApi = rules(native, built, treeApi, build)
    const tableApi = tables(native, built, ruleApi, build)
    const documentApi = documents(native, built, ruleApi, tableApi, build)
    const { Const, Expr, F, Field, and, constant, field, not, or } = built
    const { Decision, Rule, RuleSet } = ruleApi
    const { DecisionTable } = tableApi
    const { evaluateTree, evaluateValueTree } = treeApi
    const { dump, dumps, load, loads, ruleFromText } = documentApi
    found = Object.freeze({
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
    })
    made.set(native, found)
  }
  return found
}
