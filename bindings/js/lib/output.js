// Values given back to JavaScript (spec/data-model.md, *JavaScript and
// TypeScript*, *Values given back*): outputs, payloads, metadata and the
// values in an explanation. The native layer writes them in a private
// encoding (`src/common/out.rs` lists the forms), the same in both builds;
// this module turns each form into its JavaScript value. It converts only:
// every value was decided by the core.

import { Decimal, LocalDate, NaiveDateTime, zoned } from './values.js'

const MAX_SAFE = 2n ** 53n - 1n

/** An int: a `number` within ±(2⁵³ − 1), a `bigint` beyond. */
function int(digits) {
  const value = BigInt(digits)
  return value >= -MAX_SAFE && value <= MAX_SAFE ? Number(value) : value
}

/**
 * A plain object with these entries as its own enumerable properties, in
 * order; a key such as `__proto__` is an own property, never the prototype.
 */
function plainObject(entries) {
  const object = {}
  for (const [key, value] of entries) {
    Object.defineProperty(object, key, { value, enumerable: true, writable: true, configurable: true })
  }
  return object
}

/**
 * The JavaScript value of `form`; an opaque value is its object in
 * `objects` (by token), or `null` when there is none (a recorded decision
 * keeps only the type's name).
 */
function decode(form, objects) {
  if (form === null || typeof form !== 'object') {
    return form
  }
  switch (form[0]) {
    case 'i':
      return int(form[1])
    case 'f':
      return Number(form[1])
    case 'd':
      return new Decimal(form[1])
    case 'D':
      return new LocalDate(form[1], form[2], form[3])
    case 'N':
      return new NaiveDateTime(form[1], form[2], form[3], form[4], form[5], form[6], form[7])
    case 'A':
      return zoned(form.slice(1))
    case 'l':
      return form[1].map((item) => decode(item, objects))
    case 'O':
      return plainObject(form[1].map(([key, item]) => [key, decode(item, objects)]))
    case 'X': {
      const token = Number(form[2])
      return Number.isSafeInteger(token) && token < objects.length ? objects[token] : null
    }
    default:
      throw new Error(`cleanrules: unknown output form ${JSON.stringify(form[0])}; please report it`)
  }
}

/**
 * A gap of a counterfactual: a number (or `Decimal`), `{ days }` between
 * dates or `{ microseconds }` between datetimes.
 */
function decodeGap(form) {
  if (Array.isArray(form) && form[0] === 'days') {
    return Object.freeze({ days: int(form[1]) })
  }
  if (Array.isArray(form) && form[0] === 'us') {
    return Object.freeze({ microseconds: int(form[1]) })
  }
  return decode(form, [])
}

/** An int of the JSON format: a number, or `{"$int": "..."}` beyond 2⁵³. */
function jsonInt(value) {
  return typeof value === 'object' && value !== null ? int(value.$int) : value
}

export { decode, decodeGap, int, jsonInt, plainObject }
