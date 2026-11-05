// Evaluating one node and reading its result tree (spec/ast.md, *Evaluating
// one node*), as Python's `cleanrules._trees`:
//
//   const tree = evaluateTree(F.age.ge(18).and(F.country.isIn(['ES'])), { age: 20 })
//   tree.truth                     // 'unknown': country is missing
//   tree.children[1].reason.code   // 'missing_field'
//
// The core evaluates and builds the tree (`nodeTree` and `ruleTree` of the
// native layer); this module checks the arguments and makes `ResultTree`
// objects of the tree's encoding. It is made once per build.

import { ExpressionError } from '../errors.js'
import { capture } from '../read.js'
import { INTERNAL, ResultTree } from './results.js'
import { options, sameBuild, show } from './shared.js'

/** Evaluating one node, over the native layer of one build (`build` names it). */
function trees(native, expressions, build) {
  const { Expr, nodeOf, wrap } = expressions

  /** The `ResultTree` of a native tree, with the objects of its opaque values. */
  function resultTree(tree, objects) {
    try {
      const [form, json] = JSON.parse(tree.data())
      return new ResultTree(INTERNAL, form, json, { nodes: tree.nodes(), next: 0 }, objects, wrap)
    } finally {
      tree.free?.()
    }
  }

  function nodeArgument(value, what) {
    if (!(value instanceof Expr)) {
      sameBuild(value, build)
      throw new ExpressionError(`${what} takes a node, not ${show(value)}`, {
        hint: 'build it with F, e.g. F.total.gt(100)',
      })
    }
    return nodeOf(value)
  }

  function evaluate(node, data, treeOptions, value) {
    const [tree, objects] = capture(() => native.nodeTree(node, data, treeOptions, value))
    return resultTree(tree, objects[0] ?? [])
  }

  /**
   * Evaluate a condition (or a field or boolean constant used as one) on
   * `data`, read as `RuleSet.evaluate` reads it, and return its result tree.
   * `logic` is the logic's name; `strict` throws an `EvaluationError` instead
   * of giving unknown.
   */
  function evaluateTree(condition, data, treeOptions) {
    const node = nodeArgument(condition, 'evaluateTree')
    if (condition.kind === 'value') {
      throw new ExpressionError(`evaluateTree takes a condition, not the value ${condition}`, {
        hint: 'compare it, or evaluate it with evaluateValueTree',
      })
    }
    const { logic, strict } = options(treeOptions, ['logic', 'strict'], '{ strict: true }')
    return evaluate(node, data, { logic, strict }, false)
  }

  /** Evaluate a value node on `data` and return its result tree. */
  function evaluateValueTree(value, data, treeOptions) {
    const node = nodeArgument(value, 'evaluateValueTree')
    const { strict } = options(treeOptions, ['strict'], '{ strict: true }')
    return evaluate(node, data, { strict }, true)
  }

  return { evaluateTree, evaluateValueTree, resultTree }
}

export { trees }
