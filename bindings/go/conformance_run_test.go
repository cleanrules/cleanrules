package cleanrules

import (
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"sort"
	"strings"
	"testing"
	"time"
)

// reflectArray is items as an array of any.
func reflectArray(items []any) any {
	array := reflect.New(reflect.ArrayOf(len(items), anyType)).Elem()
	for i, item := range items {
		if item != nil {
			array.Index(i).Set(reflect.ValueOf(item))
		}
	}
	return array.Interface()
}

// routeCounts is what a route did with the vectors.
type routeCounts struct {
	passed, failed int
	skipped        map[string][]string // ids, by reason
}

// conformanceReport is where TestConformance writes its report, besides the
// test log: under build/, which git ignores.
var conformanceReport = filepath.Join("build", "reports", "conformance.txt")

// TestConformance runs every vector of conformance/vectors by both routes,
// reports each failure with its route and id, and checks that every vector
// ran both ways but those skipped for the reasons of conformanceSkips.
func TestConformance(t *testing.T) {
	files := vectorFiles(t)
	c := newConformance()
	counts := map[string]*routeCounts{}
	for _, route := range conformanceRoutes {
		counts[route] = &routeCounts{skipped: map[string][]string{}}
	}
	seen := map[string]*jsonObject{}
	total := 0
	started := time.Now()
	for _, file := range files {
		t.Run(file.name, func(t *testing.T) {
			for _, vector := range file.vectors {
				id := vector.str("id")
				if seen[id] != nil {
					t.Errorf("the id %s is repeated", id)
				}
				seen[id] = vector
				total++
				for _, route := range conformanceRoutes {
					failure, skip := c.run(vector, route)
					count := counts[route]
					switch {
					case skip != "":
						count.skipped[skip] = append(count.skipped[skip], id)
					case failure != "":
						count.failed++
						t.Errorf("%s:%s: %s", route, id, failure)
					default:
						count.passed++
					}
				}
			}
		})
	}
	elapsed := time.Since(started)

	report := []string{fmt.Sprintf("conformance on Go %s (%s/%s): %d vectors in %d files, %.1f s",
		runtime.Version(), runtime.GOOS, runtime.GOARCH, total, len(files), elapsed.Seconds())}
	for _, route := range conformanceRoutes {
		count := counts[route]
		skips := 0
		reasons := make([]string, 0, len(count.skipped))
		for reason, ids := range count.skipped {
			skips += len(ids)
			reasons = append(reasons, reason)
		}
		sort.Strings(reasons)
		report = append(report, fmt.Sprintf("%s: %d of %d passed, %d failed, %d skipped", route, count.passed, total, count.failed, skips))
		for _, reason := range reasons {
			ids := count.skipped[reason]
			report = append(report, fmt.Sprintf("  %d skipped: %s: %s", len(ids), conformanceSkips[reason], strings.Join(ids, ", ")))
			if !slices.Contains(allowedSkips[route], reason) {
				t.Errorf("%s skips %d vectors for %s, which that route may not skip", route, len(ids), reason)
			}
		}
		if count.passed+count.failed+skips != total {
			t.Errorf("%s: %d passed, %d failed and %d skipped, of %d vectors", route, count.passed, count.failed, skips, total)
		}
	}
	forms := make([]string, 0, len(c.forms))
	for form, ids := range c.forms {
		forms = append(forms, fmt.Sprintf("%s %d", form, len(ids)))
	}
	sort.Strings(forms)
	report = append(report, "native: vectors using each Go form (inputs, constants, metadata, grid cells, calls): "+strings.Join(forms, ", "))
	text := strings.Join(report, "\n")
	t.Log("\n" + text)
	if err := os.MkdirAll(filepath.Dir(conformanceReport), 0o777); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(conformanceReport, []byte(text+"\n"), 0o666); err != nil {
		t.Fatal(err)
	}

	if total < minimumVectors {
		t.Errorf("only %d vectors, expected at least %d", total, minimumVectors)
	}
	// The only skips of the JSON route: grids, and evaluate vectors with
	// limits (max_values bounds native data only).
	var grids, limited, zero []string
	for _, file := range files {
		for _, vector := range file.vectors {
			given, id := vector.obj("given"), vector.str("id")
			if given.has("table_grid") {
				grids = append(grids, id)
			}
			if given.str("kind") == "evaluate" && given.has("limits") {
				limited = append(limited, id)
			}
			if limits := given.obj("limits"); limits != nil {
				for _, value := range limits.values {
					if fmt.Sprint(value) == "0" {
						zero = append(zero, id)
					}
				}
			}
		}
	}
	if !slices.Equal(counts["json"].skipped["grid"], grids) {
		t.Errorf("json skipped %v as grids, the grids are %v", counts["json"].skipped["grid"], grids)
	}
	if !slices.Equal(counts["json"].skipped["native"], limited) {
		t.Errorf("json skipped %v for max_values, the vectors with it are %v", counts["json"].skipped["native"], limited)
	}
	// A limit of 0 is the one vector Go cannot give.
	if !slices.Equal(counts["native"].skipped["zero_limit"], zero) || len(zero) != 1 {
		t.Errorf("native skipped %v for a limit of 0, the vectors with one are %v", counts["native"].skipped["zero_limit"], zero)
	}
}

// TestConformanceComparator checks the comparison on cases the vectors may
// not reach, as the other runners check theirs.
func TestConformanceComparator(t *testing.T) {
	parse := func(s string) any { return must(parseJSONText(s))(t) }
	for _, c := range []struct {
		a, b string
		same bool
	}{
		{`18446744073709551617`, `18446744073709551617`, true}, // beyond 64 bits
		{`18446744073709551617`, `18446744073709551616`, false},
		{`9007199254740993`, `9007199254740992`, false}, // beyond 2^53, one apart
		{`-0`, `0`, true}, // the int 0
		{`1`, `1.0`, false},
		{`100.0`, `1e2`, true},
		{`1.0`, `1e0`, true},
		{`0.1`, `0.10000000000000001`, true}, // the same double
		{`-0.0`, `0.0`, false},
		{`[1,2]`, `[2,1]`, false},
		{`[1]`, `[1,2]`, false},
		{`{"a":1,"b":2}`, `{"b":2,"a":1}`, false},
		{`{"a":1}`, `{"a":1,"b":2}`, false},
		{`{"$opaque":"socket"}`, `{"$opaque":"bytes"}`, true},
		{`{"$opaque":"socket"}`, `null`, false},
		{`"é😀"`, `"é😀"`, true},
		{`"a"`, `"b"`, false},
		{`true`, `1`, false},
		{`null`, `false`, false},
		{`{"code":"null","detail":"a"}`, `{"code":"null"}`, false}, // detail only inside a reason
		{`{"reason":{"code":"null","detail":"a"}}`, `{"reason":{"code":"null"}}`, true},
		{`{"detail":1}`, `{"detail":2}`, false}, // a map's member named detail
		{`{"$datetime":"2026-10-31T09:30:00Z"}`, `{"$datetime":"2026-10-31T09:30:00+00:00[UTC]"}`, false},
	} {
		if got := sameJSON(parse(c.a), parse(c.b), modeExact); got != c.same {
			t.Errorf("exact: %s vs %s: %v", c.a, c.b, got)
		}
	}
	// What Go gives back, in that mode only: maps without order, the zone
	// UTC as Z, a zone Go does not know at its offset.
	unordered := func(s string) any {
		o := parse(s).(*jsonObject)
		o.unordered = true
		return o
	}
	for _, c := range []struct {
		a    any
		b    string
		same bool
	}{
		{unordered(`{"a":1,"b":2}`), `{"b":2,"a":1}`, true},
		{unordered(`{"a":1,"b":2}`), `{"b":2,"a":1.0}`, false},
		{unordered(`{"a":1}`), `{"a":1,"b":2}`, false},
		{unordered(`{"a":{"c":1,"d":2}}`), `{"a":{"d":2,"c":1}}`, false}, // an inner map is a map of its own
		{parse(`{"$datetime":"2026-10-31T09:30:00Z"}`), `{"$datetime":"2026-10-31T09:30:00+00:00[UTC]"}`, true},
		{parse(`{"$datetime":"2026-10-31T09:30:00+01:00"}`), `{"$datetime":"2026-10-31T09:30:00+01:00[Mars/Olympus]"}`, true},
		{parse(`{"$datetime":"2026-10-31T09:30:00+01:00"}`), `{"$datetime":"2026-10-31T09:30:00+01:00[Europe/Madrid]"}`, false},
		{parse(`1`), `1.0`, false},
		{parse(`null`), `{"$opaque":"x"}`, false},
	} {
		if got := sameJSON(c.a, parse(c.b), modeGiven); got != c.same {
			t.Errorf("given: %s vs %s: %v", writeJSON(c.a), c.b, got)
		}
	}
	if sameJSON(unordered(`{"a":1,"b":2}`), parse(`{"a":1,"b":2}`), modeExact) {
		t.Error("exact: a map given back is never what the core writes")
	}
	if !sameJSON(nil, parse(`{"$opaque":"x"}`), modeRecorded) {
		t.Error("recorded: an opaque value comes back as nil")
	}
	// The reader keeps what is compared, and refuses what is not JSON.
	if got := writeJSON(parse(`{"b":1.50,"a":[1e2,-0.0,"é\n"]}`)); got != `{"b":1.50,"a":[1e2,-0.0,"é\n"]}` {
		t.Errorf("read and written: %s", got)
	}
	for _, bad := range []string{`{"a":1,"a":2}`, `{"a":1} x`, `[1,]`, `{"a"}`} {
		if _, err := parseJSONText(bad); err == nil {
			t.Errorf("%s was read", bad)
		}
	}
	// Errors compare by their compared fields, without regard to order.
	err := &Error{Code: CodeMissingField, Rule: "a", Path: []any{"x", 0}, Row: 2,
		JSON: `{"code":"missing_field","rule":"a","path":["x",0],"row":2}`}
	if issue := compareError(contextForm(err), parse(`{"code":"missing_field","row":2,"path":["x",0],"rule":"a"}`).(*jsonObject), false); issue != "" {
		t.Error(issue)
	}
	if compareError(contextForm(err), parse(`{"code":"missing_field","path":["x",0],"row":2}`).(*jsonObject), false) == "" {
		t.Error("a compared field the vector lacks must differ")
	}
	if compareError(jsonForm(err), parse(`{"code":"missing_field","rule":"a","path":["x",0]}`).(*jsonObject), true) != "" {
		t.Error("a read compares the fields the vector gives")
	}
	// The Go fields must say what the JSON form says.
	if failure, _ := runStep(func() {
		contextForm(&Error{Code: CodeMissingField, Row: -1, JSON: `{"code":"missing_field","rule":"a"}`})
	}); failure == "" {
		t.Error("an error whose fields and JSON form differ")
	}
	// Values given back of a type the spec does not give fail.
	for _, v := range []any{1, int32(1), float32(1), []string{"a"}, map[string]int64{"a": 1}, &[]any{}} {
		if failure, _ := runStep(func() { given(v) }); failure == "" {
			t.Errorf("%T given back", v)
		}
	}
}

// runStep runs f as the runner runs a vector: its failure, or its skip.
func runStep(f func()) (failure, skip string) {
	defer func() {
		switch r := recover().(type) {
		case nil:
		case vectorSkip:
			skip = r.reason
		case vectorFailure:
			failure = r.message
		default:
			failure = fmt.Sprint(r)
		}
	}()
	f()
	return "", ""
}

// wrongVectors are vectors with their expected output altered, which every
// route must fail (the self-check of each runner): the vector's id, the text
// of its expected output, and what replaces it.
var wrongVectors = [][3]string{
	// A value, a truth value, an int that should be a float, an order.
	{"ast/nodes/div-int", `"value":3.5`, `"value":3.25`},
	{"ast/missing/missing-field", `"truth":null`, `"truth":false`},
	{"rules/batch/decisions", `"value":2}`, `"value":2.0}`},
	{"explain/summaries/pricing", `"priority":10`, `"priority":11`},
	{"explain/summaries/pricing", `"outputs":["discount","total"]`, `"outputs":["total","discount"]`},
	// A cause's place and a counterfactual's gap.
	{"explain/counterfactuals/at-the-limit", `"at":"/decision/results/0/when"`, `"at":"/decision/results/0"`},
	{"explain/counterfactuals/at-the-limit", `"gap":0`, `"gap":1`},
	{"explain/counterfactuals/datetime-gap", `"microseconds":9000000000`, `"microseconds":9000000001`},
	{"explain/counterfactuals/decimal-gap", `"gap":{"$decimal":"0.15"}`, `"gap":{"$decimal":"0.150"}`},
	{"ast/missing/strict-missing", `"path":["age"]`, `"path":["years"]`},
	{"format/conditions/and-flattened", `"path":["c"]`, `"path":["d"]`},
	{"format/tables/basic", "loyal,> 2,-,0.05", "loyal,> 3,-,0.05"},
	// A status, a note's priority, an aggregate, a tag, a datetime, an
	// error's pointer and code.
	{"explain/counterfactuals/at-the-limit", `"status":"not_fired"`, `"status":"unknown"`},
	{"explain/notes/same-priority", `"winner_priority":10`, `"winner_priority":11`},
	{"rules/aggregates/sum", `"aggregated":{"n":5.5`, `"aggregated":{"n":5.25`},
	{"explain/summaries/pricing", `"tag":"risk"`, `"tag":"risky"`},
	{"ast/types/aware-by-instant", "08:30:00Z", "08:30:01Z"},
	{"format/tables/grid-header-empty", `"pointer":"/header/1"`, `"pointer":"/header/0"`},
	{"format/decisions/tree-too-deep", `"code":"limit_exceeded"`, `"code":"format_error"`},
	// Go's own: the order of a map passed through to an output, and a
	// max_values limit's row.
	{"rules/payloads/map-order-kept", `"value":{"b":1,"a":{"z":true,"y":null},`, `"value":{"a":{"z":true,"y":null},"b":1,`},
	{"rules/limits/batch-row", `"row":1`, `"row":0`},
}

// TestConformanceWrongVectors checks that the runner fails vectors whose
// expected output is wrong, by both routes (grids by the native one only,
// max_values vectors too).
func TestConformanceWrongVectors(t *testing.T) {
	vectors := map[string]*jsonObject{}
	for _, file := range vectorFiles(t) {
		for _, vector := range file.vectors {
			vectors[vector.str("id")] = vector
		}
	}
	c := newConformance()
	for _, change := range wrongVectors {
		vector := vectors[change[0]]
		if vector == nil {
			t.Errorf("no vector %s", change[0])
			continue
		}
		expect := writeJSON(vector.get("expect"))
		if !strings.Contains(expect, change[1]) {
			t.Errorf("%s: no %s in %s", change[0], change[1], expect)
			continue
		}
		altered := newObject()
		for _, key := range vector.keys {
			altered.set(key, vector.values[key])
		}
		altered.set("expect", must(parseJSONText(strings.Replace(expect, change[1], change[2], 1)))(t))
		for _, route := range conformanceRoutes {
			failure, skip := c.run(altered, route)
			if failure == "" && !(route == "json" && (skip == "grid" || skip == "native")) {
				t.Errorf("%s:%s passes with %s for %s", route, change[0], change[2], change[1])
			}
		}
	}
}
