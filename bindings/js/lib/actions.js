// Actions by name (spec/rules.md, *Actions*): the native layer runs the
// selected rules' actions in rule order through these two hooks, so that a
// function that throws never leaves an exception pending in native code.

import { describe } from './read.js'

/** The registry `actions` (an object or a `Map` of functions) as `[names, functions]`. */
function actions(registry) {
  let entries
  if (registry instanceof Map) {
    entries = [...registry.entries()]
  } else if (registry !== null && typeof registry === 'object') {
    entries = Object.keys(registry).map((name) => [name, registry[name]])
  } else {
    throw new TypeError('actions must be an object or a Map from action names to functions')
  }
  const names = []
  const functions = []
  for (const [name, action] of entries) {
    if (typeof name !== 'string') {
      throw new TypeError(`an action name must be a string, not ${typeof name}`)
    }
    if (typeof action !== 'function') {
      throw new TypeError(`the action ${JSON.stringify(name)} is not a function`)
    }
    names.push(name)
    functions.push(action)
  }
  return [names, functions]
}

/** Whether `value` is a thenable; reading `then` may throw, which is thrown. */
function isThenable(value) {
  return (typeof value === 'object' || typeof value === 'function') && value !== null && typeof value.then === 'function'
}

/**
 * Call `action` with the `ActionCall` the native layer made: `undefined`, or
 * `[kind, thrown, description]` when it fails, with kind `"threw"` or
 * `"promise"`. Actions are synchronous: one that returns a promise (any
 * thenable) fails, and the promise gets a no-op rejection handler, so that a
 * later rejection is not reported as unhandled.
 */
function call(action, argument) {
  try {
    const result = action(argument)
    if (isThenable(result)) {
      try {
        result.then(undefined, () => {})
      } catch {
        // A thenable whose `then` throws: it is still not an action's result.
      }
      return ['promise', undefined, 'a Promise']
    }
    return undefined
  } catch (thrown) {
    return ['threw', thrown, describe(thrown)]
  }
}

export { actions, actions as registry, call }
