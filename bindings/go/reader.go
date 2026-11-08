package cleanrules

/*
#include <stdlib.h>
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"math"
	"math/big"
	"reflect"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"
)

// pathTree holds the paths a rule set or a node reads, merged: a reader
// follows only these (spec/data-model.md, Reading native data).
type pathTree struct {
	end     bool // a path ends here: the value is converted whole
	keys    []string
	byKey   map[string]*pathTree
	indexes []int
	byIndex map[int]*pathTree
}

// newPathTree merges paths of strings and ints, keeping their order.
func newPathTree(paths [][]any) *pathTree {
	root := &pathTree{}
	for _, path := range paths {
		node := root
		for _, segment := range path {
			switch s := segment.(type) {
			case string:
				node = node.key(s)
			case int:
				node = node.index(s)
			}
		}
		node.end = true
	}
	return root
}

func (t *pathTree) key(k string) *pathTree {
	if t.byKey == nil {
		t.byKey = map[string]*pathTree{}
	}
	child, ok := t.byKey[k]
	if !ok {
		child = &pathTree{}
		t.byKey[k] = child
		t.keys = append(t.keys, k)
	}
	return child
}

func (t *pathTree) index(i int) *pathTree {
	if t.byIndex == nil {
		t.byIndex = map[int]*pathTree{}
	}
	child, ok := t.byIndex[i]
	if !ok {
		child = &pathTree{}
		t.byIndex[i] = child
		t.indexes = append(t.indexes, i)
	}
	return child
}

// wholePaths reads a value whole, as constants and metadata are read.
var wholePaths = &pathTree{end: true}

// maxDepth is how deep containers are converted whole, where a path ends:
// deeper ones are opaque (the format's default node depth).
const maxDepth = 256

// The UTC instants of 0001-01-01T00:00:00Z and 9999-12-31T23:59:59Z.
const (
	firstSecond = -62135596800
	lastSecond  = 253402300799
)

var (
	timeType     = reflect.TypeFor[time.Time]()
	durationType = reflect.TypeFor[time.Duration]()
	bigIntType   = reflect.TypeFor[big.Int]()
	bigFloatType = reflect.TypeFor[big.Float]()
	bigRatType   = reflect.TypeFor[big.Rat]()
	decimalType  = reflect.TypeFor[Decimal]()
	dateType     = reflect.TypeFor[Date]()
	dateTimeType = reflect.TypeFor[DateTime]()
	// The package's own objects are opaque, never read through.
	ownTypes = map[reflect.Type]bool{
		reflect.TypeFor[RuleSet]():         true,
		reflect.TypeFor[Rule]():            true,
		reflect.TypeFor[Expr]():            true,
		reflect.TypeFor[Decision]():        true,
		reflect.TypeFor[RuleResult]():      true,
		reflect.TypeFor[Error]():           true,
		reflect.TypeFor[Explanation]():     true,
		reflect.TypeFor[RuleExplanation](): true,
		reflect.TypeFor[Cause]():           true,
		reflect.TypeFor[CauseGroup]():      true,
		reflect.TypeFor[Counterfactual]():  true,
		reflect.TypeFor[Note]():            true,
		reflect.TypeFor[Reason]():          true,
		reflect.TypeFor[FieldValue]():      true,
		reflect.TypeFor[Summary]():         true,
		reflect.TypeFor[RuleSummary]():     true,
		reflect.TypeFor[FieldUsage]():      true,
		reflect.TypeFor[PayloadUse]():      true,
		reflect.TypeFor[TagUsage]():        true,
		reflect.TypeFor[PayloadEntry]():    true,
		reflect.TypeFor[ActionContext]():   true,
		reflect.TypeFor[ruleOption]():      true,
		reflect.TypeFor[DecisionTable]():   true,
		reflect.TypeFor[TableRow]():        true,
		reflect.TypeFor[TableOutput]():     true,
		reflect.TypeFor[ResultTree]():      true,
		reflect.TypeFor[Limits]():          true,
		// The options' own types (the option types are interfaces).
		reflect.TypeFor[hitPolicyOption]():   true,
		reflect.TypeFor[tablePolicyOption](): true,
		reflect.TypeFor[aggregateOption]():   true,
		reflect.TypeFor[logicOption]():       true,
		reflect.TypeFor[strictOption]():      true,
		reflect.TypeFor[formatOption]():      true,
		reflect.TypeFor[limitsOption]():      true,
		reflect.TypeFor[actionsOption]():     true,
	}
)

// A readFailure stops a read when the library fails to build a value, which
// only a bug or a lack of memory causes; read recovers it as an error.
type readFailure struct{ err error }

// identity says when two opaque values are the same native object.
type identity struct {
	t   reflect.Type
	ptr uintptr
	n   int
}

// reader turns Go data into a value of the library (spec/data-model.md, Go),
// reading only the paths of its tree, and keeps the native objects of the
// opaque values it makes, by token, to give them back.
type reader struct {
	opaques  []any
	byValue  map[any]uint64
	byShared map[identity]uint64
	open     map[identity]bool
	depth    int
	// done holds a copy of each container already converted whole, by its
	// identity and depth, so that each native object is read once per input
	// (spec/data-model.md, Reading native data) and data that shares a
	// container in many places is not read again at every place. cycles
	// counts the containers made opaque because they were open, after
	// which a conversion depends on its ancestors and is not kept.
	done   map[memoKey]*C.cr_value
	cycles int
}

type memoKey struct {
	id    identity
	depth int
}

func newReader() *reader {
	return &reader{
		byValue:  map[any]uint64{},
		byShared: map[identity]uint64{},
		open:     map[identity]bool{},
		done:     map[memoKey]*C.cr_value{},
	}
}

// close frees the copies kept by done.
func (r *reader) close() {
	for key, value := range r.done {
		C.cr_value_free(value)
		delete(r.done, key)
	}
}

// read turns data into an owned value, along paths.
func (r *reader) read(data any, paths *pathTree) (value *C.cr_value, err error) {
	defer r.close()
	defer recoverRead(&value, &err)
	return r.walk(reflect.ValueOf(data), paths), nil
}

// readRows reads each item of a slice or an array along paths, into a list;
// nil (a nil slice, pointer or interface) is an empty list, and any other
// data is read whole, for the library to reject.
func (r *reader) readRows(rows any, paths *pathTree) (value *C.cr_value, err error) {
	defer r.close()
	defer recoverRead(&value, &err)
	v := reflect.ValueOf(rows)
	for v.IsValid() && (v.Kind() == reflect.Interface || v.Kind() == reflect.Pointer) && !v.IsNil() {
		v = v.Elem()
	}
	if !v.IsValid() || ((v.Kind() == reflect.Slice || v.Kind() == reflect.Pointer || v.Kind() == reflect.Interface) && v.IsNil()) {
		return newList(0), nil
	}
	if (v.Kind() != reflect.Slice && v.Kind() != reflect.Array) || isBytes(v.Type()) {
		return r.convert(reflect.ValueOf(rows)), nil
	}
	list := newList(v.Len())
	defer freeOnPanic(list)
	for i := range v.Len() {
		setItem(list, i, r.walk(v.Index(i), paths))
	}
	return list, nil
}

// recoverRead turns a readFailure into the read's error; other panics go on.
func recoverRead(value **C.cr_value, err *error) {
	if p := recover(); p != nil {
		failure, ok := p.(readFailure)
		if !ok {
			panic(p)
		}
		*value, *err = nil, failure.err
	}
}

// ---- Walking along paths.

func (r *reader) walk(v reflect.Value, node *pathTree) *C.cr_value {
	inner, outer := deref(v)
	if !inner.IsValid() {
		return C.cr_value_new_null()
	}
	if node.end {
		return r.convert(v)
	}
	switch shape(inner.Type()) {
	case shapeList:
		return r.walkList(inner, node)
	case shapeMap:
		return r.walkMap(inner, node)
	case shapeStruct:
		return r.walkStruct(inner, node)
	case shapeOpaque:
		return r.opaque(outer)
	default:
		return r.convert(v)
	}
}

func (r *reader) walkList(v reflect.Value, node *pathTree) *C.cr_value {
	length := v.Len()
	list := newList(length)
	defer freeOnPanic(list)
	for _, index := range node.indexes {
		if index < length {
			setItem(list, index, r.walk(v.Index(index), node.byIndex[index]))
		}
	}
	return list
}

func (r *reader) walkMap(v reflect.Value, node *pathTree) *C.cr_value {
	m := C.cr_value_new_map()
	defer freeOnPanic(m)
	keyType := v.Type().Key()
	var byText map[string]reflect.Value
	if keyType.Kind() != reflect.String {
		_, byText = stringEntries(v)
	}
	for _, key := range node.keys {
		var item reflect.Value
		if byText == nil {
			item = v.MapIndex(reflect.ValueOf(key).Convert(keyType))
		} else {
			item = byText[key]
		}
		if item.IsValid() {
			insertItem(m, key, r.walk(item, node.byKey[key]))
		}
	}
	return m
}

// stringEntries are the entries of a map whose keys are strings: all of
// them when the key kind is string; for an interface key type, those whose
// dynamic key is a string (when two such keys have the same text, the one
// of type string, or else the one whose type's name sorts first); none for
// any other key kind. Keys that are not valid UTF-8 are left out too.
func stringEntries(v reflect.Value) ([]string, map[string]reflect.Value) {
	keyType := v.Type().Key()
	values := map[string]reflect.Value{}
	types := map[string]reflect.Type{}
	if keyType.Kind() != reflect.String && keyType.Kind() != reflect.Interface {
		return nil, values
	}
	for iter := v.MapRange(); iter.Next(); {
		key := iter.Key()
		if key.Kind() == reflect.Interface {
			if key.IsNil() {
				continue
			}
			key = key.Elem()
		}
		if key.Kind() != reflect.String || !utf8.ValidString(key.String()) {
			continue
		}
		text := key.String()
		if old, ok := types[text]; ok && !betterKey(key.Type(), old) {
			continue
		}
		values[text], types[text] = iter.Value(), key.Type()
	}
	keys := make([]string, 0, len(values))
	for text := range values {
		keys = append(keys, text)
	}
	// Go's maps have no order: sorted by key, by code point, which Go's
	// byte order of valid UTF-8 is.
	sort.Strings(keys)
	return keys, values
}

var stringType = reflect.TypeFor[string]()

func betterKey(t, than reflect.Type) bool {
	if t == stringType || than == stringType {
		return t == stringType
	}
	return t.String() < than.String()
}

func (r *reader) walkStruct(v reflect.Value, node *pathTree) *C.cr_value {
	m := C.cr_value_new_map()
	defer freeOnPanic(m)
	fields := fieldsOf(v.Type())
	for _, key := range node.keys {
		f, ok := fields.byName[key]
		if !ok {
			continue
		}
		item, err := v.FieldByIndexErr(f.index)
		if err != nil {
			continue // through a nil embedded pointer: absent
		}
		insertItem(m, key, r.walk(item, node.byKey[key]))
	}
	return m
}

// ---- Whole values, where a path ends.

func (r *reader) convert(v reflect.Value) *C.cr_value {
	inner, outer := deref(v)
	if !inner.IsValid() {
		return C.cr_value_new_null()
	}
	t := inner.Type()
	switch t {
	case timeType:
		return r.aware(inner.Interface().(time.Time), outer)
	case bigIntType:
		var n *big.Int
		if inner.CanAddr() {
			n = inner.Addr().Interface().(*big.Int)
		} else {
			copied := inner.Interface().(big.Int)
			n = &copied
		}
		if !n.IsInt64() {
			return nullWith(C.CR_REASON_INT_OVERFLOW)
		}
		return C.cr_value_new_int(C.int64_t(n.Int64()))
	case decimalType:
		return r.decimal(inner.Interface().(Decimal), outer)
	case dateType:
		return r.date(inner.Interface().(Date), outer)
	case dateTimeType:
		return r.naive(inner.Interface().(DateTime), outer)
	}
	switch shape(t) {
	case shapeOpaque:
		return r.opaque(outer)
	case shapeList, shapeMap, shapeStruct:
		return r.container(inner, outer)
	}
	switch t.Kind() {
	case reflect.Bool:
		return C.cr_value_new_bool(C.bool(inner.Bool()))
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return C.cr_value_new_int(C.int64_t(inner.Int()))
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		u := inner.Uint()
		if u > math.MaxInt64 {
			return nullWith(C.CR_REASON_INT_OVERFLOW)
		}
		return C.cr_value_new_int(C.int64_t(u))
	case reflect.Float32, reflect.Float64:
		return C.cr_value_new_float(C.double(inner.Float())) // NaN is a plain null
	case reflect.String:
		s := inner.String()
		if !utf8.ValidString(s) {
			return r.opaque(outer)
		}
		var out *C.cr_value
		p, n := text(s)
		r.must(C.cr_value_new_string(p, n, &out, nil))
		return out
	default:
		return r.opaque(outer)
	}
}

// container converts a list, a map or a struct whole, unless it is open
// already (a cycle) or too deep, which is opaque.
func (r *reader) container(v, outer reflect.Value) *C.cr_value {
	id, shared := sharedIdentity(v, outer)
	key := memoKey{id, r.depth}
	if shared {
		if copied, ok := r.done[key]; ok {
			return r.clone(copied)
		}
	}
	if r.depth >= maxDepth || (shared && r.open[id]) {
		if shared && r.open[id] {
			r.cycles++
		}
		return r.opaque(outer)
	}
	cycles := r.cycles
	value := r.whole(v, id, shared)
	if shared && r.cycles == cycles {
		r.done[key] = r.clone(value)
	}
	return value
}

func (r *reader) whole(v reflect.Value, id identity, shared bool) *C.cr_value {
	if shared {
		r.open[id] = true
		defer delete(r.open, id)
	}
	r.depth++
	defer func() { r.depth-- }()
	switch shape(v.Type()) {
	case shapeList:
		list := newList(v.Len())
		defer freeOnPanic(list)
		for i := range v.Len() {
			setItem(list, i, r.convert(v.Index(i)))
		}
		return list
	case shapeMap:
		keys, values := stringEntries(v)
		m := C.cr_value_new_map()
		defer freeOnPanic(m)
		for _, key := range keys {
			insertItem(m, key, r.convert(values[key]))
		}
		return m
	default:
		m := C.cr_value_new_map()
		defer freeOnPanic(m)
		for _, f := range fieldsOf(v.Type()).list {
			item, err := v.FieldByIndexErr(f.index)
			if err != nil {
				continue
			}
			insertItem(m, f.name, r.convert(item))
		}
		return m
	}
}

// clone is an owned copy of a value.
func (r *reader) clone(v *C.cr_value) *C.cr_value {
	var out *C.cr_value
	r.must(C.cr_value_clone(v, &out, nil))
	return out
}

// aware converts a time.Time: its instant, truncated to the microsecond,
// with its zone or its offset (spec/data-model.md, Go).
func (r *reader) aware(t time.Time, outer reflect.Value) *C.cr_value {
	_, offset := t.Zone()
	seconds := t.Unix()
	local := seconds + int64(offset)
	if seconds < firstSecond || seconds > lastSecond || local < firstSecond || local > lastSecond {
		return r.opaque(outer)
	}
	zone := zoneName(t)
	p, n := text(zone)
	var out *C.cr_value
	if C.cr_value_new_aware_datetime(C.int64_t(seconds), C.int32_t(t.Nanosecond()/1000), C.int32_t(offset), p, n, &out, nil) != C.CR_OK {
		return r.opaque(outer)
	}
	return out
}

// zoneName is the IANA zone of t's location, or "" for a fixed offset:
// time.UTC is Z, time.Local and a location in force for all time
// (time.FixedZone) are offsets, and any other location, loaded by name,
// is that zone.
func zoneName(t time.Time) string {
	loc := t.Location()
	if loc == time.UTC || loc == time.Local {
		return ""
	}
	if start, end := t.ZoneBounds(); start.IsZero() && end.IsZero() {
		return ""
	}
	return loc.String()
}

func (r *reader) decimal(d Decimal, outer reflect.Value) *C.cr_value {
	p, n := text(d.String())
	var out *C.cr_value
	if C.cr_value_new_decimal(p, n, &out, nil) != C.CR_OK {
		return r.opaque(outer)
	}
	return out
}

func (r *reader) date(d Date, outer reflect.Value) *C.cr_value {
	if d.Year < 1 || d.Year > 9999 || !fitsInt32(int(d.Month), d.Day) {
		return r.opaque(outer)
	}
	var out *C.cr_value
	if C.cr_value_new_date(C.int32_t(d.Year), C.int32_t(d.Month), C.int32_t(d.Day), &out, nil) != C.CR_OK {
		return r.opaque(outer)
	}
	return out
}

func (r *reader) naive(d DateTime, outer reflect.Value) *C.cr_value {
	if d.Year < 1 || d.Year > 9999 || !fitsInt32(int(d.Month), d.Day, d.Hour, d.Minute, d.Second, d.Microsecond) {
		return r.opaque(outer)
	}
	var out *C.cr_value
	status := C.cr_value_new_naive_datetime(C.int32_t(d.Year), C.int32_t(d.Month), C.int32_t(d.Day),
		C.int32_t(d.Hour), C.int32_t(d.Minute), C.int32_t(d.Second), C.int32_t(d.Microsecond), &out, nil)
	if status != C.CR_OK {
		return r.opaque(outer)
	}
	return out
}

func fitsInt32(values ...int) bool {
	for _, v := range values {
		if v < math.MinInt32 || v > math.MaxInt32 {
			return false
		}
	}
	return true
}

// opaque makes an opaque value of outer, the value as found in the data
// (before its pointers are followed), whose object comes back in outputs.
func (r *reader) opaque(outer reflect.Value) *C.cr_value {
	name := outer.Type().String()
	p, n := text(name)
	var out *C.cr_value
	r.must(C.cr_value_new_opaque(p, n, C.uint64_t(r.token(outer)), &out, nil))
	return out
}

// token gives each native object one token: values that are == (pointers
// and channels to the same object) share it, and so do slices and maps
// sharing their storage; functions and other values that cannot be compared
// get a new one each time.
func (r *reader) token(v reflect.Value) uint64 {
	var object any
	if v.CanInterface() {
		object = v.Interface()
	}
	next := uint64(len(r.opaques))
	switch {
	case object == nil:
	case v.Kind() == reflect.Slice || v.Kind() == reflect.Map:
		id := identity{v.Type(), v.Pointer(), 0}
		if v.Kind() == reflect.Slice {
			id.n = v.Len()
		}
		if token, ok := r.byShared[id]; ok {
			return token
		}
		r.byShared[id] = next
	case v.Kind() != reflect.Func && v.Comparable():
		if token, ok := r.byValue[object]; ok {
			return token
		}
		r.byValue[object] = next
	}
	r.opaques = append(r.opaques, object)
	return next
}

func (r *reader) must(status C.cr_status) {
	if status != C.CR_OK {
		panic(readFailure{newError(CodeInternal, fmt.Sprintf("cleanrules: building a value failed with status %d", int(status)), "please report it")})
	}
}

// ---- Shapes and fields.

type shapeKind int

const (
	shapeScalar shapeKind = iota
	shapeList
	shapeMap
	shapeStruct
	shapeOpaque
)

// shape says how a type is read: through it (lists, maps, structs), as a
// scalar, or as an opaque value never read through.
func shape(t reflect.Type) shapeKind {
	switch t {
	case timeType, bigIntType, decimalType, dateType, dateTimeType:
		return shapeScalar
	case durationType, bigFloatType, bigRatType:
		return shapeOpaque
	}
	if ownTypes[t] {
		return shapeOpaque
	}
	switch t.Kind() {
	case reflect.Slice, reflect.Array:
		if isBytes(t) {
			return shapeOpaque
		}
		return shapeList
	case reflect.Map:
		return shapeMap
	case reflect.Struct:
		return shapeStruct
	case reflect.Complex64, reflect.Complex128, reflect.Chan, reflect.Func, reflect.UnsafePointer, reflect.Uintptr:
		return shapeOpaque
	}
	return shapeScalar
}

// isBytes reports a slice or an array of bytes, which is opaque like bytes in
// other languages.
func isBytes(t reflect.Type) bool {
	return (t.Kind() == reflect.Slice || t.Kind() == reflect.Array) && t.Elem().Kind() == reflect.Uint8
}

// deref follows interfaces and pointers: inner is the value they lead to
// (invalid for a nil pointer, interface, slice or map), and outer the value as
// found, after interfaces only (the pointer, if it is one), which an opaque
// value gives back.
func deref(v reflect.Value) (inner, outer reflect.Value) {
	for v.IsValid() && v.Kind() == reflect.Interface {
		if v.IsNil() {
			return reflect.Value{}, reflect.Value{}
		}
		v = v.Elem()
	}
	outer = v
	for v.IsValid() {
		switch v.Kind() {
		case reflect.Pointer:
			if v.IsNil() {
				return reflect.Value{}, outer
			}
			v = v.Elem()
			for v.Kind() == reflect.Interface {
				if v.IsNil() {
					return reflect.Value{}, outer
				}
				v = v.Elem()
			}
			continue
		case reflect.Slice, reflect.Map:
			if v.IsNil() {
				return reflect.Value{}, outer
			}
		}
		return v, outer
	}
	return v, outer
}

// sharedIdentity is the identity of a container that can contain itself:
// through a pointer, or as a slice or a map.
func sharedIdentity(v, outer reflect.Value) (identity, bool) {
	switch {
	case outer.Kind() == reflect.Pointer:
		return identity{outer.Type(), outer.Pointer(), 0}, true
	case v.Kind() == reflect.Slice:
		return identity{v.Type(), v.Pointer(), v.Len()}, true
	case v.Kind() == reflect.Map:
		return identity{v.Type(), v.Pointer(), 0}, true
	}
	return identity{}, false
}

// field is a struct field read as a map entry.
type field struct {
	name  string
	index []int
}

type fields struct {
	list   []field // in the order of encoding/json
	byName map[string]field
}

var fieldCache sync.Map // reflect.Type -> *fields

// fieldsOf gives the exported fields of a struct type that cleanrules reads,
// by name, as encoding/json's typeFields chooses them: promoted fields of
// embedded structs included, the shallowest one winning a name, or among the
// shallowest the only tagged one, and ambiguous names left out (also those
// of a struct type embedded twice at one depth); named by a cleanrules tag,
// else a json tag, else the field's name; a tag "-" leaves a field out.
func fieldsOf(t reflect.Type) *fields {
	if cached, ok := fieldCache.Load(t); ok {
		return cached.(*fields)
	}
	type candidate struct {
		field
		tagged bool
		typ    reflect.Type
	}
	var found []candidate
	current := []candidate{}
	next := []candidate{{typ: t}}
	var count, nextCount map[reflect.Type]int
	visited := map[reflect.Type]bool{}
	for len(next) > 0 {
		current, next = next, current[:0]
		count, nextCount = nextCount, map[reflect.Type]int{}
		for _, f := range current {
			if visited[f.typ] {
				continue
			}
			visited[f.typ] = true
			for i := range f.typ.NumField() {
				sf := f.typ.Field(i)
				if sf.Anonymous {
					ft := sf.Type
					if ft.Kind() == reflect.Pointer {
						ft = ft.Elem()
					}
					if !sf.IsExported() && ft.Kind() != reflect.Struct {
						continue
					}
				} else if !sf.IsExported() {
					continue
				}
				name, skip := fieldName(sf)
				if skip {
					continue
				}
				index := append(append([]int(nil), f.index...), i)
				ft := sf.Type
				if ft.Name() == "" && ft.Kind() == reflect.Pointer {
					ft = ft.Elem()
				}
				if name != "" || !sf.Anonymous || ft.Kind() != reflect.Struct {
					tagged := name != ""
					if name == "" {
						name = sf.Name
					}
					found = append(found, candidate{field{name, index}, tagged, ft})
					if count[f.typ] > 1 {
						// The type was embedded more than once at this depth:
						// a second copy, so that the name is ambiguous.
						found = append(found, found[len(found)-1])
					}
					continue
				}
				nextCount[ft]++
				if nextCount[ft] == 1 {
					next = append(next, candidate{field: field{ft.Name(), index}, typ: ft})
				}
			}
		}
	}
	// By name, then depth, then tagged first: the first of each name wins,
	// unless the next has its depth and taggedness.
	sort.SliceStable(found, func(i, j int) bool {
		a, b := found[i], found[j]
		if a.name != b.name {
			return a.name < b.name
		}
		if len(a.index) != len(b.index) {
			return len(a.index) < len(b.index)
		}
		if a.tagged != b.tagged {
			return a.tagged
		}
		return lessIndex(a.index, b.index)
	})
	out := &fields{byName: map[string]field{}}
	for i := 0; i < len(found); {
		j := i + 1
		for j < len(found) && found[j].name == found[i].name {
			j++
		}
		group := found[i:j]
		if len(group) == 1 || len(group[0].index) < len(group[1].index) || group[0].tagged != group[1].tagged {
			out.byName[group[0].name] = group[0].field
			out.list = append(out.list, group[0].field)
		}
		i = j
	}
	sort.Slice(out.list, func(i, j int) bool { return lessIndex(out.list[i].index, out.list[j].index) })
	cached, _ := fieldCache.LoadOrStore(t, out)
	return cached.(*fields)
}

// fieldName is a field's name for cleanrules, or "" when it has no tag
// naming it: its cleanrules tag, when it has one (the json tag is then never
// read), else its json tag; skip for the tag "-".
func fieldName(sf reflect.StructField) (name string, skip bool) {
	tag, ok := sf.Tag.Lookup("cleanrules")
	json := false
	if !ok {
		tag, ok = sf.Tag.Lookup("json")
		json = true
	}
	if !ok {
		return "", false
	}
	if tag == "-" {
		return "", true
	}
	name, _, _ = strings.Cut(tag, ",")
	if json && !validJSONName(name) {
		return "", false // encoding/json ignores such a name too
	}
	return name, false
}

// validJSONName is encoding/json's isValidTag: the names it accepts.
func validJSONName(s string) bool {
	if s == "" {
		return false
	}
	for _, c := range s {
		switch {
		case strings.ContainsRune("!#$%&()*+-./:;<=>?@[]^_{|}~ ", c):
		case !unicode.IsLetter(c) && !unicode.IsDigit(c):
			return false
		}
	}
	return true
}

func lessIndex(a, b []int) bool {
	for i := range min(len(a), len(b)) {
		if a[i] != b[i] {
			return a[i] < b[i]
		}
	}
	return len(a) < len(b)
}

// ---- Building values.

func newList(n int) *C.cr_value {
	list := C.cr_value_new_list(C.size_t(n))
	if list == nil {
		// As the Go runtime does when memory runs out.
		panic(fmt.Sprintf("cleanrules: out of memory: cannot allocate a list of %d values", n))
	}
	return list
}

// setItem moves item into the list, which then owns it.
func setItem(list *C.cr_value, i int, item *C.cr_value) {
	var e *C.cr_error
	if err := check(C.cr_value_list_set(list, C.size_t(i), item, &e), e); err != nil {
		panic(readFailure{err})
	}
}

// insertItem moves item into the map under key.
func insertItem(m *C.cr_value, key string, item *C.cr_value) {
	p, n := text(key)
	var e *C.cr_error
	if err := check(C.cr_value_map_insert(m, p, n, item, &e), e); err != nil {
		panic(readFailure{err})
	}
}

// freeOnPanic frees a value being built when a read fails inside it.
func freeOnPanic(v *C.cr_value) {
	if r := recover(); r != nil {
		C.cr_value_free(v)
		panic(r)
	}
}

func nullWith(reason C.cr_null_reason) *C.cr_value {
	var out *C.cr_value
	if C.cr_value_new_null_reason(reason, nil, 0, &out, nil) != C.CR_OK {
		return C.cr_value_new_null()
	}
	return out
}
