package cleanrules

// The conformance vectors in Go (conformance/README.md, Runners): every
// vector of conformance/vectors runs twice.
//
//   - json:<id>: documents, inputs and results cross as JSON text. Loads
//     and Dumps read and write documents in JSON, YAML, the rule syntax and
//     CSV (DecisionTable.RuleSet for the compiled rule set),
//     RuleSet.ReadDecision reads decisions, the package's unexported JSON
//     entry points (jsonentry.go, as Python's private _evaluate) evaluate
//     inputs given as JSON text, and Decision.JSON, Explanation.JSON,
//     Summary.JSON and the texts are compared exactly. Decisions are also
//     read with their methods (Results, Selected, Fired, Outputs, Output,
//     Aggregated).
//   - native:<id>: the idiomatic API with Go data. Conditions, rules and rule
//     sets are built in code (Field, Const, the methods of Expr, NewRule and
//     its options, NewRuleSet) and checked against the core's reading of the
//     same JSON; inputs are Go data (conformance_values_test.go: the integer
//     types, big.Int, float32 and float64, Decimal, Date, DateTime,
//     time.Time in each kind of location, slices, arrays, maps and structs,
//     named types and pointers, in turn); results are compared as the Go
//     values given back (spec/data-model.md, Go, Values given back), checking
//     each Go type, through Decision, Explanation, Summary and ResultTree,
//     and as their JSON forms. read vectors go through Loads, Dumps,
//     RuleFromText, NewDecisionTable (grids) and RuleSet.ReadDecision.
//
// The runner only drives the package and compares: results are parsed with
// its own JSON reader and compared with the vectors as trees, never with the
// package's own equality. Every failure is reported with its route and id,
// what came back and what the vector expects; the vectors a route cannot run
// are counted and listed with their reason, in the report (logged, and
// written to build/reports/conformance.txt).

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strings"
	"testing"
)

// minimumVectors is the count when this runner was written: vectors are only
// ever added.
const minimumVectors = 2534

// conformanceRoutes are the two ways each vector runs.
var conformanceRoutes = []string{"json", "native"}

// conformanceSkips says why a route does not run a vector.
var conformanceSkips = map[string]string{
	"grid": "grids are Go data: the JSON route reads documents only, so table_grid vectors run in the native" +
		" route (NewDecisionTable)",
	"native": "max_values bounds what is read from native data only, so evaluate vectors with limits run in the" +
		" native route",
	"zero_limit": "a limit of 0, which Go cannot give: 0 in a field of Limits keeps the default" +
		" (spec/json-format.md, Errors and limits), and a limit of 0 refuses every input",
	"zone_unknown_to_go": "a datetime in a zone that Go's zone database does not know, which no time.Time holds",
	"graded": "conditions under a graded logic: the binding evaluates one node under boolean only until task" +
		" J.1b",
}

// allowedSkips are the skips each route may have; any other fails.
var allowedSkips = map[string][]string{
	"json":   {"grid", "native", "graded"},
	"native": {"zero_limit", "zone_unknown_to_go", "graded"},
}

// conformance runs vectors; it counts the Go forms of the native route.
type conformance struct {
	turn    int
	forms   map[string]map[string]bool
	current string
}

func newConformance() *conformance { return &conformance{forms: map[string]map[string]bool{}} }

// vectorFile is a file of vectors, by its path from the vectors' root.
type vectorFile struct {
	name    string
	vectors []*jsonObject
}

// vectorFiles reads every vector file, in the order of their paths.
func vectorFiles(t *testing.T) []vectorFile {
	t.Helper()
	root := filepath.Join("..", "..", "conformance", "vectors")
	var files []vectorFile
	err := filepath.WalkDir(root, func(path string, entry os.DirEntry, err error) error {
		if err != nil || entry.IsDir() || !strings.HasSuffix(path, ".json") {
			return err
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		document, err := parseJSONText(string(data))
		if err != nil {
			return fmt.Errorf("%s: %w", path, err)
		}
		name, _ := filepath.Rel(root, path)
		file := vectorFile{name: filepath.ToSlash(name)}
		for _, item := range document.(*jsonObject).list("vectors") {
			file.vectors = append(file.vectors, item.(*jsonObject))
		}
		files = append(files, file)
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	sort.Slice(files, func(i, j int) bool { return files[i].name < files[j].name })
	return files
}

// run runs a vector one way: "" when it passed, else its failure, or the
// reason it was skipped.
func (c *conformance) run(vector *jsonObject, route string) (failure, skip string) {
	c.current = route + ":" + vector.str("id")
	defer func() {
		switch r := recover().(type) {
		case nil:
		case vectorSkip:
			failure, skip = "", r.reason
		case vectorFailure:
			failure = r.message
		default:
			buf := make([]byte, 4096)
			failure = fmt.Sprintf("panic: %v\n%s", r, buf[:runtime.Stack(buf, false)])
		}
	}()
	given, expect := vector.obj("given"), vector.obj("expect")
	if given == nil || expect == nil {
		failf("no given or expect")
	}
	if isGraded(given) {
		return "", "graded"
	}
	if route == "json" {
		return c.runJSON(given, expect), ""
	}
	return c.runNative(given, expect), ""
}

// isGraded says whether a vector evaluates a condition under a graded logic,
// which the binding does not do until task J.1b.
func isGraded(given *jsonObject) bool {
	return given.has("logic") && given.get("logic") != "boolean"
}

// ---- Reading the vectors.

// need is a member that must be there.
func need(o *jsonObject, key string) any {
	if !o.has(key) {
		failf("no %q in %s", key, writeJSON(o))
	}
	return o.get(key)
}

// content is a member of a document the package wrote, such as its
// decision.
func content(text, key string) any {
	document, err := parseJSONText(text)
	if err != nil {
		failf("the package wrote %q, which is not JSON: %v", text, err)
	}
	o, ok := document.(*jsonObject)
	if !ok || !o.has("version") || !o.has(key) {
		failf("the package wrote %s, not a document with %q", text, key)
	}
	return o.get(key)
}

func parsed(text string) any {
	v, err := parseJSONText(text)
	if err != nil {
		failf("the package wrote %q, which is not JSON: %v", text, err)
	}
	return v
}

func rulesetDocument(given *jsonObject) string {
	return `{"version":1,"ruleset":` + writeJSON(need(given, "ruleset")) + "}"
}

// vectorLimits are a vector's limits, and whether it has any; 0 is a limit Go
// cannot give.
func vectorLimits(given *jsonObject) (Limits, bool) {
	o := given.obj("limits")
	if o == nil {
		return Limits{}, false
	}
	var limits Limits
	for _, key := range o.keys {
		n, err := o.values[key].(interface{ Int64() (int64, error) }).Int64()
		if err != nil {
			failf("the limit %s", key)
		}
		if n == 0 {
			panic(vectorSkip{"zero_limit"})
		}
		field := map[string]*int{
			"max_size": &limits.MaxSize, "max_nesting": &limits.MaxNesting, "max_depth": &limits.MaxDepth,
			"max_table_size": &limits.MaxTableSize, "max_values": &limits.MaxValues,
		}[key]
		if field == nil {
			failf("the unknown limit %s", key)
		}
		*field = int(n)
	}
	return limits, true
}

// source is the text a read vector reads, and its format.
func source(given *jsonObject) (string, Format) {
	switch {
	case given.has("text"):
		return given.str("text"), FormatJSON
	case given.has("document"):
		return writeJSON(given.get("document")), FormatJSON
	case given.has("yaml"):
		return given.str("yaml"), FormatYAML
	case given.has("rules_text"):
		return given.str("rules_text"), FormatText
	case given.has("table_csv"):
		return given.str("table_csv"), FormatCSV
	}
	failf("nothing to read")
	return "", ""
}

// ---- Comparing.

// errorContext are the context fields of an error, in the order of its JSON
// form (spec/errors.md).
var errorContext = []string{"pointer", "line", "column", "rule", "rules", "path", "types", "row", "name", "executed"}

// evaluationFields are the fields compared for an error of an evaluation
// (spec/errors.md, Comparing errors).
var evaluationFields = []string{"code", "rule", "path", "types", "rules", "row", "name"}

// sentinels are the sentinel of each code and of its ancestors
// (spec/errors.md, Hierarchy).
var sentinels = map[Code][]error{
	CodeFormat:             {ErrFormat},
	CodeUnsupportedVersion: {ErrUnsupportedVersion, ErrFormat},
	CodeLimitExceeded:      {ErrLimitExceeded, ErrFormat},
	CodeDecisionMismatch:   {ErrDecisionMismatch, ErrFormat},
	CodeExpression:         {ErrExpression},
	CodeRuleDefinition:     {ErrRuleDefinition},
	CodeRuleSyntax:         {ErrRuleSyntax},
	CodeRegistry:           {ErrRegistry},
	CodeRuleNotFound:       {ErrRuleNotFound},
	CodeEvaluation:         {ErrEvaluation},
	CodeMissingField:       {ErrMissingField, ErrEvaluation},
	CodeTypeMismatch:       {ErrTypeMismatch, ErrEvaluation},
	CodeHitPolicy:          {ErrHitPolicy, ErrEvaluation},
	CodeAction:             {ErrAction},
	CodeUnsupported:        {ErrUnsupported},
}

// errorOfCode is an *Error of a code of spec/errors.md, matching the
// sentinels of its code and its ancestors only.
func errorOfCode(err error) *Error {
	var e *Error
	if !errors.As(err, &e) || e != err {
		failf("an error that is not an *Error: %T %v", err, err)
	}
	want, ok := sentinels[e.Code]
	if !ok {
		failf("an error with the code %s, not one of spec/errors.md: %s", e.Code, e.JSON)
	}
	for _, all := range sentinels {
		s := all[0]
		if errors.Is(e, s) != slices.Contains(want, s) {
			failf("errors.Is(%s error, %v) is %v", e.Code, s, errors.Is(e, s))
		}
	}
	return e
}

// jsonForm is an error's code and context, from its JSON form.
func jsonForm(err error) *jsonObject {
	e := errorOfCode(err)
	written, ok := parsed(e.JSON).(*jsonObject)
	if !ok || written.get("code") != string(e.Code) {
		failf("Code %s, but the JSON form %s", e.Code, e.JSON)
	}
	form := newObject().set("code", written.get("code"))
	for _, key := range errorContext {
		if written.has(key) {
			form.set(key, written.get(key))
		}
	}
	return form
}

// contextForm is an error's code and context, from its Go fields; they must
// say what its JSON form says.
func contextForm(err error) *jsonObject {
	e := errorOfCode(err)
	form := newObject().set("code", string(e.Code))
	if e.Pointer != nil {
		form.set("pointer", *e.Pointer)
	}
	if e.Line != 0 || e.Column != 0 {
		form.set("line", intTree(int64(e.Line))).set("column", intTree(int64(e.Column)))
	}
	if e.Rule != "" {
		form.set("rule", e.Rule)
	}
	if len(e.Rules) > 0 {
		form.set("rules", namesTree(e.Rules))
	}
	if e.Path != nil {
		form.set("path", pathJSON(e.Path))
	}
	if len(e.Types) > 0 {
		form.set("types", namesTree(e.Types))
	}
	if e.Row != -1 {
		form.set("row", intTree(int64(e.Row)))
	}
	if e.Name != "" {
		form.set("name", e.Name)
	}
	if len(e.Executed) > 0 {
		form.set("executed", namesTree(e.Executed))
	}
	if json := jsonForm(err); !sameJSON(form, json, modeExact) {
		failf("the error's fields say %s, its JSON form %s", writeJSON(form), writeJSON(json))
	}
	return form
}

// only keeps the members of o named in keys, in o's order.
func only(o *jsonObject, keys []string) *jsonObject {
	kept := newObject()
	for _, key := range o.keys {
		if slices.Contains(keys, key) {
			kept.set(key, o.values[key])
		}
	}
	return kept
}

// sameMembers compares two objects without regard to member order.
func sameMembers(actual, wanted *jsonObject) bool {
	if len(actual.keys) != len(wanted.keys) {
		return false
	}
	for _, key := range wanted.keys {
		if !actual.has(key) || !sameJSON(actual.values[key], wanted.values[key], modeExact) {
			return false
		}
	}
	return true
}

// compareError compares an error with the vector's (spec/errors.md,
// Comparing errors): for a read, the fields the vector gives; for an
// evaluation, those an evaluation error may carry.
func compareError(actual, wanted *jsonObject, read bool) string {
	keys := evaluationFields
	if read {
		keys = wanted.keys
	}
	got, want := only(actual, keys), only(wanted, keys)
	if sameMembers(got, want) {
		return ""
	}
	return "error " + writeJSON(got) + ", expected " + writeJSON(want)
}

// how says how an outcome is compared: whose errors (their JSON form or
// their Go fields), for a read or not, in which mode.
type how struct {
	read   bool
	native bool
	mode   compareMode
}

var (
	jsonHow       = how{}
	jsonReadHow   = how{read: true}
	nativeHow     = how{native: true}
	nativeReadHow = how{read: true, native: true}
	givenHow      = how{native: true, mode: modeGiven}
)

// compareOutcome compares what a step gave, or its error, with the vector's key or
// its error.
func compareOutcome(result any, err error, expect *jsonObject, key string, h how) string {
	wanted := expect.obj("error")
	switch {
	case err == nil && wanted == nil:
		want := need(expect, key)
		if sameJSON(result, want, h.mode) {
			return ""
		}
		return "\n  got  " + writeJSON(result) + "\n  want " + writeJSON(want)
	case err == nil:
		return "got " + writeJSON(result) + ", expected the error " + writeJSON(wanted)
	}
	form := jsonForm
	if h.native {
		form = contextForm
	}
	if wanted == nil {
		return "unexpected error " + writeJSON(form(err)) + ": " + err.Error()
	}
	return compareError(form(err), wanted, h.read)
}

func compareText(what, got string, want any) string {
	if got == want {
		return ""
	}
	return fmt.Sprintf("%s %q, expected %q", what, got, want)
}

func join(issues ...string) string {
	return strings.Join(slices.DeleteFunc(issues, func(s string) bool { return s == "" }), "; ")
}

// ---- The JSON route.

// loadRuleSet is the rule set the core reads from a vector's ruleset.
func loadRuleSet(given *jsonObject) *RuleSet {
	document, err := Loads(rulesetDocument(given))
	if err != nil {
		failf("reading the rule set: %v", err)
	}
	rs, ok := document.(*RuleSet)
	if !ok {
		failf("Loads of a rule set gave a %T", document)
	}
	return rs
}

func (c *conformance) runJSON(given, expect *jsonObject) string {
	kind := given.str("kind")
	strict := given.get("strict") == true
	switch kind {
	case "read":
		return c.readJSON(given, expect)
	case "condition", "value":
		node, err := readNode(writeJSON(need(given, kind)))
		if err != nil {
			failf("reading the node: %v", err)
		}
		input := writeJSON(need(given, "input"))
		var text string
		if kind == "value" {
			text, err = node.evaluateValueTreeJSON(input, strict)
		} else {
			text, err = node.evaluateTreeJSON(input, given.str("logic"), strict)
		}
		var result any
		if err == nil {
			result = parsed(text)
		}
		return compareOutcome(result, err, expect, "result", jsonHow)
	case "summary":
		rs := loadRuleSet(given)
		s, err := rs.Summary()
		var result any
		if err == nil {
			result = content(s.JSON(), "summary")
		}
		issue := compareOutcome(result, err, expect, "summary", jsonHow)
		if issue != "" || !expect.has("text") {
			return issue
		}
		return compareText("summarised", s.String(), expect.get("text"))
	case "evaluate", "explain":
		if given.has("limits") {
			panic(vectorSkip{"native"})
		}
		return c.evaluateJSON(loadRuleSet(given), kind, given, expect)
	}
	failf("unknown kind %q", kind)
	return ""
}

func (c *conformance) evaluateJSON(rs *RuleSet, kind string, given, expect *jsonObject) string {
	if given.has("inputs") {
		decisions, err := rs.evaluateManyJSON(writeJSON(given.get("inputs")))
		var written []any
		for _, d := range decisions {
			text, err := d.JSON()
			if err != nil {
				failf("Decision.JSON: %v", err)
			}
			written = append(written, content(text, "decision"))
		}
		if err == nil && written == nil {
			written = []any{}
		}
		if issue := compareOutcome(written, err, expect, "decisions", jsonHow); issue != "" || err != nil {
			return issue
		}
		for i, want := range expect.list("decisions") {
			if issue := checkDecision(rs, decisions[i], want.(*jsonObject), modeGiven); issue != "" {
				return fmt.Sprintf("decision %d: %s", i, issue)
			}
		}
		return ""
	}
	d, err := rs.evaluateJSON(writeJSON(need(given, "input")))
	var written any
	if err == nil {
		written = content(must2(d.JSON()), "decision")
	}
	if issue := compareOutcome(written, err, expect, "decision", jsonHow); issue != "" || err != nil {
		return issue
	}
	if issue := checkDecision(rs, d, expect.obj("decision"), modeGiven); issue != "" || kind == "evaluate" {
		return issue
	}
	e, err := d.Explain()
	var explained any
	if err == nil {
		explained = content(e.JSON(), "explanation")
	}
	if issue := compareOutcome(explained, err, expect, "explanation", jsonHow); issue != "" || !expect.has("text") {
		return issue
	}
	return compareText("explained", e.String(), expect.get("text"))
}

// must2 is the value of a step that cannot fail here.
func must2[T any](v T, err error) T {
	if err != nil {
		failf("unexpected error: %v", err)
	}
	return v
}

// documentKey is the content key of the document a Document is written in.
func documentKey(doc Document) string {
	switch doc.(type) {
	case *Expr:
		return "condition"
	case *Rule:
		return "rule"
	case *RuleSet:
		return "ruleset"
	case *DecisionTable:
		return "decision_table"
	}
	return fmt.Sprintf("%T", doc)
}

func (c *conformance) readJSON(given, expect *jsonObject) string {
	if given.has("table_grid") {
		panic(vectorSkip{"grid"})
	}
	text, format := source(given)
	limits, limited := vectorLimits(given)
	var options []LoadOption
	if format != FormatJSON || c.pick(2) == 0 {
		options = append(options, WithFormat(format))
	}
	if limited {
		options = append(options, WithLimits(limits))
	}
	if given.has("ruleset") {
		rs := loadRuleSet(given)
		readOptions := []ReadOption{WithFormat(format)}
		if limited {
			readOptions = append(readOptions, WithLimits(limits))
		}
		d, err := rs.ReadDecision(text, readOptions...)
		var written any
		if err == nil {
			written = parsed(must2(d.JSON()))
		}
		if issue := compareOutcome(written, err, expect, "document", jsonReadHow); issue != "" || err != nil {
			return issue
		}
		if issue := checkDecision(rs, d, need(expect, "document").(*jsonObject).obj("decision"), modeRecorded); issue != "" {
			return "the decision read: " + issue
		}
		return ""
	}
	doc, err := Loads(text, options...)
	if err != nil {
		return compareOutcome(nil, err, expect, "document", jsonReadHow)
	}
	return c.written(doc, given, expect, jsonReadHow)
}

// written compares what a read document writes with the vector: its
// document, its text, its CSV, its compiled rule set, or the error of
// writing it in the form the vector names.
func (c *conformance) written(doc Document, given, expect *jsonObject, h how) string {
	if expect.has("error") {
		target, ok := given.get("write").(string)
		if !ok {
			return fmt.Sprintf("read a %T, expected the error %s", doc, writeJSON(expect.get("error")))
		}
		_, err := Dumps(doc, WithFormat(Format(target)))
		return compareOutcome(nil, err, expect, "document", h)
	}
	var issues []string
	if wanted := expect.obj("document"); wanted != nil {
		text, err := Dumps(doc)
		var result any
		if err == nil {
			result = parsed(text)
		}
		issues = append(issues, compareOutcome(result, err, expect, "document", h))
		if key := documentKey(doc); !wanted.has(key) {
			issues = append(issues, fmt.Sprintf("read a %T, expected the document %s", doc, writeJSON(wanted)))
		}
	}
	if expect.has("printed") {
		issues = append(issues, compareText("printed", must2(Dumps(doc, WithFormat(FormatText))), expect.get("printed")))
	}
	if expect.has("csv") {
		issues = append(issues, compareText("wrote the CSV", must2(Dumps(doc, WithFormat(FormatCSV))), expect.get("csv")))
	}
	if expect.has("compiled") {
		table, ok := doc.(*DecisionTable)
		if !ok {
			return fmt.Sprintf("read a %T, expected a decision table", doc)
		}
		rs, err := table.RuleSet()
		var result any
		if err == nil {
			result = content(must2(Dumps(rs)), "ruleset")
		}
		issues = append(issues, compareOutcome(result, err, expect, "compiled", h))
	}
	return join(issues...)
}

// ---- Building in code.

// build builds a condition or value node of the format with the Go API,
// through its functions and methods in turn.
func (c *conformance) build(v any) *Expr {
	node, ok := v.(*jsonObject)
	if !ok {
		failf("not a node: %s", writeJSON(v))
	}
	op := node.str("op")
	sub := func(key string) *Expr { return c.build(need(node, key)) }
	// operand is a node, or the Go value of a constant, in turn.
	operand := func(key string) any {
		child := need(node, key).(*jsonObject)
		if child.str("op") == "const" && c.pick(2) == 0 {
			return c.toGo(need(child, "value"))
		}
		return c.build(child)
	}
	switch op {
	case "field":
		return Field(pathFrom(need(node, "path"))...)
	case "const":
		return Const(c.toGo(need(node, "value")))
	case "eq", "ne", "lt", "le", "gt", "ge", "add", "sub", "mul", "div":
		left := sub("left")
		right := operand("right")
		return map[string]func(any) *Expr{
			"eq": left.Eq, "ne": left.Ne, "lt": left.Lt, "le": left.Le, "gt": left.Gt, "ge": left.Ge,
			"add": left.Add, "sub": left.Sub, "mul": left.Mul, "div": left.Div,
		}[op](right)
	case "and", "or":
		var operands []*Expr
		for _, item := range node.list("operands") {
			operands = append(operands, c.build(item))
		}
		method := len(operands) > 0 && c.pick(2) == 0
		switch {
		case op == "and" && method:
			return operands[0].And(operands[1:]...)
		case op == "and":
			return And(operands...)
		case method:
			return operands[0].Or(operands[1:]...)
		}
		return Or(operands...)
	case "not":
		if c.pick(2) == 0 {
			return sub("operand").Not()
		}
		return Not(sub("operand"))
	case "neg":
		return sub("operand").Neg()
	case "abs":
		return sub("operand").Abs()
	case "is_null":
		return sub("operand").IsNull()
	case "in":
		var values []any
		for _, item := range node.list("values") {
			values = append(values, c.toGo(item))
		}
		x := sub("operand")
		if values == nil {
			return x.IsIn()
		}
		// One value that is a slice or an array would be taken as the list.
		if len(values) == 1 || c.pick(2) == 0 {
			c.used("IsIn with a slice")
			return x.IsIn(values)
		}
		return x.IsIn(values...)
	case "between":
		x := sub("operand")
		low, high := operand("low"), operand("high")
		closed, ok := node.get("closed").(string)
		if !ok && c.pick(2) == 0 {
			return x.Between(low, high)
		}
		if !ok {
			closed = string(ClosedBoth)
		}
		return x.BetweenClosed(low, high, Closed(closed))
	case "matches":
		return sub("operand").Matches(node.str("pattern"))
	case "starts_with":
		return sub("operand").StartsWith(node.str("text"))
	case "ends_with":
		return sub("operand").EndsWith(node.str("text"))
	case "contains":
		return sub("operand").Contains(node.str("text"))
	}
	failf("unknown op %q", op)
	return nil
}

// buildRuleSet builds a rule set of the format with NewRule and its options
// and NewRuleSet.
func (c *conformance) buildRuleSet(v any) *RuleSet {
	o := v.(*jsonObject)
	var rules []*Rule
	for _, item := range o.list("rules") {
		r := item.(*jsonObject)
		var options []RuleOption
		if then := r.obj("then"); then != nil {
			for _, key := range then.keys {
				value := then.values[key].(*jsonObject)
				if value.str("op") == "const" && c.pick(2) == 0 {
					options = append(options, Then(key, c.toGo(value.get("value"))))
				} else {
					options = append(options, Then(key, c.build(value)))
				}
			}
		}
		if r.has("priority") {
			n, err := r.get("priority").(interface{ Int64() (int64, error) }).Int64()
			if err != nil {
				failf("the priority %v", r.get("priority"))
			}
			options = append(options, WithPriority(n))
		}
		if tags := r.list("tags"); tags != nil {
			var names []string
			for _, tag := range tags {
				names = append(names, tag.(string))
			}
			options = append(options, WithTags(names...))
		}
		if r.has("description") {
			options = append(options, WithDescription(r.str("description")))
		}
		if metadata := r.obj("metadata"); metadata != nil {
			m := map[string]any{}
			for _, key := range metadata.keys {
				m[key] = c.toGo(metadata.values[key])
			}
			options = append(options, WithMetadata(m))
		}
		if r.has("do") {
			options = append(options, WithAction(r.str("do")))
		}
		when := c.build(need(r, "when"))
		var built *Rule
		var err error
		if len(options) > 0 && c.pick(2) == 0 {
			// The rule, then its options with With.
			built, err = NewRule(r.str("name"), when)
			if err == nil {
				built, err = built.With(options...)
			}
		} else {
			built, err = NewRule(r.str("name"), when, options...)
		}
		if err != nil {
			failf("building the rule %s: %v", r.str("name"), err)
		}
		rules = append(rules, built)
	}
	var options []RuleSetOption
	if p := o.str("hit_policy"); p != "" && (p != "first" || c.pick(2) == 0) {
		options = append(options, WithHitPolicy(HitPolicy(p)))
	}
	if a := o.str("aggregate"); a != "" {
		options = append(options, WithAggregate(Aggregate(a)))
	}
	if l := o.str("logic"); l != "" && (l != "boolean" || c.pick(2) == 0) {
		options = append(options, WithLogic(l))
	}
	if strict := o.get("strict") == true; strict || c.pick(4) == 0 {
		options = append(options, WithStrict(strict))
	}
	rs, err := NewRuleSet(rules, options...)
	if err != nil {
		failf("building the rule set: %v", err)
	}
	return rs
}

// checkBuilt checks that a rule set built in code is the one the core reads
// from the vector (the same document), that its accessors say what it
// holds, and that what Loads reads of it writes the same document.
func checkBuilt(rs *RuleSet, given *jsonObject) string {
	built := content(must2(Dumps(rs)), "ruleset")
	core := loadRuleSet(given)
	read := content(must2(Dumps(core)), "ruleset").(*jsonObject)
	if !sameJSON(built, read, modeExact) {
		return "the rule set built in code writes\n  " + writeJSON(built) + "\nthe core reads\n  " + writeJSON(read)
	}
	if string(rs.HitPolicy()) != read.str("hit_policy") || rs.Logic() != read.str("logic") ||
		rs.Strict() != (read.get("strict") == true) || string(rs.Aggregate()) != read.str("aggregate") ||
		rs.Len() != len(read.list("rules")) {
		return fmt.Sprintf("the rule set built in code says %s %s %v %q, %d rules", rs.HitPolicy(), rs.Logic(), rs.Strict(), rs.Aggregate(), rs.Len())
	}
	again := content(must2(Dumps(must2(Loads(must2(Dumps(rs)))))), "ruleset")
	if !sameJSON(again, read, modeExact) {
		return "Loads(Dumps(rule set)) writes " + writeJSON(again)
	}
	return join(checkRules(rs.Rules(), read.list("rules"))...)
}

// checkRules checks each rule's accessors against its document.
func checkRules(rules []*Rule, sources []any) []string {
	if len(rules) != len(sources) {
		return []string{fmt.Sprintf("%d rules, expected %d", len(rules), len(sources))}
	}
	var issues []string
	for i, r := range rules {
		issues = append(issues, checkRule(r, sources[i].(*jsonObject)))
	}
	return issues
}

// checkRule checks what a rule's accessors say against its document.
func checkRule(r *Rule, source *jsonObject) string {
	name := source.str("name")
	switch {
	case r.Name() != name:
		return "Name() " + r.Name() + ", expected " + name
	case !sameJSON(intTree(r.Priority()), orZero(source.get("priority")), modeExact):
		return fmt.Sprintf("%s: Priority() %d", name, r.Priority())
	case r.ActionName() != source.str("do"):
		return fmt.Sprintf("%s: ActionName() %q", name, r.ActionName())
	case r.Description() != source.str("description"):
		return fmt.Sprintf("%s: Description() %q", name, r.Description())
	case !slices.Equal(r.Tags(), stringsOf(source.list("tags"))):
		return fmt.Sprintf("%s: Tags() %q", name, r.Tags())
	}
	// Metadata's keys are literal, even those that start with $
	// (spec/json-format.md, Rules), and Go gives them in a map.
	metadata := givenMap(r.Metadata())
	wanted := source.obj("metadata")
	if wanted == nil {
		wanted = newObject()
	}
	if !sameJSON(metadata, wanted, modeGiven) {
		return fmt.Sprintf("%s: Metadata() %s, expected %s", name, writeJSON(metadata), writeJSON(wanted))
	}
	if when := content(must2(Dumps(r.When())), "condition"); !sameJSON(when, source.get("when"), modeExact) {
		return fmt.Sprintf("%s: When() writes %s", name, writeJSON(when))
	}
	then := source.obj("then")
	if then == nil {
		then = newObject()
	}
	payload := r.Payload()
	if len(payload) != len(then.keys) {
		return fmt.Sprintf("%s: Payload() has %d entries", name, len(payload))
	}
	for i, entry := range payload {
		if entry.Key != then.keys[i] {
			return fmt.Sprintf("%s: Payload()[%d] is %q", name, i, entry.Key)
		}
		if value := valueNode(entry.Value); !sameJSON(value, then.values[entry.Key], modeExact) {
			return fmt.Sprintf("%s: Payload()[%d] writes %s", name, i, writeJSON(value))
		}
	}
	return ""
}

func orZero(v any) any {
	if v == nil {
		return intTree(0)
	}
	return v
}

func stringsOf(list []any) []string {
	var out []string
	for _, item := range list {
		out = append(out, item.(string))
	}
	return out
}

// valueNode is a node in its JSON form, as the core writes it in a rule's
// payload (Dumps writes a condition only).
func valueNode(e *Expr) any {
	r, err := NewRule("n", Const(true), Then("v", e))
	if err != nil {
		failf("a rule with the payload %v: %v", e, err)
	}
	return content(must2(Dumps(r)), "rule").(*jsonObject).obj("then").get("v")
}

// coreValueNode is the core's reading of a value node of the format.
func coreValueNode(node any) any {
	document := `{"version":1,"rule":{"name":"n","when":{"op":"const","value":true},"then":{"v":` + writeJSON(node) + "}}}"
	doc, err := Loads(document)
	if err != nil {
		failf("the core reads the node %s: %v", writeJSON(node), err)
	}
	return content(must2(Dumps(doc)), "rule").(*jsonObject).obj("then").get("v")
}

// checkBuiltNode checks a node built in code against the core's reading of
// its JSON.
func checkBuiltNode(e *Expr, kind string, node any) string {
	if err := e.Err(); err != nil {
		failf("building the node: %v", err)
	}
	var built, read any
	if kind == "condition" {
		built = content(must2(Dumps(e)), "condition")
		read = content(must2(Dumps(must2(Loads(`{"version":1,"condition":`+writeJSON(node)+"}")))), "condition")
	} else {
		built, read = valueNode(e), coreValueNode(node)
	}
	if !sameJSON(built, read, modeExact) {
		return "the node built in code writes\n  " + writeJSON(built) + "\nthe core reads\n  " + writeJSON(read)
	}
	if e.Tag() != read.(*jsonObject).str("op") {
		return "Tag() " + e.Tag()
	}
	core := must2(readNode(writeJSON(node)))
	if e.Kind() != core.Kind() {
		return fmt.Sprintf("Kind() %s, the core's %s", e.Kind(), core.Kind())
	}
	return ""
}

// ---- Values given back, as values of the format.

func truthTree(t Truth) any {
	switch t {
	case TruthTrue:
		return true
	case TruthFalse:
		return false
	}
	return nil
}

func truthOfJSON(v any) Truth {
	switch v {
	case true:
		return TruthTrue
	case false:
		return TruthFalse
	}
	return TruthUnknown
}

func reasonTree(r *Reason) *jsonObject {
	out := newObject().set("code", r.Code)
	if len(r.Paths) > 0 {
		paths := make([]any, len(r.Paths))
		for i, path := range r.Paths {
			paths[i] = pathJSON(path)
		}
		out.set("paths", paths)
	}
	if len(r.Types) > 0 {
		out.set("types", namesTree(r.Types))
	}
	return out
}

// resultTree is a ResultTree in the JSON format (spec/json-format.md,
// Result trees).
func resultTree(t *ResultTree) *jsonObject {
	out := newObject().set("op", t.Tag)
	if node := t.Node(); node.Tag() != t.Tag {
		failf("a result whose Tag is %s, and whose Node's %s", t.Tag, node.Tag())
	}
	if t.Tag == "field" {
		out.set("path", pathJSON(t.Path))
	} else if t.Path != nil {
		failf("a %s result with a path", t.Tag)
	}
	if t.Truth != nil {
		out.set("truth", truthTree(*t.Truth))
		if t.Value != nil {
			failf("a result with a truth value has the value %v", t.Value)
		}
	} else {
		out.set("value", given(t.Value))
	}
	if t.Reason != nil {
		out.set("reason", reasonTree(t.Reason))
	}
	if len(t.Children) > 0 {
		children := make([]any, len(t.Children))
		for i, child := range t.Children {
			children[i] = resultTree(child)
		}
		out.set("children", children)
	}
	return out
}

func gapTree(gap any) any {
	switch g := gap.(type) {
	case GapDays:
		return newObject().set("days", intTree(int64(g)))
	case GapMicroseconds:
		return newObject().set("microseconds", intTree(int64(g)))
	case int64, float64, Decimal:
		return given(g)
	}
	failf("a gap given back as a %T", gap)
	return nil
}

func requirementTree(r Requirement) any {
	switch r := r.(type) {
	case *CauseGroup:
		items := make([]any, len(r.Items))
		for i, item := range r.Items {
			items[i] = requirementTree(item)
		}
		return newObject().set(string(r.Kind), items)
	case *Cause:
		holds := (r.Truth == TruthTrue && !r.Negated) || (r.Truth == TruthFalse && r.Negated)
		if r.Holds != holds {
			failf("a cause %s, negated %v, says Holds %v", r.Truth, r.Negated, r.Holds)
		}
		out := newObject().set("condition", parsed(r.Condition)).set("at", r.At).
			set("truth", truthTree(r.Truth)).set("negated", r.Negated)
		values := make([]any, len(r.Values))
		for i, v := range r.Values {
			values[i] = newObject().set("path", pathJSON(v.Path)).set("value", given(v.Value))
		}
		out.set("values", values)
		if r.Reason != nil {
			out.set("reason", reasonTree(r.Reason))
		}
		if cf := r.Counterfactual; cf != nil {
			counterfactual := newObject().set("path", pathJSON(cf.Path)).set("op", cf.Op).set("target", given(cf.Target))
			if cf.Gap != nil {
				counterfactual.set("gap", gapTree(cf.Gap))
			}
			out.set("counterfactual", counterfactual)
		}
		if node := must2(r.Node()); !sameJSON(content(must2(Dumps(node)), "condition"), parsed(r.Condition), modeExact) {
			failf("Node() of the cause %s writes another condition", r.Condition)
		}
		return out
	}
	failf("a requirement %T", r)
	return nil
}

// leaves are the causes of a requirement, in order.
func leaves(r Requirement, out []*Cause) []*Cause {
	switch r := r.(type) {
	case *Cause:
		return append(out, r)
	case *CauseGroup:
		for _, item := range r.Items {
			out = leaves(item, out)
		}
	}
	return out
}

// explanationTree is an Explanation in the JSON format (spec/json-format.md,
// Explanations), from its fields.
func explanationTree(d *Decision, e *Explanation) *jsonObject {
	rules := make([]any, len(e.Rules))
	for i, r := range e.Rules {
		if found, err := e.ForRule(r.Rule); err != nil || found != r {
			failf("ForRule(%q) is not Rules[%d]", r.Rule, i)
		}
		if found, err := d.WhyNot(r.Rule); err != nil || found != r {
			failf("WhyNot(%q) is not Rules[%d]", r.Rule, i)
		}
		out := newObject().set("rule", r.Rule).set("status", string(r.Status))
		if n := r.Note; n != nil {
			note := newObject().set("kind", string(n.Kind)).set("winner", n.Winner)
			if (n.Priority == nil) != (n.WinnerPriority == nil) {
				failf("a note with one priority: %+v", n)
			}
			if n.Priority != nil {
				note.set("priority", intTree(*n.Priority)).set("winner_priority", intTree(*n.WinnerPriority))
			}
			out.set("note", note)
		}
		if r.Achievable != nil {
			out.set("achievable", *r.Achievable)
		}
		out.set("requirement", requirementTree(r.Requirement))
		causes := leaves(r.Requirement, nil)
		if len(causes) == 0 || !slices.Equal(causes, r.Causes()) {
			failf("Causes() of %s are not its requirement's, in order", r.Rule)
		}
		rules[i] = out
	}
	return newObject().set("hit_policy", string(e.HitPolicy)).set("logic", e.Logic).
		set("selected", namesTree(e.Selected)).set("rules", rules)
}

// summaryTree is a Summary in the JSON format (spec/json-format.md,
// Summaries), from its fields.
func summaryTree(s *Summary) *jsonObject {
	out := newObject().set("hit_policy", string(s.HitPolicy))
	if s.Aggregate != "" {
		out.set("aggregate", string(s.Aggregate))
	}
	out.set("logic", s.Logic)
	if s.Strict {
		out.set("strict", true)
	}
	rules := make([]any, len(s.Rules))
	for i, r := range s.Rules {
		item := newObject().set("name", r.Name).set("priority", intTree(r.Priority)).set("outputs", namesTree(r.Outputs))
		if r.Action {
			item.set("action", true)
		}
		if r.ActionName != "" {
			item.set("do", r.ActionName)
		}
		rules[i] = item
	}
	out.set("rules", rules)
	fields := make([]any, len(s.Fields))
	for i, f := range s.Fields {
		payloads := make([]any, len(f.Payloads))
		for j, p := range f.Payloads {
			payloads[j] = newObject().set("rule", p.Rule).set("key", p.Key)
		}
		fields[i] = newObject().set("path", pathJSON(f.Path)).set("conditions", namesTree(f.Conditions)).set("payloads", payloads)
	}
	out.set("fields", fields).set("outputs", namesTree(s.Outputs))
	tags := make([]any, len(s.Tags))
	for i, tag := range s.Tags {
		tags[i] = newObject().set("tag", tag.Tag).set("rules", namesTree(tag.Rules))
	}
	return out.set("tags", tags)
}

// singleHit are the hit policies with one output.
var singleHit = []string{"first", "priority", "unique", "any"}

// outputOf is the payload of an expected result: each key's value, or nil
// when it was not evaluated.
func outputOf(result *jsonObject) *jsonObject {
	then := result.obj("then")
	if then == nil {
		return nil
	}
	output := newObject()
	for _, key := range then.keys {
		output.set(key, need(then.values[key].(*jsonObject), "value"))
	}
	return output
}

// checkDecision checks a Decision's methods against the expected decision,
// comparing the Go values given back in mode.
func checkDecision(rs *RuleSet, d *Decision, expected *jsonObject, mode compareMode) string {
	if d.RuleSet() != rs {
		return "RuleSet() is another rule set"
	}
	if string(d.HitPolicy()) != expected.str("hit_policy") {
		return "HitPolicy() " + string(d.HitPolicy())
	}
	results := expected.list("results")
	got := d.Results()
	rules := rs.Rules()
	if len(got) != len(results) {
		return fmt.Sprintf("%d results, expected %d", len(got), len(results))
	}
	var outputs []*jsonObject
	var fired []string
	for i, item := range results {
		result := item.(*jsonObject)
		at := fmt.Sprintf("Results()[%d]: ", i)
		if got[i].Rule != rules[i] || got[i].Rule.Name() != result.str("rule") {
			return at + "Rule " + got[i].Rule.Name()
		}
		if got[i].Fired != (need(result, "fired") == true) {
			return fmt.Sprintf("%sFired %v", at, got[i].Fired)
		}
		if result.get("fired") == true {
			fired = append(fired, result.str("rule"))
		}
		if truth := truthOfJSON(need(result.obj("when"), "truth")); got[i].Truth != truth {
			return fmt.Sprintf("%sTruth %s, expected %s", at, got[i].Truth, truth)
		}
		output := outputOf(result)
		outputs = append(outputs, output)
		if (output == nil) != (got[i].Output == nil) || output != nil && !sameJSON(given(got[i].Output), output, mode) {
			return fmt.Sprintf("%sOutput %s, expected %s", at, writeJSON(given(got[i].Output)), writeJSON(output))
		}
	}
	var names []any
	var wantOutputs []any
	for _, s := range d.Selected() {
		position := slices.IndexFunc(got, func(r RuleResult) bool { return r.Rule == s.Rule })
		if position < 0 {
			return "Selected() holds a result that is not in Results()"
		}
		names = append(names, s.Rule.Name())
		output := outputs[position]
		if output == nil {
			output = newObject()
		}
		wantOutputs = append(wantOutputs, output)
	}
	if names == nil {
		names = []any{}
	}
	if !sameJSON(names, need(expected, "selected"), modeExact) {
		return "Selected() " + writeJSON(names)
	}
	if !slices.Equal(d.Fired(), fired) {
		return fmt.Sprintf("Fired() %q", d.Fired())
	}
	gotOutputs := []any{}
	for _, output := range d.Outputs() {
		if output == nil {
			return "Outputs() holds nil"
		}
		gotOutputs = append(gotOutputs, given(output))
	}
	if wantOutputs == nil {
		wantOutputs = []any{}
	}
	if !sameJSON(gotOutputs, wantOutputs, mode) {
		return "Outputs() " + writeJSON(gotOutputs) + ", expected " + writeJSON(wantOutputs)
	}
	output, err := d.Output()
	var want any
	switch {
	case expected.has("aggregated"):
		want = expected.get("aggregated")
	case slices.Contains(singleHit, expected.str("hit_policy")):
		if len(wantOutputs) > 0 {
			want = wantOutputs[0]
		}
	default:
		var e *Error
		if !errors.As(err, &e) || e.Code != CodeHitPolicy || len(e.Rules) != 0 || output != nil {
			return fmt.Sprintf("Output() under %s: %v, expected a hit_policy_error", expected.str("hit_policy"), err)
		}
		if d.Aggregated() != nil {
			return "Aggregated() is not nil without an aggregate"
		}
		return ""
	}
	if err != nil {
		return "Output(): " + err.Error()
	}
	if !sameJSON(given(output), want, mode) {
		return "Output() " + writeJSON(given(output)) + ", expected " + writeJSON(want)
	}
	aggregated := given(d.Aggregated())
	if !expected.has("aggregated") {
		want = nil
	}
	if !sameJSON(aggregated, want, mode) {
		return "Aggregated() " + writeJSON(aggregated) + ", expected " + writeJSON(want)
	}
	return ""
}

// checkRecorded checks that a decision's document (JSON) is the expected
// one, and that it reads back with ReadDecision as the same decision.
func checkRecorded(rs *RuleSet, d *Decision, expected *jsonObject) string {
	text := must2(d.JSON())
	if written := content(text, "decision"); !sameJSON(written, expected, modeExact) {
		return "JSON()\n  got  " + writeJSON(written) + "\n  want " + writeJSON(expected)
	}
	again, err := rs.ReadDecision(text)
	if err != nil {
		return "ReadDecision(JSON()): " + err.Error()
	}
	defer again.Close()
	if issue := checkDecision(rs, again, expected, modeRecorded); issue != "" {
		return "ReadDecision(JSON()): " + issue
	}
	return ""
}

// ---- The native route.

func (c *conformance) runNative(given, expect *jsonObject) string {
	kind := given.str("kind")
	switch kind {
	case "read":
		return c.readNative(given, expect)
	case "summary":
		return c.summaryNative(given, expect)
	case "condition", "value":
		return c.treeNative(kind, given, expect)
	case "evaluate", "explain":
		return c.evaluateNative(kind, given, expect)
	}
	failf("unknown kind %q", kind)
	return ""
}

// actionless reports whether no rule of a rule set has an action, so Run
// evaluates as Evaluate does.
func actionless(rs *RuleSet) bool {
	return !slices.ContainsFunc(rs.Rules(), func(r *Rule) bool { return r.ActionName() != "" })
}

func (c *conformance) evaluateNative(kind string, given, expect *jsonObject) string {
	var options []EvaluateOption
	if limits, ok := vectorLimits(given); ok {
		options = append(options, WithLimits(limits))
	} else if c.pick(4) == 0 {
		options = append(options, WithLimits(DefaultLimits()))
	}
	rs := c.buildRuleSet(need(given, "ruleset"))
	if issue := checkBuilt(rs, given); issue != "" {
		return issue
	}
	if given.has("inputs") {
		var rows any
		items := given.list("inputs")
		converted := make([]any, len(items))
		for i, item := range items {
			converted[i] = c.toGo(item)
		}
		rows = converted
		if c.pick(2) == 0 {
			c.used("rows in an array")
			array := reflectArray(converted)
			rows = array
		}
		decisions, err := rs.EvaluateMany(rows, options...)
		if err != nil || expect.has("error") {
			return compareOutcome(nil, err, expect, "decisions", nativeHow)
		}
		wanted := expect.list("decisions")
		if len(decisions) != len(wanted) {
			return fmt.Sprintf("%d decisions, expected %d", len(decisions), len(wanted))
		}
		for i, d := range decisions {
			expected := wanted[i].(*jsonObject)
			if issue := join(checkDecision(rs, d, expected, modeGiven), checkRecorded(rs, d, expected)); issue != "" {
				return fmt.Sprintf("decision %d: %s", i, issue)
			}
		}
		return ""
	}
	input := c.toGo(need(given, "input"))
	evaluate := rs.Evaluate
	if actionless(rs) && c.pick(3) == 0 {
		c.used("RuleSet.Run")
		evaluate = rs.Run
	}
	d, err := evaluate(input, options...)
	if err != nil || expect.has("error") {
		return compareOutcome(nil, err, expect, "decision", nativeHow)
	}
	expected := expect.obj("decision")
	if issue := join(checkDecision(rs, d, expected, modeGiven), checkRecorded(rs, d, expected)); issue != "" || kind == "evaluate" {
		return issue
	}
	e, err := d.Explain()
	var explained any
	if err == nil {
		explained = explanationTree(d, e)
	}
	if issue := compareOutcome(explained, err, expect, "explanation", givenHow); issue != "" {
		return issue
	}
	if issue := compareOutcome(content(e.JSON(), "explanation"), nil, expect, "explanation", nativeHow); issue != "" {
		return "Explanation.JSON(): " + issue
	}
	if expect.has("text") {
		return compareText("explained", e.String(), expect.get("text"))
	}
	return ""
}

func (c *conformance) summaryNative(given, expect *jsonObject) string {
	rs := c.buildRuleSet(need(given, "ruleset"))
	if issue := checkBuilt(rs, given); issue != "" {
		return issue
	}
	s, err := rs.Summary()
	if err != nil {
		return compareOutcome(nil, err, expect, "summary", nativeHow)
	}
	issue := compareOutcome(summaryTree(s), nil, expect, "summary", givenHow)
	if issue == "" {
		issue = compareOutcome(content(s.JSON(), "summary"), nil, expect, "summary", nativeHow)
	}
	if issue != "" || !expect.has("text") {
		return issue
	}
	return compareText("summarised", s.String(), expect.get("text"))
}

func (c *conformance) treeNative(kind string, given, expect *jsonObject) string {
	node := need(given, kind)
	e := c.build(node)
	if issue := checkBuiltNode(e, kind, node); issue != "" {
		return issue
	}
	input := c.toGo(need(given, "input"))
	strict := given.get("strict") == true
	var tree *ResultTree
	var err error
	if kind == "value" {
		var options []ValueTreeOption
		if strict || c.pick(3) == 0 {
			options = append(options, WithStrict(strict))
		}
		tree, err = e.EvaluateValueTree(input, options...)
	} else {
		var options []TreeOption
		if logic := given.str("logic"); logic != "" {
			options = append(options, WithLogic(logic))
		}
		if strict || c.pick(3) == 0 {
			options = append(options, WithStrict(strict))
		}
		tree, err = e.EvaluateTree(input, options...)
	}
	if err != nil {
		return compareOutcome(nil, err, expect, "result", nativeHow)
	}
	if issue := compareOutcome(resultTree(tree), nil, expect, "result", givenHow); issue != "" {
		return issue
	}
	if issue := compareOutcome(parsed(tree.JSON()), nil, expect, "result", nativeHow); issue != "" {
		return "ResultTree.JSON(): " + issue
	}
	return ""
}

func (c *conformance) readNative(given, expect *jsonObject) string {
	limits, limited := vectorLimits(given)
	if given.has("ruleset") {
		text, format := source(given)
		rs := c.buildRuleSet(need(given, "ruleset"))
		if issue := checkBuilt(rs, given); issue != "" {
			return issue
		}
		options := []ReadOption{}
		if format != FormatJSON || c.pick(2) == 0 {
			options = append(options, WithFormat(format))
		}
		if limited {
			options = append(options, WithLimits(limits))
		}
		d, err := rs.ReadDecision(text, options...)
		if err != nil || expect.has("error") {
			return compareOutcome(nil, err, expect, "document", nativeReadHow)
		}
		expected := need(expect, "document").(*jsonObject).obj("decision")
		return join(checkDecision(rs, d, expected, modeRecorded), checkRecorded(rs, d, expected))
	}
	var doc Document
	var err error
	var issues []string
	if grid := given.obj("table_grid"); grid != nil {
		doc, err = c.table(grid, limits, limited)
	} else {
		text, format := source(given)
		var options []LoadOption
		if format != FormatJSON || c.pick(2) == 0 {
			options = append(options, WithFormat(format))
		}
		if limited {
			options = append(options, WithLimits(limits))
		}
		if format == FormatText {
			issues = append(issues, fromText(text, limits, limited, expect))
		}
		doc, err = Loads(text, options...)
	}
	if err != nil {
		return join(append(issues, compareOutcome(nil, err, expect, "document", nativeReadHow))...)
	}
	issues = append(issues, c.written(doc, given, expect, nativeReadHow))
	if wanted := expect.obj("document"); wanted != nil && !expect.has("error") {
		again := parsed(must2(Dumps(must2(Loads(must2(Dumps(doc)))))))
		if !sameJSON(again, wanted, modeExact) {
			issues = append(issues, "Loads(Dumps(x)) writes "+writeJSON(again))
		}
		issues = append(issues, checkContent(doc, wanted))
	}
	return join(issues...)
}

// checkContent checks what a document's objects say against the document.
func checkContent(doc Document, wanted *jsonObject) string {
	switch doc := doc.(type) {
	case *Rule:
		return checkRule(doc, wanted.obj("rule"))
	case *RuleSet:
		return join(checkRules(doc.Rules(), wanted.obj("ruleset").list("rules"))...)
	case *DecisionTable:
		table := wanted.obj("decision_table")
		hitPolicy := table.str("hit_policy")
		if hitPolicy == "" {
			hitPolicy = "first"
		}
		logic := table.str("logic")
		if logic == "" {
			logic = "boolean"
		}
		if string(doc.HitPolicy()) != hitPolicy || string(doc.Aggregate()) != table.str("aggregate") ||
			doc.Logic() != logic || doc.Strict() != (table.get("strict") == true) || doc.Len() != len(table.list("rows")) {
			return fmt.Sprintf("the table says %s %q %s %v, %d rows", doc.HitPolicy(), doc.Aggregate(), doc.Logic(), doc.Strict(), doc.Len())
		}
	}
	return ""
}

// table builds a grid with NewDecisionTable: text cells as text, empty
// cells as nil or a nil pointer, other cells as Go data. (A nil *Expr is
// not an empty cell but an expression_error, whatever spec/tables.md says
// of nil pointers: an open point.)
func (c *conformance) table(grid *jsonObject, limits Limits, limited bool) (*DecisionTable, error) {
	var options []TableOption
	if p := grid.str("hit_policy"); p != "" {
		options = append(options, WithTablePolicy(TablePolicy(p)))
	}
	if a := grid.str("aggregate"); a != "" {
		options = append(options, WithAggregate(Aggregate(a)))
	}
	if l := grid.str("logic"); l != "" {
		options = append(options, WithLogic(l))
	}
	if grid.get("strict") == true {
		options = append(options, WithStrict(true))
	}
	if limited {
		options = append(options, WithLimits(limits))
	}
	cell := func(v any) any {
		switch v := v.(type) {
		case string:
			return v
		case nil:
			if c.pick(2) == 0 {
				c.used("nil *string cell")
				return (*string)(nil)
			}
			return nil
		}
		return c.toGo(v)
	}
	var header []any
	for _, item := range grid.list("header") {
		header = append(header, cell(item))
	}
	var rows [][]any
	for _, row := range grid.list("rows") {
		var cells []any
		for _, item := range row.([]any) {
			cells = append(cells, cell(item))
		}
		rows = append(rows, cells)
	}
	return NewDecisionTable(header, rows, options...)
}

// fromText checks RuleFromText on a rule text: the rule Loads reads, a
// rule_syntax_error for a condition or a rule set (spec/text.md), and an
// error where Loads fails.
func fromText(text string, limits Limits, limited bool, expect *jsonObject) string {
	var options []TextOption
	if limited {
		options = append(options, WithLimits(limits))
	}
	r, err := RuleFromText(text, options...)
	document := expect.obj("document")
	if expect.has("error") || document == nil {
		if expect.has("error") && err == nil {
			return "RuleFromText read a rule, expected the error " + writeJSON(expect.get("error"))
		}
		return ""
	}
	if document.has("rule") {
		if err != nil {
			return "RuleFromText: unexpected error " + err.Error()
		}
		if written := parsed(must2(Dumps(r))); !sameJSON(written, document, modeExact) {
			return "RuleFromText wrote " + writeJSON(written)
		}
		return ""
	}
	if err == nil || errorOfCode(err).Code != CodeRuleSyntax {
		what := "rule set"
		if document.has("condition") {
			what = "condition"
		}
		return fmt.Sprintf("RuleFromText of a %s: %v, expected a rule_syntax_error", what, err)
	}
	return ""
}
