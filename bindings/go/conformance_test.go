package cleanrules

// A smoke pass over some conformance vectors (conformance/README.md), through
// the idiomatic API with Go data: rule sets are built in code from their
// JSON, inputs are Go values, and decisions, errors, explanations and
// summaries are compared as Go values. The full runner is task H.2.

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"math/big"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strconv"
	"strings"
	"testing"
	"time"
)

// The vector files of the smoke pass.
var smokeFiles = []string{
	"rules/policies.json", "rules/aggregates.json", "rules/payloads.json", "rules/strict.json", "rules/batch.json",
	"explain/counterfactuals.json", "explain/notes.json", "explain/requirements.json", "explain/summaries.json",
	"generated/rulesets.json", "generated/explanations.json",
}

// object is a JSON object that keeps its members' order.
type object struct {
	keys   []string
	values map[string]any
}

func (o *object) get(key string) any {
	if o == nil {
		return nil
	}
	return o.values[key]
}

// parseJSON reads JSON without losing what vectors compare: numbers as
// json.Number, objects in order.
func parseJSON(data []byte) (any, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	return parseValue(decoder)
}

func parseValue(d *json.Decoder) (any, error) {
	token, err := d.Token()
	if err != nil {
		return nil, err
	}
	switch token {
	case json.Delim('['):
		list := []any{}
		for d.More() {
			item, err := parseValue(d)
			if err != nil {
				return nil, err
			}
			list = append(list, item)
		}
		_, err := d.Token()
		return list, err
	case json.Delim('{'):
		o := &object{values: map[string]any{}}
		for d.More() {
			key, err := d.Token()
			if err != nil {
				return nil, err
			}
			value, err := parseValue(d)
			if err != nil {
				return nil, err
			}
			o.keys = append(o.keys, key.(string))
			o.values[key.(string)] = value
		}
		_, err := d.Token()
		return o, err
	}
	return token, nil
}

// goData turns a data value of the JSON format into Go data, as a program
// would hold it.
func goData(t *testing.T, v any) any {
	switch v := v.(type) {
	case json.Number:
		s := string(v)
		if strings.ContainsAny(s, ".eE") {
			f, _ := strconv.ParseFloat(s, 64)
			return f
		}
		return goInt(s)
	case []any:
		out := make([]any, len(v))
		for i, item := range v {
			out[i] = goData(t, item)
		}
		return out
	case *object:
		if len(v.keys) == 1 && strings.HasPrefix(v.keys[0], "$") {
			return typedValue(t, v.keys[0], v.values[v.keys[0]])
		}
		out := map[string]any{}
		for _, key := range v.keys {
			out[key] = goData(t, v.values[key])
		}
		return out
	}
	return v // nil, bool, string
}

// goInt is an int: int when it fits in 32 bits, int64 beyond, and a
// *big.Int beyond 64 bits.
func goInt(s string) any {
	if n, err := strconv.ParseInt(s, 10, 64); err == nil {
		if n >= math.MinInt32 && n <= math.MaxInt32 {
			return int(n)
		}
		return n
	}
	n, _ := new(big.Int).SetString(s, 10)
	return n
}

var datetimePattern = regexp.MustCompile(`^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(?:\.(\d{1,6}))?(Z|[+-]\d\d:\d\d(?::\d\d)?)?(?:\[([^\]]+)\])?$`)

func typedValue(t *testing.T, key string, v any) any {
	switch key {
	case "$int":
		if s, ok := v.(string); ok {
			return goInt(s) // strconv reads leading zeros, as the format does
		}
		return goInt(string(v.(json.Number)))
	case "$float":
		switch v {
		case "inf":
			return math.Inf(1)
		case "-inf":
			return math.Inf(-1)
		}
		f, _ := strconv.ParseFloat(string(v.(json.Number)), 64)
		return f
	case "$decimal":
		return MustDecimal(v.(string))
	case "$date":
		at, err := time.Parse(time.DateOnly, v.(string))
		if err != nil {
			t.Fatalf("date %v: %v", v, err)
		}
		return DateOf(at)
	case "$datetime":
		return parseDatetime(t, v.(string))
	case "$map":
		o := v.(*object)
		out := map[string]any{}
		for _, k := range o.keys {
			out[k] = goData(t, o.values[k])
		}
		return out
	case "$opaque":
		return opaqueMark{}
	}
	t.Fatalf("unknown typed value %s", key)
	return nil
}

// opaqueMark is an expected opaque value: any opaque value equals it.
type opaqueMark struct{}

func parseDatetime(t *testing.T, s string) any {
	m := datetimePattern.FindStringSubmatch(s)
	if m == nil {
		t.Fatalf("datetime %q", s)
	}
	n := func(i int) int { v, _ := strconv.Atoi(m[i]); return v }
	us := 0
	if m[7] != "" {
		us, _ = strconv.Atoi((m[7] + "00000")[:6])
	}
	if m[8] == "" {
		return DateTime{n(1), time.Month(n(2)), n(3), n(4), n(5), n(6), us}
	}
	var loc *time.Location
	offset := 0
	if m[8] != "Z" {
		parts := strings.Split(m[8][1:], ":")
		h, _ := strconv.Atoi(parts[0])
		mi, _ := strconv.Atoi(parts[1])
		sec := 0
		if len(parts) == 3 {
			sec, _ = strconv.Atoi(parts[2])
		}
		offset = h*3600 + mi*60 + sec
		if m[8][0] == '-' {
			offset = -offset
		}
	}
	switch {
	case m[9] != "":
		loc = must(time.LoadLocation(m[9]))(t)
	case offset == 0:
		loc = time.UTC
	default:
		loc = time.FixedZone("", offset)
	}
	// The instant, from the local fields and the offset written.
	at := time.Date(n(1), time.Month(n(2)), n(3), n(4), n(5), n(6), us*1000, time.UTC).Add(-time.Duration(offset) * time.Second)
	return at.In(loc)
}

// exprOf builds a node of the JSON format with the Go API.
func exprOf(t *testing.T, v any) *Expr {
	o := v.(*object)
	op := o.get("op").(string)
	sub := func(key string) *Expr { return exprOf(t, o.get(key)) }
	switch op {
	case "field":
		var path []any
		for _, segment := range o.get("path").([]any) {
			if n, ok := segment.(json.Number); ok {
				i, _ := strconv.Atoi(string(n))
				path = append(path, i)
			} else {
				path = append(path, segment)
			}
		}
		return Field(path...)
	case "const":
		return Const(goData(t, o.get("value")))
	case "eq":
		return sub("left").Eq(sub("right"))
	case "ne":
		return sub("left").Ne(sub("right"))
	case "lt":
		return sub("left").Lt(sub("right"))
	case "le":
		return sub("left").Le(sub("right"))
	case "gt":
		return sub("left").Gt(sub("right"))
	case "ge":
		return sub("left").Ge(sub("right"))
	case "add":
		return sub("left").Add(sub("right"))
	case "sub":
		return sub("left").Sub(sub("right"))
	case "mul":
		return sub("left").Mul(sub("right"))
	case "div":
		return sub("left").Div(sub("right"))
	case "and", "or":
		var operands []*Expr
		for _, item := range o.get("operands").([]any) {
			operands = append(operands, exprOf(t, item))
		}
		if op == "and" {
			return And(operands...)
		}
		return Or(operands...)
	case "not":
		return Not(sub("operand"))
	case "neg":
		return sub("operand").Neg()
	case "abs":
		return sub("operand").Abs()
	case "is_null":
		return sub("operand").IsNull()
	case "in":
		var values []any
		for _, item := range o.get("values").([]any) {
			values = append(values, goData(t, item))
		}
		return sub("operand").IsIn(values...)
	case "between":
		closed := ClosedBoth
		if c, ok := o.get("closed").(string); ok {
			closed = Closed(c)
		}
		return sub("operand").BetweenClosed(sub("low"), sub("high"), closed)
	case "matches":
		return sub("operand").Matches(o.get("pattern").(string))
	case "starts_with":
		return sub("operand").StartsWith(o.get("text").(string))
	case "ends_with":
		return sub("operand").EndsWith(o.get("text").(string))
	case "contains":
		return sub("operand").Contains(o.get("text").(string))
	}
	t.Fatalf("unknown op %s", op)
	return nil
}

// ruleSetOf builds a rule set document's content with the Go API, and
// checks each condition against the core's reading of the same JSON.
func ruleSetOf(t *testing.T, v any) (*RuleSet, error) {
	o := v.(*object)
	var rules []*Rule
	for _, item := range o.get("rules").([]any) {
		r := item.(*object)
		when := exprOf(t, r.get("when"))
		if raw, err := json.Marshal(toPlain(r.get("when"))); err == nil && when.Err() == nil {
			read := must(readNode(string(raw)))(t)
			if !when.Equal(read) {
				t.Errorf("built %v, the core read %v", when, read)
			}
		}
		var options []RuleOption
		if then, ok := r.get("then").(*object); ok {
			for _, key := range then.keys {
				options = append(options, Then(key, exprOf(t, then.values[key])))
			}
		}
		if p, ok := r.get("priority").(json.Number); ok {
			n, _ := p.Int64()
			options = append(options, WithPriority(n))
		}
		if tags, ok := r.get("tags").([]any); ok {
			var names []string
			for _, tag := range tags {
				names = append(names, tag.(string))
			}
			options = append(options, WithTags(names...))
		}
		if d, ok := r.get("description").(string); ok {
			options = append(options, WithDescription(d))
		}
		if m, ok := r.get("metadata").(*object); ok {
			options = append(options, WithMetadata(goData(t, m).(map[string]any)))
		}
		if do, ok := r.get("do").(string); ok {
			options = append(options, WithAction(do))
		}
		built, err := NewRule(r.get("name").(string), when, options...)
		if err != nil {
			return nil, err
		}
		rules = append(rules, built)
	}
	var options []RuleSetOption
	if p, ok := o.get("hit_policy").(string); ok {
		options = append(options, WithHitPolicy(HitPolicy(p)))
	}
	if a, ok := o.get("aggregate").(string); ok {
		options = append(options, WithAggregate(Aggregate(a)))
	}
	if l, ok := o.get("logic").(string); ok {
		options = append(options, WithLogic(l))
	}
	if s, ok := o.get("strict").(bool); ok {
		options = append(options, WithStrict(s))
	}
	return NewRuleSet(rules, options...)
}

// toPlain turns an ordered value into one encoding/json writes (map order
// does not matter to the core's reader).
func toPlain(v any) any {
	switch v := v.(type) {
	case *object:
		m := map[string]any{}
		for _, k := range v.keys {
			m[k] = toPlain(v.values[k])
		}
		return m
	case []any:
		out := make([]any, len(v))
		for i, item := range v {
			out[i] = toPlain(item)
		}
		return out
	}
	return v
}

// expected turns an expected value of the JSON format into the Go value
// given back: ints as int64.
func expected(t *testing.T, v any) any {
	switch g := goData(t, v).(type) {
	case int:
		return int64(g)
	case []any:
		for i, item := range g {
			g[i] = widen(item)
		}
		return g
	case map[string]any:
		for k, item := range g {
			g[k] = widen(item)
		}
		return g
	default:
		return g
	}
}

func widen(v any) any {
	switch v := v.(type) {
	case int:
		return int64(v)
	case []any:
		for i, item := range v {
			v[i] = widen(item)
		}
	case map[string]any:
		for k, item := range v {
			v[k] = widen(item)
		}
	}
	return v
}

// equalValues compares values given back with expected ones.
func equalValues(got, want any) bool {
	if want == nil {
		v := reflect.ValueOf(got)
		return got == nil || (v.Kind() == reflect.Map || v.Kind() == reflect.Slice) && v.IsNil()
	}
	switch w := want.(type) {
	case opaqueMark:
		return true
	case time.Time:
		g, ok := got.(time.Time)
		_, go_ := g.Zone()
		_, wo := w.Zone()
		return ok && g.Equal(w) && go_ == wo && g.Location().String() == w.Location().String()
	case float64:
		g, ok := got.(float64)
		return ok && math.Float64bits(g) == math.Float64bits(w)
	case []any:
		g, ok := got.([]any)
		if !ok || len(g) != len(w) {
			return false
		}
		for i := range w {
			if !equalValues(g[i], w[i]) {
				return false
			}
		}
		return true
	case map[string]any:
		g, ok := got.(map[string]any)
		if !ok || g == nil || len(g) != len(w) {
			return false
		}
		for k := range w {
			if _, present := g[k]; !present || !equalValues(g[k], w[k]) {
				return false
			}
		}
		return true
	}
	return reflect.DeepEqual(got, want)
}

// compareError compares an error with an expected one by its JSON form's
// compared fields (spec/errors.md, Comparing errors).
func compareError(err error, want any) string {
	e, ok := err.(*Error)
	if !ok {
		return fmt.Sprintf("got %v, want error %v", err, toPlain(want))
	}
	// The compared fields, from the Go fields of the error.
	got := map[string]any{"code": string(e.Code)}
	if e.Pointer != nil {
		got["pointer"] = *e.Pointer
	}
	if e.Line != 0 {
		got["line"], got["column"] = float64(e.Line), float64(e.Column)
	}
	if e.Rule != "" {
		got["rule"] = e.Rule
	}
	if e.Path != nil {
		path := make([]any, len(e.Path))
		for i, segment := range e.Path {
			if n, ok := segment.(int); ok {
				path[i] = float64(n)
			} else {
				path[i] = segment
			}
		}
		got["path"] = path
	}
	if e.Row >= 0 {
		got["row"] = float64(e.Row)
	}
	if e.Name != "" {
		got["name"] = e.Name
	}
	for key, names := range map[string][]string{"rules": e.Rules, "types": e.Types, "executed": e.Executed} {
		if len(names) > 0 {
			list := make([]any, len(names))
			for i, name := range names {
				list[i] = name
			}
			got[key] = list
		}
	}
	var plain map[string]any
	raw, _ := json.Marshal(toPlain(want))
	_ = json.Unmarshal(raw, &plain)
	if !reflect.DeepEqual(got, plain) {
		return fmt.Sprintf("error %v, want %v", got, plain)
	}
	// And the core's JSON form says the same.
	var form map[string]any
	_ = json.Unmarshal([]byte(e.JSON), &form)
	for _, key := range []string{"code", "pointer", "line", "column", "rule", "rules", "path", "types", "row", "name", "executed"} {
		if !reflect.DeepEqual(form[key], got[key]) {
			return fmt.Sprintf("error field %s: %v, JSON form %v", key, got[key], form[key])
		}
	}
	return ""
}

// compareDecision compares a decision with an expected decision document.
func compareDecision(t *testing.T, d *Decision, want *object) string {
	results := want.get("results").([]any)
	got := d.Results()
	if len(got) != len(results) {
		return fmt.Sprintf("%d results, want %d", len(got), len(results))
	}
	for i, item := range results {
		r := item.(*object)
		truth := TruthUnknown
		switch r.get("when").(*object).get("truth") {
		case true:
			truth = TruthTrue
		case false:
			truth = TruthFalse
		}
		if got[i].Rule.Name() != r.get("rule") || got[i].Fired != (r.get("fired") == true) || got[i].Truth != truth {
			return fmt.Sprintf("result %d: %s fired %v truth %v", i, got[i].Rule.Name(), got[i].Fired, got[i].Truth)
		}
		then, ok := r.get("then").(*object)
		if !ok {
			if got[i].Output != nil {
				return fmt.Sprintf("result %d: output %v, want none", i, got[i].Output)
			}
			continue
		}
		output := map[string]any{}
		for _, key := range then.keys {
			output[key] = expected(t, then.values[key].(*object).get("value"))
		}
		if got[i].Output == nil || !equalValues(got[i].Output, output) {
			return fmt.Sprintf("result %d: output %#v, want %#v", i, got[i].Output, output)
		}
	}
	var selected []string
	for _, s := range d.Selected() {
		selected = append(selected, s.Rule.Name())
	}
	var wantSelected []string
	for _, s := range want.get("selected").([]any) {
		wantSelected = append(wantSelected, s.(string))
	}
	if !reflect.DeepEqual(selected, wantSelected) {
		return fmt.Sprintf("selected %v, want %v", selected, wantSelected)
	}
	if aggregated, ok := want.values["aggregated"]; ok {
		if !equalValues(d.Aggregated(), expected(t, aggregated)) {
			return fmt.Sprintf("aggregated %#v, want %#v", d.Aggregated(), expected(t, aggregated))
		}
	}
	return ""
}

// compareExplanation compares an explanation with an expected document.
func compareExplanation(t *testing.T, e *Explanation, want *object) string {
	var selected []string
	for _, s := range want.get("selected").([]any) {
		selected = append(selected, s.(string))
	}
	if string(e.HitPolicy) != want.get("hit_policy") || e.Logic != want.get("logic") || !reflect.DeepEqual(e.Selected, selected) {
		return fmt.Sprintf("header %s %s %v", e.HitPolicy, e.Logic, e.Selected)
	}
	rules := want.get("rules").([]any)
	if len(rules) != len(e.Rules) {
		return "number of rules"
	}
	for i, item := range rules {
		w := item.(*object)
		g := e.Rules[i]
		if g.Rule != w.get("rule") || string(g.Status) != w.get("status") {
			return fmt.Sprintf("rule %d: %s %s", i, g.Rule, g.Status)
		}
		if a, ok := w.values["achievable"]; ok != (g.Achievable != nil) || (ok && *g.Achievable != a) {
			return fmt.Sprintf("rule %s: achievable", g.Rule)
		}
		if note, ok := w.get("note").(*object); ok != (g.Note != nil) {
			return fmt.Sprintf("rule %s: note", g.Rule)
		} else if ok {
			if string(g.Note.Kind) != note.get("kind") || g.Note.Winner != note.get("winner") ||
				!sameInt(g.Note.Priority, note.get("priority")) || !sameInt(g.Note.WinnerPriority, note.get("winner_priority")) {
				return fmt.Sprintf("rule %s: note %+v", g.Rule, g.Note)
			}
		}
		if diff := compareRequirement(t, g.Requirement, w.get("requirement").(*object)); diff != "" {
			return fmt.Sprintf("rule %s: %s", g.Rule, diff)
		}
	}
	return ""
}

func sameInt(got *int64, want any) bool {
	if want == nil {
		return got == nil
	}
	n, _ := want.(json.Number).Int64()
	return got != nil && *got == n
}

func compareRequirement(t *testing.T, got Requirement, want *object) string {
	for _, kind := range []GroupKind{GroupAll, GroupAny} {
		if items, ok := want.get(string(kind)).([]any); ok {
			g, ok := got.(*CauseGroup)
			if !ok || g.Kind != kind || len(g.Items) != len(items) {
				return fmt.Sprintf("group %s", kind)
			}
			for i, item := range items {
				if diff := compareRequirement(t, g.Items[i], item.(*object)); diff != "" {
					return diff
				}
			}
			return ""
		}
	}
	c, ok := got.(*Cause)
	if !ok {
		return "not a cause"
	}
	gotCondition, _ := parseJSON([]byte(c.Condition))
	truth := TruthUnknown
	switch want.get("truth") {
	case true:
		truth = TruthTrue
	case false:
		truth = TruthFalse
	}
	if !sameTree(gotCondition, want.get("condition")) || c.At != want.get("at") || c.Truth != truth || c.Negated != (want.get("negated") == true) {
		return fmt.Sprintf("cause %s at %s", c.Condition, c.At)
	}
	values := want.get("values").([]any)
	if len(values) != len(c.Values) {
		return fmt.Sprintf("cause %s: values %v", c.Condition, c.Values)
	}
	for i, item := range values {
		v := item.(*object)
		if !reflect.DeepEqual(c.Values[i].Path, pathOfJSON(v.get("path"))) || !equalValues(c.Values[i].Value, expected(t, v.get("value"))) {
			return fmt.Sprintf("cause %s: value %d %v", c.Condition, i, c.Values[i])
		}
	}
	if reason, ok := want.get("reason").(*object); ok != (c.Reason != nil) {
		return fmt.Sprintf("cause %s: reason", c.Condition)
	} else if ok {
		var paths [][]any
		if list, ok := reason.get("paths").([]any); ok {
			for _, p := range list {
				paths = append(paths, pathOfJSON(p))
			}
		}
		var types []string
		if list, ok := reason.get("types").([]any); ok {
			for _, ty := range list {
				types = append(types, ty.(string))
			}
		}
		if c.Reason.Code != reason.get("code") || !reflect.DeepEqual(c.Reason.Paths, paths) || !reflect.DeepEqual(c.Reason.Types, types) {
			return fmt.Sprintf("cause %s: reason %+v", c.Condition, c.Reason)
		}
	}
	cf, ok := want.get("counterfactual").(*object)
	if ok != (c.Counterfactual != nil) {
		return fmt.Sprintf("cause %s: counterfactual %+v", c.Condition, c.Counterfactual)
	}
	if ok {
		g := c.Counterfactual
		var gap any
		switch w := cf.get("gap").(type) {
		case nil:
		case *object:
			if days := w.get("days"); days != nil {
				gap = GapDays(expected(t, days).(int64))
			} else if us := w.get("microseconds"); us != nil {
				gap = GapMicroseconds(expected(t, us).(int64))
			} else {
				gap = expected(t, w) // a typed number
			}
		default:
			gap = expected(t, w)
		}
		if !reflect.DeepEqual(g.Path, pathOfJSON(cf.get("path"))) || g.Op != cf.get("op") ||
			!equalValues(g.Target, expected(t, cf.get("target"))) || !equalValues(g.Gap, gap) {
			return fmt.Sprintf("cause %s: counterfactual %+v", c.Condition, g)
		}
	}
	return ""
}

func pathOfJSON(v any) []any {
	var path []any
	for _, segment := range v.([]any) {
		if n, ok := segment.(json.Number); ok {
			i, _ := strconv.Atoi(string(n))
			path = append(path, i)
		} else {
			path = append(path, segment)
		}
	}
	return path
}

// sameTree compares parsed JSON as conformance/README.md says: members in
// order, numbers by lexical class and exact value (and the sign of a zero).
func sameTree(got, want any) bool {
	switch w := want.(type) {
	case *object:
		g, ok := got.(*object)
		if !ok || !reflect.DeepEqual(g.keys, w.keys) {
			return false
		}
		for _, k := range w.keys {
			if !sameTree(g.values[k], w.values[k]) {
				return false
			}
		}
		return true
	case []any:
		g, ok := got.([]any)
		if !ok || len(g) != len(w) {
			return false
		}
		for i := range w {
			if !sameTree(g[i], w[i]) {
				return false
			}
		}
		return true
	case json.Number:
		g, ok := got.(json.Number)
		if !ok {
			return false
		}
		isFloat := func(n json.Number) bool { return strings.ContainsAny(string(n), ".eE") }
		if isFloat(g) != isFloat(w) {
			return false
		}
		if isFloat(w) {
			a, _ := strconv.ParseFloat(string(g), 64)
			b, _ := strconv.ParseFloat(string(w), 64)
			return math.Float64bits(a) == math.Float64bits(b)
		}
		a, _ := new(big.Int).SetString(string(g), 10)
		b, _ := new(big.Int).SetString(string(w), 10)
		return a != nil && b != nil && a.Cmp(b) == 0
	}
	return got == want
}

// compareSummary compares a summary with an expected document.
func compareSummary(s *Summary, want *object) string {
	gotRaw, _ := parseJSON([]byte(s.JSON()))
	if !sameTree(gotRaw.(*object).get("summary"), want) {
		return fmt.Sprintf("summary %s", s.JSON())
	}
	// The objects say the same as the document.
	if string(s.HitPolicy) != want.get("hit_policy") || len(s.Rules) != len(want.get("rules").([]any)) ||
		len(s.Fields) != len(want.get("fields").([]any)) {
		return "summary objects"
	}
	return ""
}

func TestConformanceSmoke(t *testing.T) {
	root := filepath.Join("..", "..", "conformance", "vectors")
	ran := 0
	for _, file := range smokeFiles {
		data, err := os.ReadFile(filepath.Join(root, file))
		if err != nil {
			t.Fatal(err)
		}
		document := must(parseJSON(data))(t).(*object)
		for _, item := range document.get("vectors").([]any) {
			vector := item.(*object)
			id := vector.get("id").(string)
			given, expect := vector.get("given").(*object), vector.get("expect").(*object)
			ran++
			if diff := runVector(t, given, expect); diff != "" {
				t.Errorf("%s: %s", id, diff)
			}
		}
	}
	if ran < 600 {
		t.Errorf("only %d vectors ran", ran)
	}
	t.Logf("%d vectors", ran)
}

func runVector(t *testing.T, given, expect *object) string {
	rs, err := ruleSetOf(t, given.get("ruleset"))
	wantErr, hasErr := expect.values["error"]
	if err != nil {
		if hasErr {
			return compareError(err, wantErr)
		}
		return fmt.Sprintf("building the rule set: %v", err)
	}
	switch given.get("kind") {
	case "summary":
		s, err := rs.Summary()
		if err != nil {
			return err.Error()
		}
		if text, ok := expect.get("text").(string); ok && s.String() != text {
			return fmt.Sprintf("text %q, want %q", s.String(), text)
		}
		return compareSummary(s, expect.get("summary").(*object))
	case "evaluate", "explain":
		if inputs, ok := given.values["inputs"]; ok {
			decisions, err := rs.EvaluateMany(goData(t, inputs))
			if hasErr {
				return compareError(err, wantErr)
			}
			if err != nil {
				return err.Error()
			}
			wantDecisions := expect.get("decisions").([]any)
			if len(decisions) != len(wantDecisions) {
				return fmt.Sprintf("%d decisions, want %d", len(decisions), len(wantDecisions))
			}
			for i, want := range wantDecisions {
				if diff := compareDecision(t, decisions[i], want.(*object)); diff != "" {
					return fmt.Sprintf("row %d: %s", i, diff)
				}
			}
			return ""
		}
		d, err := rs.Evaluate(goData(t, given.get("input")))
		if hasErr {
			return compareError(err, wantErr)
		}
		if err != nil {
			return err.Error()
		}
		if diff := compareDecision(t, d, expect.get("decision").(*object)); diff != "" {
			return diff
		}
		if given.get("kind") == "explain" {
			e, err := d.Explain()
			if err != nil {
				return err.Error()
			}
			if text, ok := expect.get("text").(string); ok && e.String() != text {
				return fmt.Sprintf("text %q, want %q", e.String(), text)
			}
			return compareExplanation(t, e, expect.get("explanation").(*object))
		}
		return ""
	}
	return "unknown kind"
}

// TestComparator checks that the smoke pass's comparison is not lenient.
func TestComparator(t *testing.T) {
	parse := func(s string) any { return must(parseJSON([]byte(s)))(t) }
	for _, c := range []struct {
		a, b string
		same bool
	}{
		{`1`, `1`, true}, {`1`, `1.0`, false}, {`1.0`, `1e0`, true}, {`0.0`, `-0.0`, false},
		{`{"a":1,"b":2}`, `{"b":2,"a":1}`, false}, {`[1,2]`, `[1,2]`, true}, {`[1]`, `[1,2]`, false},
		{`123456789012345678901`, `123456789012345678901`, true}, {`"a"`, `"b"`, false}, {`null`, `false`, false},
	} {
		if got := sameTree(parse(c.a), parse(c.b)); got != c.same {
			t.Errorf("%s vs %s: %v", c.a, c.b, got)
		}
	}
	if equalValues(int64(1), 1.0) || !equalValues(map[string]any(nil), nil) || equalValues(math.Copysign(0, -1), 0.0) {
		t.Error("equalValues")
	}
	if compareError(&Error{Code: CodeMissingField, Rule: "a", Row: -1, JSON: `{"code":"missing_field","rule":"a"}`}, parse(`{"code":"missing_field"}`)) == "" {
		t.Error("an extra compared field must differ")
	}
	if compareError(&Error{Code: CodeMissingField, Row: -1, JSON: `{"code":"missing_field","rule":"a"}`}, parse(`{"code":"missing_field"}`)) == "" {
		t.Error("the Go fields and the JSON form must agree")
	}
	if compareError(&Error{Code: CodeMissingField, Row: 2, Path: []any{"a", 0}, JSON: `{"code":"missing_field","path":["a",0],"row":2}`},
		parse(`{"code":"missing_field","path":["a",0],"row":2}`)) != "" {
		t.Error("equal errors")
	}
	if equalValues(map[string]any(nil), map[string]any{}) {
		t.Error("a payload not evaluated is not an empty one")
	}
}
