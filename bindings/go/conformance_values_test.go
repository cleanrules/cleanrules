package cleanrules

// The conformance runner's Go data: values of the format turned into Go
// data as a program would hold them, rotating through the forms that
// spec/data-model.md (Go) reads, and Go values given back turned into
// values of the format, checking each Go type that the spec gives back.

import (
	"encoding/json"
	"fmt"
	"math"
	"math/big"
	"reflect"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"time"
)

// Named types, as programs declare them: read by their reflect.Kind.
type (
	goInt64  int64
	goFloat  float64
	goText   string
	goBool   bool
	goList   []any
	goObject map[string]any
)

// vectorFailure is a vector the runner cannot run as written, or an answer
// that breaks a promise of the API: the vector fails with its message.
type vectorFailure struct{ message string }

func failf(format string, args ...any) { panic(vectorFailure{fmt.Sprintf(format, args...)}) }

// vectorSkip is a vector one route cannot run, for a reason of
// conformanceSkips.
type vectorSkip struct{ reason string }

// used counts a Go form for the vector running.
func (c *conformance) used(form string) {
	ids := c.forms[form]
	if ids == nil {
		ids = map[string]bool{}
		c.forms[form] = ids
	}
	ids[c.current] = true
}

// pick takes the next of n forms, in turn.
func (c *conformance) pick(n int) int {
	c.turn++
	return c.turn % n
}

var anyType = reflect.TypeFor[any]()

// toGo turns a data value of the format into Go data.
func (c *conformance) toGo(v any) any {
	switch v := v.(type) {
	case nil:
		return c.null()
	case bool:
		return c.boolean(v)
	case string:
		return c.text(v)
	case json.Number:
		if isFloatText(v) {
			f, err := strconv.ParseFloat(string(v), 64)
			if err != nil {
				failf("the float %s: %v", v, err)
			}
			return c.float(f)
		}
		n, ok := new(big.Int).SetString(string(v), 10)
		if !ok {
			failf("the int %s", v)
		}
		return c.integer(n)
	case []any:
		items := make([]any, len(v))
		for i, item := range v {
			items[i] = c.toGo(item)
		}
		return c.list(items)
	case *jsonObject:
		if len(v.keys) == 1 && strings.HasPrefix(v.keys[0], "$") {
			return c.typed(v.keys[0], v.values[v.keys[0]])
		}
		return c.mapOf(v)
	}
	failf("not a value of the format: %T", v)
	return nil
}

func (c *conformance) typed(key string, v any) any {
	switch key {
	case "$int":
		text, ok := v.(string)
		if !ok {
			text = string(v.(json.Number))
		}
		n, ok := new(big.Int).SetString(text, 10) // leading zeros, as the format reads them
		if !ok {
			failf("the int %v", v)
		}
		return c.integer(n)
	case "$float":
		switch v {
		case "inf":
			return c.float(math.Inf(1))
		case "-inf":
			return c.float(math.Inf(-1))
		}
		f, err := strconv.ParseFloat(string(v.(json.Number)), 64)
		if err != nil {
			failf("the float %v: %v", v, err)
		}
		return c.float(f)
	case "$decimal":
		d, err := ParseDecimal(v.(string))
		if err != nil {
			failf("the decimal %v: %v", v, err)
		}
		return c.form(d, "Decimal", "*Decimal")
	case "$date":
		at, err := time.Parse(time.DateOnly, v.(string))
		if err != nil {
			failf("the date %v: %v", v, err)
		}
		return c.form(Date{at.Year(), at.Month(), at.Day()}, "Date", "*Date")
	case "$datetime":
		return c.datetime(v.(string))
	case "$map":
		return c.mapOf(v.(*jsonObject))
	}
	failf("the typed value %s, which Go data cannot hold", key)
	return nil
}

// form gives v, or a pointer to it, in turn; names are the forms' names.
func (c *conformance) form(v any, names ...string) any {
	i := c.pick(len(names))
	c.used(names[i])
	if i == 0 {
		return v
	}
	p := reflect.New(reflect.TypeOf(v))
	p.Elem().Set(reflect.ValueOf(v))
	return p.Interface()
}

func (c *conformance) null() any {
	forms := []any{nil, (*int64)(nil), []any(nil), map[string]any(nil), (*time.Time)(nil)}
	names := []string{"nil", "nil *int64", "nil []any", "nil map[string]any", "nil *time.Time"}
	i := c.pick(len(forms))
	c.used(names[i])
	return forms[i]
}

func (c *conformance) boolean(b bool) any {
	switch c.pick(3) {
	case 0:
		c.used("bool")
		return b
	case 1:
		c.used("named bool")
		return goBool(b)
	}
	c.used("*bool")
	return &b
}

func (c *conformance) text(s string) any {
	switch c.pick(3) {
	case 0:
		c.used("string")
		return s
	case 1:
		c.used("named string")
		return goText(s)
	}
	c.used("*string")
	return &s
}

// integer is an int in one of the Go types that hold it: the signed and
// unsigned integers, a named one, a pointer, big.Int and *big.Int.
func (c *conformance) integer(n *big.Int) any {
	var forms []any
	if n.IsInt64() {
		i := n.Int64()
		forms = append(forms, i, int(i), goInt64(i), &i)
		if i >= math.MinInt32 && i <= math.MaxInt32 {
			forms = append(forms, int32(i))
		}
		if i >= math.MinInt16 && i <= math.MaxInt16 {
			forms = append(forms, int16(i))
		}
		if i >= math.MinInt8 && i <= math.MaxInt8 {
			forms = append(forms, int8(i))
		}
	}
	if n.IsUint64() {
		u := n.Uint64()
		forms = append(forms, u, uint(u))
		if u <= math.MaxUint32 {
			forms = append(forms, uint32(u))
		}
		if u <= math.MaxUint16 {
			forms = append(forms, uint16(u))
		}
		if u <= math.MaxUint8 {
			forms = append(forms, uint8(u))
		}
	}
	forms = append(forms, new(big.Int).Set(n), *new(big.Int).Set(n))
	v := forms[c.pick(len(forms))]
	c.used(reflect.TypeOf(v).String())
	return v
}

// float is a float64, a float32 when it holds the same number, a named
// float or a pointer.
func (c *conformance) float(f float64) any {
	forms := []any{f, goFloat(f), &f}
	if float64(float32(f)) == f || math.IsInf(f, 0) {
		forms = append(forms, float32(f))
	}
	v := forms[c.pick(len(forms))]
	c.used(reflect.TypeOf(v).String())
	return v
}

var datetimeText = regexp.MustCompile(`^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(?:\.(\d{1,6}))?(Z|[+-]\d\d:\d\d(?::\d\d)?)?(?:\[([^\]]+)\])?$`)

// datetime is a naive datetime as a DateTime; an aware one as a time.Time:
// Z in time.UTC, an offset in a fixed zone (time.FixedZone, unnamed or
// named, or an Etc/GMT zone by name for whole hours), a zone by its name.
func (c *conformance) datetime(s string) any {
	m := datetimeText.FindStringSubmatch(s)
	if m == nil {
		failf("the datetime %q", s)
	}
	n := func(i int) int { v, _ := strconv.Atoi(m[i]); return v }
	us := 0
	if m[7] != "" {
		us, _ = strconv.Atoi((m[7] + "00000")[:6])
	}
	if m[8] == "" {
		return c.form(DateTime{n(1), time.Month(n(2)), n(3), n(4), n(5), n(6), us}, "DateTime", "*DateTime")
	}
	offset := 0
	if m[8] != "Z" {
		parts := strings.Split(m[8][1:], ":")
		for i, part := range parts {
			v, _ := strconv.Atoi(part)
			offset += v * []int{3600, 60, 1}[i]
		}
		if m[8][0] == '-' {
			offset = -offset
		}
	}
	// The instant, from the local fields and the offset written.
	at := time.Date(n(1), time.Month(n(2)), n(3), n(4), n(5), n(6), us*1000, time.UTC).Add(-time.Duration(offset) * time.Second)
	var loc *time.Location
	switch {
	case m[9] != "":
		zone, err := time.LoadLocation(m[9])
		if err != nil {
			panic(vectorSkip{"zone_unknown_to_go"})
		}
		loc = zone
		c.used("time.Time in a zone")
	case m[8] == "Z":
		loc = time.UTC
		c.used("time.Time in time.UTC")
	default:
		forms := []string{"time.Time in time.FixedZone", "time.Time in a named time.FixedZone"}
		if offset%3600 == 0 && offset != 0 && offset >= -12*3600 && offset <= 14*3600 {
			forms = append(forms, "time.Time in an Etc/GMT zone")
		}
		form := forms[c.pick(len(forms))]
		c.used(form)
		switch form {
		case "time.Time in time.FixedZone":
			loc = time.FixedZone("", offset)
		case "time.Time in a named time.FixedZone":
			loc = time.FixedZone("Fixed", offset)
		default:
			// Etc/GMT+5 is five hours behind: the sign is reversed.
			zone, err := time.LoadLocation(fmt.Sprintf("Etc/GMT%+d", -offset/3600))
			if err != nil {
				failf("the zone of the offset %d: %v", offset, err)
			}
			loc = zone
		}
	}
	t := at.In(loc)
	if _, got := t.Zone(); got != offset {
		failf("%s: Go's zone gives the offset %d there, not %d", s, got, offset)
	}
	if c.pick(4) == 0 {
		c.used("*time.Time")
		return &t
	}
	return t
}

// list is a list as a []any, an array, a slice of its items' type, a named
// slice or a pointer to a []any.
func (c *conformance) list(items []any) any {
	forms := []string{"[]any", "array", "named []any", "*[]any"}
	var elem reflect.Type
	for i, item := range items {
		t := reflect.TypeOf(item)
		if t == nil || (i > 0 && t != elem) {
			elem = nil
			break
		}
		elem = t
	}
	// A slice or array of uint8 is bytes, which Go data reads as opaque.
	if elem != nil && elem.Kind() != reflect.Uint8 {
		forms = append(forms, "typed slice")
	}
	form := forms[c.pick(len(forms))]
	switch form {
	case "[]any":
		c.used(form)
		return items
	case "named []any":
		c.used(form)
		return goList(items)
	case "*[]any":
		c.used(form)
		return &items
	case "array":
		c.used("array")
		array := reflect.New(reflect.ArrayOf(len(items), anyType)).Elem()
		for i, item := range items {
			if item != nil {
				array.Index(i).Set(reflect.ValueOf(item))
			}
		}
		return array.Interface()
	}
	c.used("[]" + typeName(elem))
	slice := reflect.MakeSlice(reflect.SliceOf(elem), len(items), len(items))
	for i, item := range items {
		slice.Index(i).Set(reflect.ValueOf(item))
	}
	return slice.Interface()
}

// structKey reports whether a key can name a struct field: by its
// cleanrules tag, which ends at a comma and is "-" to leave a field out.
func structKey(key string) bool {
	return key != "" && key != "-" && !strings.Contains(key, ",")
}

// mapOf is a map as a map[string]any, a map[any]any, a map of its values'
// type, a named map, or a struct or a pointer to one, whose fields keep the
// keys' order. Go reads a map's entries sorted by key (spec/data-model.md,
// Go), so keys out of that order are given as a struct.
func (c *conformance) mapOf(o *jsonObject) any {
	values := make([]any, len(o.keys))
	for i, key := range o.keys {
		values[i] = c.toGo(o.values[key])
	}
	sorted := slices.IsSorted(o.keys)
	fields := !slices.ContainsFunc(o.keys, func(key string) bool { return !structKey(key) })
	var forms []string
	switch {
	case !sorted && fields:
		forms = []string{"struct", "*struct"}
	case !sorted:
		failf("the map %v has keys out of order that no struct field can name", o.keys)
	default:
		forms = []string{"map[string]any", "map[any]any", "named map", "struct", "*struct"}
		var elem reflect.Type
		for i, v := range values {
			t := reflect.TypeOf(v)
			if t == nil || (i > 0 && t != elem) {
				elem = nil
				break
			}
			elem = t
		}
		if elem != nil {
			forms = append(forms, "typed map")
		}
		if !fields {
			forms = slices.DeleteFunc(forms, func(f string) bool { return strings.HasSuffix(f, "struct") })
		}
	}
	form := forms[c.pick(len(forms))]
	switch form {
	case "map[string]any", "named map":
		m := map[string]any{}
		for i, key := range o.keys {
			m[key] = values[i]
		}
		c.used(form)
		if form == "named map" {
			return goObject(m)
		}
		return m
	case "map[any]any":
		c.used(form)
		m := map[any]any{}
		for i, key := range o.keys {
			m[key] = values[i]
		}
		return m
	case "typed map":
		t := reflect.TypeOf(values[0])
		c.used("map[string]" + typeName(t))
		m := reflect.MakeMap(reflect.MapOf(reflect.TypeFor[string](), t))
		for i, key := range o.keys {
			m.SetMapIndex(reflect.ValueOf(key), reflect.ValueOf(values[i]))
		}
		return m.Interface()
	}
	// A struct of its own, its fields named by cleanrules tags in the keys'
	// order, of the values' types or of type any, in turn.
	c.used(form)
	typed := c.pick(2) == 0
	fieldList := make([]reflect.StructField, len(o.keys))
	for i, key := range o.keys {
		t := anyType
		if typed && values[i] != nil {
			t = reflect.TypeOf(values[i])
		}
		fieldList[i] = reflect.StructField{
			Name: "F" + strconv.Itoa(i),
			Type: t,
			Tag:  reflect.StructTag("cleanrules:" + strconv.Quote(key)),
		}
	}
	s := reflect.New(reflect.StructOf(fieldList))
	for i, v := range values {
		if v != nil {
			s.Elem().Field(i).Set(reflect.ValueOf(v))
		}
	}
	if form == "*struct" {
		return s.Interface()
	}
	return s.Elem().Interface()
}

// ---- Values given back, as values of the format.

// typedValue is a typed value of the format: {key: v}.
func typedValue(key string, v any) *jsonObject { return newObject().set(key, v) }

// intTree is an int in its canonical form (spec/json-format.md, Values).
func intTree(n int64) any {
	if n > 1<<53-1 || n < -(1<<53-1) {
		return typedValue("$int", strconv.FormatInt(n, 10))
	}
	return json.Number(strconv.FormatInt(n, 10))
}

// floatTree is a float in its canonical form: shortest digits, with a point
// or an exponent; infinities typed.
func floatTree(f float64) any {
	switch {
	case math.IsInf(f, 1):
		return typedValue("$float", "inf")
	case math.IsInf(f, -1):
		return typedValue("$float", "-inf")
	case math.IsNaN(f):
		failf("a NaN given back, which is a null")
	}
	s := strconv.FormatFloat(f, 'g', -1, 64)
	if !strings.ContainsAny(s, ".eE") {
		s += ".0"
	}
	return json.Number(s)
}

// awareTree is a time.Time given back, written as the format writes an
// aware datetime: Z for time.UTC; otherwise its offset, and its zone's name
// when it is in a zone loaded by name.
func awareTree(t time.Time) any {
	if t.Nanosecond()%1000 != 0 {
		failf("a datetime finer than a microsecond given back: %v", t)
	}
	text := localText(t.Year(), int(t.Month()), t.Day(), t.Hour(), t.Minute(), t.Second(), t.Nanosecond()/1000)
	if t.Location() == time.UTC {
		return typedValue("$datetime", text+"Z")
	}
	_, offset := t.Zone()
	sign := '+'
	if offset < 0 {
		sign, offset = '-', -offset
	}
	text += fmt.Sprintf("%c%02d:%02d", sign, offset/3600, offset/60%60)
	if offset%60 != 0 {
		text += fmt.Sprintf(":%02d", offset%60)
	}
	if name := t.Location().String(); name != "" {
		text += "[" + name + "]"
	}
	return typedValue("$datetime", text)
}

func localText(year, month, day, hour, minute, second, us int) string {
	text := fmt.Sprintf("%04d-%02d-%02dT%02d:%02d:%02d", year, month, day, hour, minute, second)
	if us != 0 {
		text += fmt.Sprintf(".%06d", us)
	}
	return text
}

// given turns a Go value given back into a value of the format, checking
// that its Go type is one spec/data-model.md (Go, Values given back) gives.
func given(v any) any {
	switch v := v.(type) {
	case nil:
		return nil
	case bool, string:
		return v
	case int64:
		return intTree(v)
	case float64:
		return floatTree(v)
	case Decimal:
		return typedValue("$decimal", v.String())
	case Date:
		return typedValue("$date", fmt.Sprintf("%04d-%02d-%02d", v.Year, int(v.Month), v.Day))
	case DateTime:
		return typedValue("$datetime", localText(v.Year, int(v.Month), v.Day, v.Hour, v.Minute, v.Second, v.Microsecond))
	case time.Time:
		return awareTree(v)
	case []any:
		if v == nil {
			failf("a nil []any given back, which should be nil")
		}
		out := make([]any, len(v))
		for i, item := range v {
			out[i] = given(item)
		}
		return out
	case map[string]any:
		if v == nil {
			return nil // an output that is not there
		}
		out := givenMap(v)
		for _, key := range out.keys {
			if strings.HasPrefix(key, "$") {
				return typedValue("$map", out)
			}
		}
		return out
	}
	failf("a value given back as a %T (%v), which spec/data-model.md (Go, Values given back) never gives", v, v)
	return nil
}

// givenMap is a map given back, without order: its keys sorted, its order
// not compared.
func givenMap(m map[string]any) *jsonObject {
	out := newObject()
	out.unordered = true
	keys := make([]string, 0, len(m))
	for key := range m {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	for _, key := range keys {
		out.set(key, given(m[key]))
	}
	return out
}

// pathJSON is a path of string keys and int indexes, as the format writes
// it.
func pathJSON(path []any) []any {
	out := make([]any, len(path))
	for i, segment := range path {
		switch s := segment.(type) {
		case string:
			out[i] = s
		case int:
			out[i] = json.Number(strconv.Itoa(s))
		default:
			failf("a path segment given back as a %T", segment)
		}
	}
	return out
}

// pathFrom is a path of the format as Go segments: strings and ints.
func pathFrom(v any) []any {
	list, _ := v.([]any)
	path := make([]any, len(list))
	for i, segment := range list {
		if n, ok := segment.(json.Number); ok {
			index, err := strconv.Atoi(string(n))
			if err != nil {
				failf("the index %s", n)
			}
			path[i] = index
		} else {
			path[i] = segment
		}
	}
	return path
}

// namesTree turns names into a JSON array.
func namesTree(names []string) []any {
	out := make([]any, len(names))
	for i, name := range names {
		out[i] = name
	}
	return out
}

// typeName names a Go type for the report: the structs this runner makes
// as struct, and interface{} as any.
func typeName(t reflect.Type) string {
	switch {
	case t.Name() != "":
		return t.String()
	case t.Kind() == reflect.Pointer:
		return "*" + typeName(t.Elem())
	case t.Kind() == reflect.Slice:
		return "[]" + typeName(t.Elem())
	case t.Kind() == reflect.Map:
		return "map[" + typeName(t.Key()) + "]" + typeName(t.Elem())
	case t.Kind() == reflect.Struct:
		return "struct"
	case t.Kind() == reflect.Interface:
		return "any"
	}
	return t.String()
}
