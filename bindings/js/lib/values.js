'use strict'

// Marker values for the kinds JavaScript has no type for (spec/data-model.md,
// *Conversions by language*, *JavaScript and TypeScript*): a `number` that is
// an integer reads as an int, so a float needs `float(x)`; decimals, dates and
// naive datetimes need `decimal`, `date` and `datetime`. The reader
// (`lib/read.js`) recognises them by the brand `Symbol.for('cleanrules.marker')`
// on their prototype, so markers made by another copy of the package work too.
// Their final API shape is decided with the TypeScript API (phase F.2).

/** The brand of marker values: its value names the kind. */
const MARKER = Symbol.for('cleanrules.marker')

function brand(Class, kind) {
  Object.defineProperty(Class.prototype, MARKER, { value: kind })
}

let decimalProblem = () => null

/** Install the core's check of decimal literals (`index.js` does it). */
function setDecimalCheck(check) {
  decimalProblem = check
}

function wholeIn(name, value, low, high) {
  if (!Number.isInteger(value) || value < low || value > high) {
    throw new RangeError(`${name} is ${String(value)}; it must be a whole number from ${low} to ${high}`)
  }
}

function daysIn(year, month) {
  if (month === 2) {
    const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0
    return leap ? 29 : 28
  }
  return [4, 6, 9, 11].includes(month) ? 30 : 31
}

function checkDate(year, month, day) {
  wholeIn('year', year, 1, 9999)
  wholeIn('month', month, 1, 12)
  wholeIn('day', day, 1, daysIn(year, month))
}

/** An exact decimal, given by its literal text, such as `"0.10"`. */
class Decimal {
  constructor(text) {
    if (typeof text !== 'string') {
      throw new TypeError(`a decimal is given as text, such as "0.10", not as a ${typeof text}`)
    }
    const problem = decimalProblem(text)
    if (problem !== null) {
      throw new TypeError(`${JSON.stringify(text)} is ${problem}`)
    }
    /** The literal, as given. */
    this.text = text
    Object.freeze(this)
  }

  toString() {
    return this.text
  }
}

/** A float, for a `number` that would otherwise read as an int. */
class Float {
  constructor(value) {
    if (typeof value !== 'number') {
      throw new TypeError(`a float is a number, not a ${typeof value}`)
    }
    /** The number. */
    this.value = value
    Object.freeze(this)
  }

  valueOf() {
    return this.value
  }

  toString() {
    return String(this.value)
  }
}

/** A day of the proleptic Gregorian calendar, years 1 to 9999. */
class LocalDate {
  constructor(year, month, day) {
    checkDate(year, month, day)
    this.year = year
    this.month = month
    this.day = day
    Object.freeze(this)
  }

  toString() {
    const pad = (n, width) => String(n).padStart(width, '0')
    return `${pad(this.year, 4)}-${pad(this.month, 2)}-${pad(this.day, 2)}`
  }
}

/** A date and a time of day, to the microsecond, without a time zone. */
class NaiveDateTime {
  constructor(year, month, day, hour = 0, minute = 0, second = 0, microsecond = 0) {
    checkDate(year, month, day)
    wholeIn('hour', hour, 0, 23)
    wholeIn('minute', minute, 0, 59)
    wholeIn('second', second, 0, 59)
    wholeIn('microsecond', microsecond, 0, 999999)
    this.year = year
    this.month = month
    this.day = day
    this.hour = hour
    this.minute = minute
    this.second = second
    this.microsecond = microsecond
    Object.freeze(this)
  }

  toString() {
    const pad = (n, width) => String(n).padStart(width, '0')
    const date = `${pad(this.year, 4)}-${pad(this.month, 2)}-${pad(this.day, 2)}`
    const time = `${pad(this.hour, 2)}:${pad(this.minute, 2)}:${pad(this.second, 2)}`
    return this.microsecond === 0 ? `${date}T${time}` : `${date}T${time}.${pad(this.microsecond, 6)}`
  }
}

brand(Decimal, 'decimal')
brand(Float, 'float')
brand(LocalDate, 'date')
brand(NaiveDateTime, 'naive_datetime')

/** `decimal("0.10")`: an exact decimal (JavaScript has none). */
function decimal(text) {
  return new Decimal(text)
}

/** `float(2)`: the number as a float, not an int. */
function float(value) {
  return new Float(value)
}

/** `date(2026, 11, 4)`: a date (months count from 1). */
function date(year, month, day) {
  return new LocalDate(year, month, day)
}

/** `datetime(2026, 11, 4, 9, 30)`: a naive datetime (no time zone). */
function datetime(year, month, day, hour, minute, second, microsecond) {
  return new NaiveDateTime(year, month, day, hour, minute, second, microsecond)
}

module.exports = {
  Decimal,
  Float,
  LocalDate,
  MARKER,
  NaiveDateTime,
  date,
  datetime,
  decimal,
  float,
  setDecimalCheck,
}
