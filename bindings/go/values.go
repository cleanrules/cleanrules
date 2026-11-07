package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"strings"
	"sync"
	"time"
)

// Decimal is an exact decimal (spec/data-model.md, Decimals), which Go's
// standard library lacks: up to 38 significant digits, or an infinity. It
// holds its canonical text, such as "0.15" or "inf", which String,
// MarshalText and so encoding/json give. Make one with ParseDecimal; the
// zero Decimal is 0.
//
// Decimals compare with == by their canonical text, the decimal's value and
// scale: Decimal{} == MustDecimal("0") and MustDecimal("007.50") ==
// MustDecimal("7.50"), while "1.0" and "1.00" differ under == although
// cleanrules finds them equal (as the core does, the scale is kept).
type Decimal struct{ text string }

// ParseDecimal reads a decimal written as -?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?,
// inf or -inf, exactly: "1.5e-3" is 0.0015 and "1.00" keeps its two places.
// Other text is a format_error. A decimal that does not fit in 38 digits is
// kept as written: in data it reads as a null with reason decimal_overflow,
// and as a constant it is an expression_error.
func ParseDecimal(s string) (Decimal, error) {
	p, n := text(s)
	var out *C.cr_value
	var e *C.cr_error
	if err := check(C.cr_value_new_decimal(p, n, &out, &e), e); err != nil {
		return Decimal{}, err
	}
	defer C.cr_value_free(out)
	if C.cr_value_kind(out) != C.CR_VALUE_DECIMAL {
		return Decimal{s}, nil
	}
	return decimalOf(decimalText(out)), nil
}

// decimalOf is the Decimal of a canonical text: "0" is stored as the zero
// Decimal, so that == holds between them.
func decimalOf(text string) Decimal {
	if text == "0" {
		return Decimal{}
	}
	return Decimal{text}
}

// MarshalText gives the canonical text, so encoding/json writes a decimal
// as a string, such as "0.15", without losing a digit.
func (d Decimal) MarshalText() ([]byte, error) { return []byte(d.String()), nil }

// UnmarshalText reads a decimal as ParseDecimal does.
func (d *Decimal) UnmarshalText(text []byte) error {
	parsed, err := ParseDecimal(string(text))
	if err != nil {
		return err
	}
	*d = parsed
	return nil
}

// MustDecimal is ParseDecimal for decimals written in the program: it panics
// on text that is not a decimal.
func MustDecimal(s string) Decimal {
	d, err := ParseDecimal(s)
	if err != nil {
		panic(err)
	}
	return d
}

// String gives the decimal's canonical text (spec/json-format.md, Numbers).
func (d Decimal) String() string {
	if d.text == "" {
		return "0"
	}
	return d.text
}

// Date is a day of the proleptic Gregorian calendar, which Go's standard
// library lacks as a type of its own. Dates outside years 1 to 9999 are opaque
// in data.
type Date struct {
	Year  int
	Month time.Month
	Day   int
}

// DateOf is the date of t in its location.
func DateOf(t time.Time) Date {
	y, m, d := t.Date()
	return Date{y, m, d}
}

// String writes the date as YYYY-MM-DD.
func (d Date) String() string {
	return fmt.Sprintf("%04d-%02d-%02d", d.Year, int(d.Month), d.Day)
}

// MarshalText writes the date as String does.
func (d Date) MarshalText() ([]byte, error) { return []byte(d.String()), nil }

// UnmarshalText reads a date written YYYY-MM-DD, a day of the calendar.
func (d *Date) UnmarshalText(text []byte) error {
	at, err := time.Parse(time.DateOnly, string(text))
	if err != nil {
		return fmt.Errorf("cleanrules: %q is not a date (YYYY-MM-DD)", text)
	}
	*d = DateOf(at)
	return nil
}

// DateTime is a date and a time of day, to the microsecond, without a zone (a
// naive datetime), which Go's standard library lacks: time.Time always has a
// location.
type DateTime struct {
	Year                                   int
	Month                                  time.Month
	Day, Hour, Minute, Second, Microsecond int
}

// DateTimeOf is the date and time of day of t in its location, truncated to
// the microsecond.
func DateTimeOf(t time.Time) DateTime {
	y, m, d := t.Date()
	return DateTime{y, m, d, t.Hour(), t.Minute(), t.Second(), t.Nanosecond() / 1000}
}

// String writes the datetime as YYYY-MM-DDTHH:MM:SS, with six digits of
// fraction when it has microseconds.
func (d DateTime) String() string {
	s := fmt.Sprintf("%04d-%02d-%02dT%02d:%02d:%02d", d.Year, int(d.Month), d.Day, d.Hour, d.Minute, d.Second)
	if d.Microsecond != 0 {
		s += fmt.Sprintf(".%06d", d.Microsecond)
	}
	return s
}

// MarshalText writes the datetime as String does.
func (d DateTime) MarshalText() ([]byte, error) { return []byte(d.String()), nil }

// UnmarshalText reads a datetime written YYYY-MM-DDTHH:MM:SS, with up to six
// digits of fraction and no zone.
func (d *DateTime) UnmarshalText(text []byte) error {
	s := string(text)
	at, err := time.Parse("2006-01-02T15:04:05", s)
	if dot := strings.IndexByte(s, '.'); err != nil && dot == 19 && len(s) > 20 && len(s) <= 26 {
		at, err = time.Parse("2006-01-02T15:04:05."+strings.Repeat("0", len(s)-20), s)
	}
	if err != nil {
		return fmt.Errorf("cleanrules: %q is not a datetime without a zone (YYYY-MM-DDTHH:MM:SS[.ffffff])", text)
	}
	*d = DateTimeOf(at)
	return nil
}

// GapDays is a counterfactual's gap between two dates, in days.
type GapDays int64

// GapMicroseconds is a counterfactual's gap between two datetimes, in
// microseconds (a time.Duration cannot hold every one).
type GapMicroseconds int64

// ---- Values given back (spec/data-model.md, Go, Values given back).

// goValue gives a value of the library back as a Go value; opaques holds the
// native objects of the evaluation, by token.
func goValue(v *C.cr_value, opaques []any) any {
	switch C.cr_value_kind(v) {
	case C.CR_VALUE_BOOL:
		var b C.bool
		C.cr_value_get_bool(v, &b, nil)
		return bool(b)
	case C.CR_VALUE_INT:
		var i C.int64_t
		C.cr_value_get_int(v, &i, nil)
		return int64(i)
	case C.CR_VALUE_FLOAT:
		var f C.double
		C.cr_value_get_float(v, &f, nil)
		return float64(f)
	case C.CR_VALUE_DECIMAL:
		return decimalOf(decimalText(v))
	case C.CR_VALUE_STRING:
		var p *C.char
		var n C.size_t
		C.cr_value_get_string(v, &p, &n, nil)
		return goString(p, n)
	case C.CR_VALUE_DATE:
		var y, m, d C.int32_t
		C.cr_value_get_date(v, &y, &m, &d, nil)
		return Date{int(y), time.Month(m), int(d)}
	case C.CR_VALUE_NAIVE_DATETIME:
		var y, mo, d, h, mi, s, us C.int32_t
		C.cr_value_get_naive_datetime(v, &y, &mo, &d, &h, &mi, &s, &us, nil)
		return DateTime{int(y), time.Month(mo), int(d), int(h), int(mi), int(s), int(us)}
	case C.CR_VALUE_AWARE_DATETIME:
		var seconds C.int64_t
		var us, offset C.int32_t
		var zone *C.char
		var n C.size_t
		C.cr_value_get_aware_datetime(v, &seconds, &us, &offset, &zone, &n, nil)
		return time.Unix(int64(seconds), int64(us)*1000).In(location(goString(zone, n), int(offset)))
	case C.CR_VALUE_LIST:
		n := int(C.cr_value_len(v))
		list := make([]any, n)
		for i := range n {
			list[i] = goValue(C.cr_value_list_get(v, C.size_t(i)), opaques)
		}
		return list
	case C.CR_VALUE_MAP:
		n := int(C.cr_value_len(v))
		m := make(map[string]any, n)
		for i := range n {
			var key *C.char
			var length C.size_t
			var item *C.cr_value
			C.cr_value_map_entry(v, C.size_t(i), &key, &length, &item, nil)
			m[goString(key, length)] = goValue(item, opaques)
		}
		return m
	case C.CR_VALUE_OPAQUE:
		var token C.uint64_t
		C.cr_value_get_opaque_token(v, &token, nil)
		if uint64(token) < uint64(len(opaques)) {
			return opaques[token]
		}
		return nil
	default: // a null, whatever its reason
		return nil
	}
}

// takeValue gives an owned value back as a Go value and frees it.
func takeValue(v *C.cr_value, opaques []any) any {
	defer C.cr_value_free(v)
	return goValue(v, opaques)
}

func decimalText(v *C.cr_value) string {
	var p *C.char
	var n C.size_t
	if C.cr_value_get_decimal(v, &p, &n, nil) != C.CR_OK {
		return ""
	}
	return takeString(p, n)
}

// locations caches the time zones Go knows, by name (nil when it does not).
var locations sync.Map

// location is the zone of an aware datetime given back: its IANA zone when
// Go's database knows it, else its fixed offset, and time.UTC for Z.
func location(zone string, offset int) *time.Location {
	if zone != "" {
		cached, ok := locations.Load(zone)
		if !ok {
			loc, err := time.LoadLocation(zone)
			if err != nil {
				loc = nil
			}
			cached, _ = locations.LoadOrStore(zone, loc)
		}
		if loc, _ := cached.(*time.Location); loc != nil {
			return loc
		}
	}
	if offset == 0 {
		return time.UTC
	}
	return time.FixedZone("", offset)
}
