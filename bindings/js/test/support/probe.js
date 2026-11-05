// What the binding reads: the value tree of one field, through the core.

import cr from './cr.js'
import { parse, write } from './json.js'

const nodes = new Map()

function node(path) {
  let found = nodes.get(path)
  if (found === undefined) {
    found = cr.Node.read(path, { format: 'text' })
    nodes.set(path, found)
  }
  return found
}

/**
 * What the field `path` (rule syntax) reads in `data`: the value in the JSON
 * format, exactly as the core writes it, or the reason's code when it is null
 * (`missing_field`, `null`, `int_overflow`, `decimal_overflow`, `read_error`).
 */
function read(data, path = 'x') {
  const tree = parse(node(path).evaluateValueTree(data))
  const value = tree.get('value')
  if (value === null && tree.has('reason')) return tree.get('reason').get('code')
  return write(value)
}

/** The `detail` of a null's reason. */
function detail(data, path = 'x') {
  return parse(node(path).evaluateValueTree(data)).get('reason').get('detail')
}

/** A rule set in the rule syntax. */
function rules(text) {
  return cr.RuleSet.read(text, { format: 'text' })
}

export { detail, read, rules }
