// The idiomatic API of one build (the Node addon or the WebAssembly module):
// conditions, rules, rule sets and decisions over its native layer. Each
// entry point makes it once for its build; the classes of what decisions,
// explanations and summaries give back (`results.js`) and the errors and
// markers are shared.

import { expressions } from './expr.js'
import { rules } from './rules.js'

/**
 * The idiomatic API over `native`, the native layer of one build; `build`
 * names it in messages (`'Node addon'` or `'WebAssembly'`).
 */
export function api(native, build) {
  const built = expressions(native, build)
  const { Decision, Rule, RuleSet } = rules(native, built, build)
  const { Const, Expr, F, Field, and, constant, field, not, or } = built
  return { Const, Decision, Expr, F, Field, Rule, RuleSet, and, constant, field, not, or }
}
