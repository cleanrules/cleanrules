package cleanrules

// The conformance runner's JSON: a reader that loses nothing the vectors
// compare, a writer, and the comparison of conformance/README.md
// (Comparing). The runner never compares with encoding/json's maps, which
// lose member order, nor with the library's own equality.

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"math/big"
	"slices"
	"strconv"
	"strings"
	"time"
)

// jsonObject is a JSON object that keeps its members' order. unordered
// marks a map given back to Go (spec/data-model.md, Go, Values given back:
// Go's maps have no order), whose keys are sorted and whose order is not
// compared.
type jsonObject struct {
	keys      []string
	values    map[string]any
	unordered bool
}

func newObject() *jsonObject { return &jsonObject{values: map[string]any{}} }

// set adds a member, or replaces its value.
func (o *jsonObject) set(key string, value any) *jsonObject {
	if _, ok := o.values[key]; !ok {
		o.keys = append(o.keys, key)
	}
	o.values[key] = value
	return o
}

func (o *jsonObject) get(key string) any {
	if o == nil {
		return nil
	}
	return o.values[key]
}

func (o *jsonObject) has(key string) bool {
	if o == nil {
		return false
	}
	_, ok := o.values[key]
	return ok
}

// str is a member that is a string, or "".
func (o *jsonObject) str(key string) string {
	s, _ := o.get(key).(string)
	return s
}

// obj is a member that is an object, or nil.
func (o *jsonObject) obj(key string) *jsonObject {
	v, _ := o.get(key).(*jsonObject)
	return v
}

// list is a member that is an array, or nil.
func (o *jsonObject) list(key string) []any {
	v, _ := o.get(key).([]any)
	return v
}

// parseJSONText reads a JSON text: objects in order (a repeated key is an
// error), numbers as json.Number, which keeps their text, and nothing after
// the value.
func parseJSONText(text string) (any, error) {
	decoder := json.NewDecoder(strings.NewReader(text))
	decoder.UseNumber()
	value, err := parseJSONValue(decoder)
	if err != nil {
		return nil, err
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return nil, fmt.Errorf("text after the JSON value at offset %d", decoder.InputOffset())
	}
	return value, nil
}

func parseJSONValue(d *json.Decoder) (any, error) {
	token, err := d.Token()
	if err != nil {
		return nil, err
	}
	switch token {
	case json.Delim('['):
		list := []any{}
		for d.More() {
			item, err := parseJSONValue(d)
			if err != nil {
				return nil, err
			}
			list = append(list, item)
		}
		_, err := d.Token()
		return list, err
	case json.Delim('{'):
		o := newObject()
		for d.More() {
			key, err := d.Token()
			if err != nil {
				return nil, err
			}
			name := key.(string)
			if o.has(name) {
				return nil, fmt.Errorf("the key %q is repeated", name)
			}
			value, err := parseJSONValue(d)
			if err != nil {
				return nil, err
			}
			o.set(name, value)
		}
		_, err := d.Token()
		return o, err
	}
	return token, nil // nil, bool, string or json.Number
}

// writeJSON writes a parsed tree as compact JSON, numbers as written.
func writeJSON(v any) string {
	var out bytes.Buffer
	writeJSONTo(&out, v)
	return out.String()
}

func writeJSONTo(out *bytes.Buffer, v any) {
	switch v := v.(type) {
	case nil:
		out.WriteString("null")
	case bool:
		out.WriteString(strconv.FormatBool(v))
	case json.Number:
		out.WriteString(string(v))
	case string:
		writeJSONString(out, v)
	case []any:
		out.WriteByte('[')
		for i, item := range v {
			if i > 0 {
				out.WriteByte(',')
			}
			writeJSONTo(out, item)
		}
		out.WriteByte(']')
	case *jsonObject:
		out.WriteByte('{')
		for i, key := range v.keys {
			if i > 0 {
				out.WriteByte(',')
			}
			writeJSONString(out, key)
			out.WriteByte(':')
			writeJSONTo(out, v.values[key])
		}
		out.WriteByte('}')
	default:
		panic(fmt.Sprintf("not a JSON tree: %T %v", v, v))
	}
}

// writeJSONString escapes what JSON requires, and nothing more.
func writeJSONString(out *bytes.Buffer, s string) {
	out.WriteByte('"')
	for _, r := range s {
		switch {
		case r == '"':
			out.WriteString(`\"`)
		case r == '\\':
			out.WriteString(`\\`)
		case r == '\n':
			out.WriteString(`\n`)
		case r == '\r':
			out.WriteString(`\r`)
		case r == '\t':
			out.WriteString(`\t`)
		case r < 0x20:
			fmt.Fprintf(out, `\u%04x`, r)
		default:
			out.WriteRune(r)
		}
	}
	out.WriteByte('"')
}

// compareMode says what the comparison allows besides the format's exact
// trees.
type compareMode int

const (
	// modeExact compares trees as the core writes them.
	modeExact compareMode = iota
	// modeGiven compares Go values given back (spec/data-model.md, Go,
	// Values given back): maps without their order, an aware datetime in
	// the zone UTC as Z (Go's time.UTC), and one in a zone Go's database
	// does not know at its offset.
	modeGiven
	// modeRecorded is modeGiven for a decision read from a document, whose
	// opaque values come back as nil.
	modeRecorded
)

// isFloatText reports whether a number literal is a float: a fraction or an
// exponent (spec/json-format.md, Numbers).
func isFloatText(n json.Number) bool { return strings.ContainsAny(string(n), ".eE") }

// sameJSON reports whether actual matches expected (conformance/README.md,
// Comparing): object members in order, ints exact at any size, floats by
// exact double and sign of zero, an int never equal to a float, any two
// opaque values equal, and detail in reasons ignored; with what mode
// allows.
func sameJSON(actual, expected any, mode compareMode) bool {
	return sameIn(actual, expected, mode, false)
}

func sameIn(actual, expected any, mode compareMode, inReason bool) bool {
	switch want := expected.(type) {
	case json.Number:
		got, ok := actual.(json.Number)
		return ok && sameNumber(got, want)
	case []any:
		got, ok := actual.([]any)
		if !ok || len(got) != len(want) {
			return false
		}
		for i := range want {
			if !sameIn(got[i], want[i], mode, false) {
				return false
			}
		}
		return true
	case *jsonObject:
		if len(want.keys) == 1 && want.keys[0] == "$opaque" {
			if mode == modeRecorded && actual == nil {
				return true
			}
			got, ok := actual.(*jsonObject)
			return ok && len(got.keys) == 1 && got.keys[0] == "$opaque"
		}
		if mode != modeExact && len(want.keys) == 1 {
			if allowed, ok := givenForm(want); ok {
				return sameIn(actual, allowed, modeExact, false)
			}
		}
		got, ok := actual.(*jsonObject)
		if !ok {
			return false
		}
		a, e := members(got, inReason), members(want, inReason)
		if len(a) != len(e) {
			return false
		}
		if got.unordered {
			if mode == modeExact {
				return false
			}
			for _, key := range e {
				if !got.has(key) || !sameIn(got.values[key], want.values[key], mode, key == "reason") {
					return false
				}
			}
			return true
		}
		for i, key := range e {
			if a[i] != key || !sameIn(got.values[key], want.values[key], mode, key == "reason") {
				return false
			}
		}
		return true
	case nil:
		return actual == nil
	}
	return actual == expected // a bool or a string
}

// members are an object's keys, without detail in a reason.
func members(o *jsonObject, inReason bool) []string {
	if !inReason {
		return o.keys
	}
	return slices.DeleteFunc(slices.Clone(o.keys), func(key string) bool { return key == "detail" })
}

func sameNumber(actual, expected json.Number) bool {
	if isFloatText(actual) != isFloatText(expected) {
		return false
	}
	if !isFloatText(expected) {
		a, okA := new(big.Int).SetString(string(actual), 10)
		e, okE := new(big.Int).SetString(string(expected), 10)
		return okA && okE && a.Cmp(e) == 0
	}
	a, errA := strconv.ParseFloat(string(actual), 64)
	e, errE := strconv.ParseFloat(string(expected), 64)
	return errA == nil && errE == nil && math.Float64bits(a) == math.Float64bits(e)
}

// givenForm is what Go gives back for a typed value it cannot hold as
// written (spec/data-model.md, Go, Values given back): an aware datetime in
// a zone that Go's zone database does not know, at its offset, and one in
// the zone UTC, which time.LoadLocation gives as time.UTC, as Z.
func givenForm(typed *jsonObject) (any, bool) {
	text, ok := typed.get("$datetime").(string)
	if !ok || !strings.HasSuffix(text, "]") {
		return nil, false
	}
	open := strings.IndexByte(text, '[')
	loc, err := time.LoadLocation(text[open+1 : len(text)-1])
	if err == nil && loc != time.UTC {
		return nil, false
	}
	fixed := text[:open]
	if strings.HasSuffix(fixed, "+00:00") {
		fixed = strings.TrimSuffix(fixed, "+00:00") + "Z"
	}
	return newObject().set("$datetime", fixed), true
}
