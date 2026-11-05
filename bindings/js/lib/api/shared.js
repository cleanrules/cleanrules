// What the idiomatic API's modules share: describing values in messages,
// checking options objects, and telling apart the objects of the two builds.

import { typeName } from '../read.js'

/** The brand of the API's prototypes: its value names the build. */
const API = Symbol.for('cleanrules.api')

/** What a value is, for messages: `the number 5`, `a Set`, `undefined`... */
function show(value) {
  if (value === null || value === undefined) return String(value)
  switch (typeof value) {
    case 'string':
      return `the string ${JSON.stringify(value)}`
    case 'number':
    case 'boolean':
      return `the ${typeof value} ${String(value)}`
    case 'bigint':
      return `the bigint ${String(value)}n`
    case 'function':
      return 'a function'
    case 'symbol':
      return 'a symbol'
    default:
      return Array.isArray(value) ? 'an array' : `a ${typeName(value)}`
  }
}

/**
 * An options object, checked: `{}` when left out, a `TypeError` for anything
 * but an object and for a name not in `names` (`example` shows one).
 */
function options(value, names, example) {
  if (value === undefined || value === null) return {}
  if (typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError(`options must be an object, such as ${example}, not ${show(value)}`)
  }
  const unknown = Object.keys(value).filter((name) => !names.includes(name))
  if (unknown.length > 0) {
    throw new TypeError(`unknown option ${JSON.stringify(unknown[0])}; the options are ${names.join(', ')}`)
  }
  return value
}

/** Brand the prototypes of `classes` with the name of their build. */
function brand(build, ...classes) {
  for (const Class of classes) {
    Object.defineProperty(Class.prototype, API, { value: build })
  }
}

/**
 * Throw if `value` is an object of the idiomatic API of another build than
 * `build` (the Node addon and the WebAssembly module each make their own
 * classes, over their own native objects).
 */
function sameBuild(value, build) {
  if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return
  let other
  try {
    other = value[API]
  } catch {
    return
  }
  if (typeof other === 'string' && other !== build) {
    throw new TypeError(
      `this ${typeName(value)} comes from the ${other} build of cleanrules, not the ${build} one; ` +
        "import everything from one entry point ('cleanrules'), so that one build makes every object",
    )
  }
}

export { API, brand, options, sameBuild, show }
