// Marker values for the kinds JavaScript has no type for (spec/data-model.md,
// *Conversions by language*, *JavaScript and TypeScript*): a `number` that is
// an integer reads as an int, so a float needs `float(x)`; decimals, dates and
// naive datetimes need `decimal`, `date` and `datetime`. The reader
// (`lib/read.js`) recognises them by the brand `Symbol.for('cleanrules.marker')`
// on their prototype, so markers made by another copy of the package work too.
// Values given back (*Values given back*) use the same classes, and
// `ZonedDateTime` for aware datetimes, which the engine alone makes; each
// reads back as the value it came from.

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

/** Only the engine makes `ZonedDateTime` values (`zoned`). */
const ENGINE = Symbol('cleanrules.engine')

/** `floor(a / b)` for bigints (division truncates toward zero). */
function floorDiv(a, b) {
  const q = a / b
  return a % b !== 0n && a < 0n !== b < 0n ? q - 1n : q
}

/** An offset in seconds as `±HH:MM`, with `:SS` when the seconds are not zero. */
function offsetText(seconds) {
  const pad = (n) => String(n).padStart(2, '0')
  const sign = seconds < 0 ? '-' : '+'
  const total = Math.abs(seconds)
  const [h, m, s] = [Math.floor(total / 3600), Math.floor((total % 3600) / 60), total % 60]
  return `${sign}${pad(h)}:${pad(m)}${s === 0 ? '' : `:${pad(s)}`}`
}

/**
 * An aware datetime as the engine gives it back: an instant, to the
 * microsecond, with its local date and time in its IANA time zone or at a
 * fixed offset. The same in every runtime (Node 22 has no Temporal); it
 * converts to a `Date` and, where the runtime has Temporal, to a
 * `Temporal.ZonedDateTime`, and reads back as the same value.
 */
class ZonedDateTime {
  #text

  constructor(engine, fields) {
    if (engine !== ENGINE) {
      throw new TypeError(
        'ZonedDateTime values come from the engine; as input, pass a Date or a Temporal.ZonedDateTime',
      )
    }
    const [text, year, month, day, hour, minute, second, microsecond, offsetSeconds, timeZone, epoch] = fields
    this.#text = text
    /** The local date and time: year, month (from 1), day, hour, minute, second and microsecond. */
    this.year = year
    this.month = month
    this.day = day
    this.hour = hour
    this.minute = minute
    this.second = second
    this.microsecond = microsecond
    /** The offset from UTC at this instant, in seconds. */
    this.offsetSeconds = offsetSeconds
    /** The IANA time zone, such as `"Europe/Madrid"`, or `null` for a fixed offset. */
    this.timeZone = timeZone
    /** The instant, in microseconds since 1970-01-01T00:00:00Z. */
    this.epochMicroseconds = BigInt(epoch)
    Object.freeze(this)
  }

  /** The instant as a `Date` (to the millisecond, rounded toward the past). */
  toDate() {
    return new Date(Number(floorDiv(this.epochMicroseconds, 1000n)))
  }

  /**
   * The same instant as a `Temporal.ZonedDateTime`, in its IANA zone or at
   * its offset. In a named zone its local fields follow the runtime's zone
   * database (this object's follow the engine's bundled one). A `TypeError`
   * where the runtime has no `Temporal`; a `RangeError` for an offset with
   * seconds, which Temporal's offset zones (whole minutes) cannot hold.
   */
  toTemporal() {
    const Temporal = globalThis.Temporal
    if (typeof Temporal !== 'object' || Temporal === null) {
      throw new TypeError(
        'this runtime has no Temporal: use toDate(), or install a Temporal polyfill as globalThis.Temporal',
      )
    }
    if (this.timeZone === null && this.offsetSeconds % 60 !== 0) {
      throw new RangeError(
        `the offset ${offsetText(this.offsetSeconds)} has seconds, which a Temporal.ZonedDateTime cannot hold; ` +
          'use toDate(), or epochMicroseconds with offsetSeconds',
      )
    }
    const instant = Temporal.Instant.fromEpochNanoseconds(this.epochMicroseconds * 1000n)
    return instant.toZonedDateTimeISO(this.timeZone ?? offsetText(this.offsetSeconds))
  }

  /**
   * The value as the JSON format writes it (RFC 9557): `2026-10-31T09:30:00+01:00[Europe/Madrid]`,
   * or with a fixed offset, `Z` for UTC.
   */
  toString() {
    return this.#text
  }

  /** `toString()`, for `JSON.stringify` (its `epochMicroseconds` is a bigint). */
  toJSON() {
    return this.#text
  }
}

/** A `ZonedDateTime` of the engine, from the fields of `lib/output.js`. */
function zoned(fields) {
  return new ZonedDateTime(ENGINE, fields)
}

brand(Decimal, 'decimal')
brand(Float, 'float')
brand(LocalDate, 'date')
brand(NaiveDateTime, 'naive_datetime')
brand(ZonedDateTime, 'aware_datetime')

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

export {
  Decimal,
  Float,
  LocalDate,
  MARKER,
  NaiveDateTime,
  ZonedDateTime,
  date,
  datetime,
  decimal,
  float,
  setDecimalCheck,
  zoned,
}
