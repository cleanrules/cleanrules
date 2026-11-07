package cleanrules

import (
	"math"
	"math/big"
	"reflect"
	"testing"
	"time"
)

type status string

type level int8

// TestValuesBothWays reads each kind of Go value (spec/data-model.md, Go) and
// checks the value it gives back.
func TestValuesBothWays(t *testing.T) {
	madrid := must(time.LoadLocation("Europe/Madrid"))(t)
	instant := time.Date(2026, 10, 31, 9, 30, 0, 123456789, time.UTC)
	tests := []struct {
		name string
		in   any
		want any
	}{
		{"nil", nil, nil},
		{"bool", true, true},
		{"int", 7, int64(7)},
		{"int8", int8(-8), int64(-8)},
		{"int16", int16(16), int64(16)},
		{"int32", int32(-32), int64(-32)},
		{"int64", int64(math.MinInt64), int64(math.MinInt64)},
		{"uint", uint(7), int64(7)},
		{"uint8", uint8(255), int64(255)},
		{"uint16", uint16(16), int64(16)},
		{"uint32", uint32(math.MaxUint32), int64(math.MaxUint32)},
		{"uint64", uint64(math.MaxInt64), int64(math.MaxInt64)},
		{"uint64 overflow", uint64(math.MaxUint64), nil},
		{"named int", level(3), int64(3)},
		{"big.Int pointer", big.NewInt(-42), int64(-42)},
		{"big.Int value", *big.NewInt(42), int64(42)},
		{"big.Int overflow", new(big.Int).Lsh(big.NewInt(1), 70), nil},
		{"float32 widened", float32(0.1), float64(float32(0.1))},
		{"float64", 2.5, 2.5},
		{"NaN", math.NaN(), nil},
		{"infinity", math.Inf(-1), math.Inf(-1)},
		{"decimal", MustDecimal("1.50"), MustDecimal("1.50")},
		{"decimal canonical", MustDecimal("1.5e-3"), MustDecimal("0.0015")},
		{"decimal infinity", MustDecimal("-inf"), MustDecimal("-inf")},
		{"decimal overflow", MustDecimal("1e40"), nil},
		{"zero decimal", Decimal{}, MustDecimal("0")},
		{"string", "olé", "olé"},
		{"named string", status("gold"), "gold"},
		{"date", Date{2026, time.October, 31}, Date{2026, time.October, 31}},
		{"datetime", DateTime{2026, time.October, 31, 9, 30, 0, 5}, DateTime{2026, time.October, 31, 9, 30, 0, 5}},
		{"time in UTC, truncated", instant, instant.Truncate(time.Microsecond)},
		{"pointer", &[]int{1, 2}, []any{int64(1), int64(2)}},
		{"slice", []string{"a", "b"}, []any{"a", "b"}},
		{"array", [2]float64{1, 2}, []any{1.0, 2.0}},
		{"slice of any", []any{1, "a", nil}, []any{int64(1), "a", nil}},
		{"map", map[string]int{"b": 2, "a": 1}, map[string]any{"a": int64(1), "b": int64(2)}},
		{"named key", map[status]bool{"x": true}, map[string]any{"x": true}},
		{"struct", struct {
			A int
			b int
		}{1, 2}, map[string]any{"A": int64(1)}},
		{"nil pointer", (*int)(nil), nil},
		{"nil slice", []int(nil), nil},
		{"nil map", map[string]int(nil), nil},
		{"zero value", 0, int64(0)},
		{"zone", time.Date(2026, 3, 29, 3, 0, 0, 0, madrid), time.Date(2026, 3, 29, 3, 0, 0, 0, madrid)},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := echoed(t, map[string]any{"x": tt.in})
			if !sameValue(got, tt.want) {
				t.Errorf("got %#v (%T), want %#v (%T)", got, got, tt.want, tt.want)
			}
		})
	}
}

// sameValue compares values given back, times by instant and location.
func sameValue(got, want any) bool {
	if g, ok := got.(time.Time); ok {
		w, ok := want.(time.Time)
		return ok && g.Equal(w) && g.Location().String() == w.Location().String()
	}
	if g, ok := got.(float64); ok {
		w, ok := want.(float64)
		return ok && math.Float64bits(g) == math.Float64bits(w)
	}
	return reflect.DeepEqual(got, want)
}

// TestTimeZones checks how a time.Time's location is read: time.UTC is Z,
// time.Local and fixed zones are offsets, and loaded zones keep their name.
func TestTimeZones(t *testing.T) {
	madrid := must(time.LoadLocation("Europe/Madrid"))(t)
	fixedNamed := time.FixedZone("Europe/Madrid", 3600)
	tests := []struct {
		name     string
		in       time.Time
		location string
		offset   int
	}{
		{"UTC", time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC), "UTC", 0},
		{"zone", time.Date(2026, 7, 1, 12, 0, 0, 0, madrid), "Europe/Madrid", 7200},
		{"fixed zone named as a zone", time.Date(2026, 7, 1, 12, 0, 0, 0, fixedNamed), "", 3600},
		{"fixed offset", time.Date(2026, 7, 1, 12, 0, 0, 0, time.FixedZone("", -5*3600-1800)), "", -5*3600 - 1800},
		{"loaded zone of one offset", time.Date(2026, 7, 1, 12, 0, 0, 0, must(time.LoadLocation("Etc/GMT+5"))(t)), "", -5 * 3600},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, ok := echoed(t, map[string]any{"x": tt.in}).(time.Time)
			if !ok {
				t.Fatalf("not a time.Time")
			}
			_, offset := got.Zone()
			if !got.Equal(tt.in) || offset != tt.offset || got.Location().String() != tt.location {
				t.Errorf("got %v in %q, want %v in %q at %d", got, got.Location(), tt.in, tt.location, tt.offset)
			}
		})
	}
	// time.Local is its offset at that instant, never a zone.
	local := time.Date(2026, 7, 1, 12, 0, 0, 0, time.Local)
	got := echoed(t, map[string]any{"x": local}).(time.Time)
	_, want := local.Zone()
	if _, offset := got.Zone(); !got.Equal(local) || offset != want || got.Location() == time.Local {
		t.Errorf("time.Local: got %v", got)
	}
}

// TestOpaqueValues checks that values with no equivalent come back as the
// same object, and are never read through.
func TestOpaqueValues(t *testing.T) {
	bytes := []byte("abc")
	ch := make(chan int)
	fn := func() {}
	rs := echo(t)
	invalid := string([]byte{0xff, 'a'})
	type node struct {
		Next *node
		V    int
	}
	cycle := &node{V: 1}
	cycle.Next = cycle

	if got := echoed(t, map[string]any{"x": bytes}).([]byte); &got[0] != &bytes[0] {
		t.Error("a []byte does not come back as the same slice")
	}
	if got := echoed(t, map[string]any{"x": ch}); got != any(ch) {
		t.Error("a channel does not come back as itself")
	}
	if got, ok := echoed(t, map[string]any{"x": fn}).(func()); !ok || got == nil {
		t.Error("a function does not come back")
	}
	if got := echoed(t, map[string]any{"x": rs}); got != any(rs) {
		t.Error("a *RuleSet does not come back as the same pointer")
	}
	for name, v := range map[string]any{
		"complex":            complex(1, 2),
		"duration":           2 * time.Second,
		"big.Float":          big.NewFloat(1.5),
		"invalid UTF-8":      invalid,
		"array of bytes":     [4]byte{1, 2, 3, 4},
		"uintptr":            uintptr(7),
		"date of year 0":     Date{0, 1, 1},
		"impossible date":    Date{2026, 2, 30},
		"time of year 10000": time.Date(10000, 1, 1, 0, 0, 0, 0, time.UTC),
	} {
		if got := echoed(t, map[string]any{"x": v}); !reflect.DeepEqual(got, v) {
			t.Errorf("%s: got %#v, want the same %#v", name, got, v)
		}
	}
	// A cycle, where a path ends, is opaque where it closes.
	got := echoed(t, map[string]any{"x": cycle}).(map[string]any)
	if got["V"] != int64(1) || got["Next"] != any(cycle) {
		t.Errorf("cycle: got %#v", got)
	}
	// Opaque values compare under ANY by identity: the same slice agrees,
	// two equal but distinct slices do not.
	anyRS := ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyAny)},
		rule(t, "a", Const(true), Then("v", Field("a"))),
		rule(t, "b", Const(true), Then("v", Field("b"))))
	if _, err := must(anyRS.Evaluate(map[string]any{"a": bytes, "b": bytes}))(t).Output(); err != nil {
		t.Errorf("the same slice twice: %v", err)
	}
	other := []byte("abc")
	if _, err := anyRS.Evaluate(map[string]any{"a": bytes, "b": other}); codeOf(err) != CodeHitPolicy {
		t.Errorf("two distinct slices: got %v, want a hit_policy_error", err)
	}
}

// TestNullReasons checks the reasons of nulls read from Go data.
func TestNullReasons(t *testing.T) {
	rs := ruleSet(t, nil, rule(t, "r", Field("x").Gt(0)))
	tests := []struct {
		name string
		in   any
		code string
	}{
		{"uint64 overflow", uint64(math.MaxUint64), "int_overflow"},
		{"big.Int overflow", new(big.Int).Lsh(big.NewInt(1), 64), "int_overflow"},
		{"decimal overflow", MustDecimal("1e40"), "decimal_overflow"},
		{"NaN", math.NaN(), "null"},
		{"nil", nil, "null"},
		{"absent", struct{}{}, "missing_field"},
		{"opaque", complex(1, 1), "type_mismatch"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			data := map[string]any{"x": tt.in}
			if tt.name == "absent" {
				data = map[string]any{}
			}
			cause := firstCause(t, must(rs.Evaluate(data))(t))
			if cause.Truth != TruthUnknown || cause.Reason == nil || cause.Reason.Code != tt.code {
				t.Errorf("got %v with %+v, want unknown with %s", cause.Truth, cause.Reason, tt.code)
			}
		})
	}
}

type Address struct {
	City string `json:"city"`
	Zip  string `cleanrules:"postcode" json:"zip"`
}

type Base struct {
	ID     int
	Shadow string
}

type inner struct {
	Hidden string // promoted from an unexported embedded struct
}

type Customer struct {
	Base
	*inner
	Name    string
	Age     int     `cleanrules:"age"`
	Email   string  `json:"-"`
	Secret  string  `cleanrules:"-" json:"secret"`
	Address Address `json:"address"`
	Shadow  string  // shadows Base.Shadow
	private string
	Tagged  string `json:"tagged,omitempty"`
}

// TestStructs checks how structs are read: exported fields only, named by
// tags, promoted fields as encoding/json chooses them.
func TestStructs(t *testing.T) {
	c := Customer{
		Base:    Base{ID: 7, Shadow: "base"},
		inner:   &inner{Hidden: "h"},
		Name:    "Ana",
		Age:     40,
		Email:   "a@example.com",
		Secret:  "s",
		Address: Address{City: "Sevilla", Zip: "41001"},
		Shadow:  "own",
		private: "p",
		Tagged:  "t",
	}
	got := echoed(t, map[string]any{"x": &c})
	want := map[string]any{
		"ID":      int64(7),
		"Hidden":  "h",
		"Name":    "Ana",
		"age":     int64(40),
		"address": map[string]any{"city": "Sevilla", "postcode": "41001"},
		"Shadow":  "own",
		"tagged":  "t",
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("got %#v\nwant %#v", got, want)
	}
	// Paths read through a struct, by these names; the others are missing.
	rs := ruleSet(t, nil,
		rule(t, "adult", Field("c", "age").Ge(18).And(Field("c", "address", "postcode").StartsWith("41"))),
		rule(t, "unexported", Field("c", "private").IsNull().Not()),
		rule(t, "skipped", Field("c", "Email").IsNull().Not()),
		rule(t, "renamed", Field("c", "Age").IsNull().Not()))
	d := must(rs.Evaluate(map[string]any{"c": c}))(t)
	if fired := d.Fired(); !reflect.DeepEqual(fired, []string{"adult"}) {
		t.Errorf("fired %v", fired)
	}
	// A nil embedded pointer leaves its promoted fields missing.
	c.inner = nil
	if got := echoed(t, map[string]any{"x": c}).(map[string]any); got["Hidden"] != nil {
		t.Errorf("a nil embedded pointer: %#v", got)
	}
}

// TestOnlyPathsRead checks that only the paths the rule set reads are read:
// what lies elsewhere, even a cycle or a huge list, is never touched.
func TestOnlyPathsRead(t *testing.T) {
	type node struct{ Next *node }
	cycle := &node{}
	cycle.Next = cycle
	rs := ruleSet(t, nil, rule(t, "r", Field("items", 1, "price").Gt(10)))
	if paths := must(rs.FieldPaths())(t); !reflect.DeepEqual(paths, [][]any{{"items", 1, "price"}}) {
		t.Fatalf("paths %v", paths)
	}
	data := map[string]any{
		"items": []map[string]any{{"price": 1, "cycle": cycle}, {"price": 20, "big": make([]int, 1<<20)}},
		"other": cycle,
	}
	d := must(rs.Evaluate(data))(t)
	if !d.Results()[0].Fired {
		t.Error("items[1].price > 10 did not fire")
	}
	// An index beyond the list is missing, and a key on a list is too.
	cause := firstCause(t, must(rs.Evaluate(map[string]any{"items": []int{1}}))(t))
	if cause.Reason == nil || cause.Reason.Code != "missing_field" {
		t.Errorf("got %+v", cause.Reason)
	}
}

// TestDecimals checks ParseDecimal.
func TestDecimals(t *testing.T) {
	for in, want := range map[string]string{"007.50": "7.50", "1e2": "100", "-0.00": "0.00", "inf": "inf"} {
		if got := must(ParseDecimal(in))(t).String(); got != want {
			t.Errorf("%s: got %s, want %s", in, got, want)
		}
	}
	if _, err := ParseDecimal("1,5"); codeOf(err) != CodeFormat {
		t.Errorf("1,5: got %v", err)
	}
	defer func() {
		if recover() == nil {
			t.Error("MustDecimal did not panic")
		}
	}()
	MustDecimal("x")
}

// TestDates checks the date types' helpers.
func TestDates(t *testing.T) {
	at := time.Date(2026, 10, 31, 9, 5, 7, 123456789, time.FixedZone("", 3600))
	if got := DateOf(at); got != (Date{2026, time.October, 31}) || got.String() != "2026-10-31" {
		t.Errorf("DateOf: %v", got)
	}
	if got := DateTimeOf(at); got.String() != "2026-10-31T09:05:07.123456" {
		t.Errorf("DateTimeOf: %v", got)
	}
	if got := (DateTime{Year: 1, Month: 1, Day: 1}).String(); got != "0001-01-01T00:00:00" {
		t.Errorf("String: %v", got)
	}
}
