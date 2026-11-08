package cleanrules

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

// documents are one value of each kind of document.
func documents(t *testing.T) map[string]Document {
	t.Helper()
	cond := Field("customer", "years").Gt(5).And(Field("country").IsIn("ES", "PT"))
	vip := rule(t, "vip", cond, Then("discount", MustDecimal("0.15")), WithPriority(10), WithAction("notify"))
	loyal := rule(t, "loyal", Field("customer", "years").Gt(2), Then("discount", 0.05))
	rs := ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyPriority)}, vip, loyal)
	table := must(NewDecisionTable(
		[]any{"RULE", "customer.years", "THEN discount"},
		[][]any{{"vip", "> 5", MustDecimal("0.15")}, {"loyal", "> 2", MustDecimal("0.05")}},
		WithTablePolicy(TablePolicyFirst)))(t)
	return map[string]Document{"condition": cond, "rule": vip, "ruleset": rs, "table": table}
}

// TestDocumentsBothWays writes each kind of document in each format that
// holds it, reads it back as the same kind, and writes it again the same.
func TestDocumentsBothWays(t *testing.T) {
	formats := map[string][]Format{
		"condition": {FormatJSON, FormatYAML, FormatText},
		"rule":      {FormatJSON, FormatYAML, FormatText},
		"ruleset":   {FormatJSON, FormatYAML, FormatText},
		"table":     {FormatJSON, FormatYAML, FormatCSV},
	}
	for kind, doc := range documents(t) {
		for _, format := range formats[kind] {
			t.Run(kind+"/"+string(format), func(t *testing.T) {
				text := must(Dumps(doc, WithFormat(format)))(t)
				back := must(Loads(text, WithFormat(format)))(t)
				if kindOf(back) != kind {
					t.Fatalf("read a %s, wrote a %s", kindOf(back), kind)
				}
				if again := must(Dumps(back, WithFormat(format)))(t); again != text {
					t.Errorf("written again:\n%s\nfirst:\n%s", again, text)
				}
				// And from bytes.
				if fromBytes := must(Loads(string([]byte(text)), WithFormat(format)))(t); kindOf(fromBytes) != kind {
					t.Errorf("from bytes: a %T", fromBytes)
				}
			})
		}
	}
}

func kindOf(doc Document) string {
	switch doc.(type) {
	case *Expr:
		return "condition"
	case *Rule:
		return "rule"
	case *RuleSet:
		return "ruleset"
	case *DecisionTable:
		return "table"
	}
	return "nil"
}

// TestLoadedObjects reads what the core read: a rule set's settings and
// rules, a rule's parts, a condition's structure.
func TestLoadedObjects(t *testing.T) {
	text := "RULE vip PRIORITY 10 IF customer.years > 5 THEN discount = 0.15 DO notify\n" +
		"RULE loyal IF customer.years > 2 THEN discount = 0.05"
	doc := must(Loads(text, WithFormat(FormatText)))(t)
	rs, ok := doc.(*RuleSet)
	if !ok {
		t.Fatalf("a %T", doc)
	}
	if rs.Len() != 2 || rs.Rules()[0].Name() != "vip" || rs.Rules()[0].Priority() != 10 || rs.Rules()[0].ActionName() != "notify" {
		t.Errorf("rules: %v", rs.Rules())
	}
	d := must(rs.Evaluate(map[string]any{"customer": map[string]any{"years": 7}}))(t)
	if out, _ := d.Output(); out["discount"] != MustDecimal("0.15") || d.Results()[0].Rule != rs.Rules()[0] {
		t.Errorf("decision: %v %v", out, d.Results())
	}
	cond := must(Loads(`{"version": 1, "condition": {"op": "gt", "left": {"op": "field", "path": ["x"]}, "right": {"op": "const", "value": 1}}}`))(t)
	if !cond.(*Expr).Equal(Field("x").Gt(1)) {
		t.Errorf("condition: %v", cond)
	}
}

// TestLoadsErrors checks the errors of reading, with their places.
func TestLoadsErrors(t *testing.T) {
	cases := []struct {
		name    string
		text    string
		options []LoadOption
		code    Code
		pointer string
		line    int
	}{
		{"unknown key", `{"version": 1, "condition": {"op": "gt", "left": 1, "right": 2, "x": 1}}`, nil, CodeFormat, "/condition", 0},
		{"syntax", "IF x >", []LoadOption{WithFormat(FormatText)}, CodeRuleSyntax, "", 1},
		{"size", `{"version": 1}`, []LoadOption{WithLimits(Limits{MaxSize: 4})}, CodeLimitExceeded, "", 0},
		{"depth", "IF ((((x > 1))))", []LoadOption{WithFormat(FormatText), WithLimits(Limits{MaxDepth: 1})}, CodeLimitExceeded, "", 0},
		{"yaml size", "version: 1\n", []LoadOption{WithFormat(FormatYAML), WithLimits(Limits{MaxYAMLSize: 3})}, CodeLimitExceeded, "", 0},
		{"not UTF-8", "IF x > '\xff'", []LoadOption{WithFormat(FormatText)}, CodeFormat, "", 0},
		{"value node", `{"version": 1, "condition": {"op": "add", "left": {"op": "field", "path": ["a"]}, "right": {"op": "const", "value": 1}}}`, nil, CodeExpression, "/condition", 0},
		{"unknown format", "x", []LoadOption{WithFormat("xml")}, CodeFormat, "", 0},
		{"nil option", "x", []LoadOption{nil}, CodeRuleDefinition, "", 0},
		{"negative limit", "x", []LoadOption{WithLimits(Limits{MaxDepth: -1})}, CodeFormat, "", 0},
		{"empty", "", nil, CodeFormat, "", 0},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			_, err := Loads(c.text, c.options...)
			var e *Error
			if !errors.As(err, &e) || e.Code != c.code {
				t.Fatalf("got %#v, want %s", err, c.code)
			}
			if c.pointer != "" && (e.Pointer == nil || !strings.HasPrefix(*e.Pointer, c.pointer)) {
				t.Errorf("pointer %v, want %s", e.Pointer, c.pointer)
			}
			if c.line != 0 && (e.Line != c.line || e.Column == 0) {
				t.Errorf("at %d:%d", e.Line, e.Column)
			}
		})
	}
}

// TestLimits checks DefaultLimits and how the fields set the core's limits.
func TestLimits(t *testing.T) {
	defaults := DefaultLimits()
	for i, field := range defaults.fields() {
		if *field <= 0 {
			t.Errorf("default %d is %d", i, *field)
		}
	}
	if defaults.MaxYAMLSize >= defaults.MaxSize {
		t.Errorf("defaults %+v", defaults)
	}
	if got := must((&Limits{}).effective())(t); got != defaults {
		t.Errorf("zero limits: %+v", got)
	}
	if got := must((&Limits{MaxSize: 100, MaxTextSize: 50}).effective())(t); got.MaxSize != 100 || got.MaxYAMLSize != 100 || got.MaxTextSize != 50 || got.MaxDepth != defaults.MaxDepth {
		t.Errorf("MaxSize sets the three, the others change theirs: %+v", got)
	}
	changed := defaults
	changed.MaxDepth = 3
	changed.MaxSize = 100
	if got := must(changed.effective())(t); got != changed || got.MaxYAMLSize != defaults.MaxYAMLSize {
		t.Errorf("defaults changed: %+v", got)
	}
	// A negative limit is an error that names it, wherever limits are taken.
	for _, field := range limitNames {
		var l Limits
		*l.fields()[slices.Index(limitNames, field)] = -1
		_, err := l.effective()
		var e *Error
		if !errors.As(err, &e) || e.Code != CodeFormat || !strings.Contains(e.Message, "Limits."+field+" is -1") {
			t.Errorf("%s: %v", field, err)
		}
		for name, err := range map[string]error{
			"Loads":            func() error { _, err := Loads("{}", WithLimits(l)); return err }(),
			"RuleFromText":     func() error { _, err := RuleFromText("IF x > 1 THEN y = 1", WithLimits(l)); return err }(),
			"NewDecisionTable": func() error { _, err := NewDecisionTable([]any{"x", "THEN y"}, nil, WithLimits(l)); return err }(),
			"ReadDecision":     func() error { _, err := echo(t).ReadDecision("{}", WithLimits(l)); return err }(),
			"Load":             func() error { _, err := Load("x.json", WithLimits(l)); return err }(),
		} {
			if codeOf(err) != CodeFormat {
				t.Errorf("%s with %s: %v", name, field, err)
			}
		}
	}
}

// TestLoadFiles writes and reads files in the format of their extension.
func TestLoadFiles(t *testing.T) {
	dir := t.TempDir()
	docs := documents(t)
	files := map[string]string{
		"condition": "c.yml", "rule": "r.rules", "ruleset": "rs.json", "table": "t.csv",
	}
	for kind, name := range files {
		path := filepath.Join(dir, name)
		if err := Dump(docs[kind], path); err != nil {
			t.Fatal(err)
		}
		if back := must(Load(path))(t); kindOf(back) != kind {
			t.Errorf("%s: a %T", name, back)
		}
	}
	text := must(os.ReadFile(filepath.Join(dir, "r.rules")))(t)
	if !strings.HasPrefix(string(text), "RULE vip PRIORITY 10\n") || !strings.HasSuffix(string(text), "\n") {
		t.Errorf("rule text: %q", text)
	}
	// The format given wins over the extension.
	path := filepath.Join(dir, "rules.txt")
	if err := Dump(docs["ruleset"], path, WithFormat(FormatYAML)); err != nil {
		t.Fatal(err)
	}
	if _, err := Load(path); err == nil || codeOf(err) != "" || !strings.Contains(err.Error(), "rules.txt") {
		t.Errorf("unknown extension: %v", err)
	}
	if err := Dump(docs["ruleset"], path); err == nil || codeOf(err) != "" {
		t.Errorf("unknown extension: %v", err)
	}
	if back := must(Load(path, WithFormat(FormatYAML)))(t); kindOf(back) != "ruleset" {
		t.Errorf("a %T", back)
	}
	if _, err := Load(filepath.Join(dir, "missing.json")); !errors.Is(err, fs.ErrNotExist) {
		t.Errorf("missing file: %v", err)
	}
	// A file beyond its format's size limit is not read; the hint names the
	// field to raise.
	for name, field := range map[string]string{"rs.json": "MaxSize", "c.yml": "MaxYAMLSize", "t.csv": "MaxTextSize"} {
		_, err := Load(filepath.Join(dir, name), WithLimits(Limits{MaxSize: 10}))
		var e *Error
		if !errors.As(err, &e) || e.Code != CodeLimitExceeded || !strings.Contains(e.Hint, "Limits."+field) || !strings.Contains(e.Message, name) {
			t.Errorf("%s: %#v", name, err)
		}
	}
	if _, err := Load(filepath.Join(dir, "rs.json"), nil); codeOf(err) != CodeRuleDefinition {
		t.Errorf("nil option: %v", err)
	}
	if err := Dump(docs["ruleset"], filepath.Join(dir, "x.json"), nil); codeOf(err) != CodeRuleDefinition {
		t.Errorf("nil option: %v", err)
	}
	if _, err := Load(filepath.Join(dir, "rs.json"), WithFormat("xml")); codeOf(err) != CodeFormat {
		t.Errorf("unknown format: %v", err)
	}
	if err := Dump(Field("x").Add(1), filepath.Join(dir, "v.json")); codeOf(err) != CodeExpression {
		t.Errorf("value node: %v", err)
	}
}

// TestLoadActions binds the actions a document names on loading.
func TestLoadActions(t *testing.T) {
	text := "RULE vip IF x > 1 THEN y = 1 DO notify\nRULE other IF x > 0 THEN y = 2"
	var ran []string
	actions := Actions{
		"notify": func(ctx *ActionContext) error { ran = append(ran, ctx.Rule().Name()); return nil },
		"unused": func(*ActionContext) error { return nil },
	}
	bound := must(Loads(text, WithFormat(FormatText), WithActions(actions)))(t).(*RuleSet)
	if _, err := bound.Run(map[string]any{"x": 2}); err != nil || len(ran) != 1 || ran[0] != "vip" {
		t.Errorf("run: %v %v", ran, err)
	}
	// Without WithActions, the names stay unbound.
	unbound := must(Loads(text, WithFormat(FormatText)))(t).(*RuleSet)
	if _, err := unbound.Run(map[string]any{"x": 2}); codeOf(err) != CodeRegistry {
		t.Errorf("unbound: %v", err)
	}
	// A name the registry lacks is an error at loading, naming the rule.
	_, err := Loads(text, WithFormat(FormatText), WithActions(Actions{"other": actions["notify"]}))
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeRegistry || e.Rule != "vip" || e.Name != "notify" {
		t.Errorf("missing action: %#v", err)
	}
	// A rule holds no function: WithActions checks its name.
	one := "RULE vip IF x > 1 THEN y = 1 DO notify"
	if r := must(Loads(one, WithFormat(FormatText), WithActions(actions)))(t).(*Rule); r.ActionName() != "notify" {
		t.Errorf("rule: %v", r.ActionName())
	}
	if _, err := Loads(one, WithFormat(FormatText), WithActions(nil)); codeOf(err) != CodeRegistry {
		t.Errorf("rule without its action: %v", err)
	}
	// A table has no actions; a condition neither.
	table := must(Dumps(documents(t)["table"], WithFormat(FormatCSV)))(t)
	if _, err := Loads(table, WithFormat(FormatCSV), WithActions(nil)); err != nil {
		t.Error(err)
	}
	// From a file.
	path := filepath.Join(t.TempDir(), "rules.rules")
	if err := os.WriteFile(path, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	ran = nil
	fromFile := must(Load(path, WithActions(actions)))(t).(*RuleSet)
	if _, err := fromFile.Run(map[string]any{"x": 2}); err != nil || len(ran) != 1 {
		t.Errorf("from a file: %v %v", ran, err)
	}
}

// TestActionNamesAreWritten checks that a Go rule's action, always a name,
// is written, bound or not.
func TestActionNamesAreWritten(t *testing.T) {
	rs := documents(t)["ruleset"].(*RuleSet)
	bound := must(rs.Bind(Actions{"notify": func(*ActionContext) error { return nil }}))(t)
	for _, doc := range []Document{rs, bound, rs.Rules()[0]} {
		if text := must(Dumps(doc))(t); !strings.Contains(text, `"do":"notify"`) {
			t.Errorf("%s", text)
		}
	}
	path := filepath.Join(t.TempDir(), "rules.yaml")
	if err := Dump(bound, path); err != nil {
		t.Fatal(err)
	}
	if text := must(os.ReadFile(path))(t); !strings.Contains(string(text), "do: \"notify\"") {
		t.Errorf("%s", text)
	}
}

// TestDumpsErrors checks what cannot be written.
func TestDumpsErrors(t *testing.T) {
	docs := documents(t)
	closed := ruleSet(t, nil, rule(t, "r", Field("x").Gt(1)))
	closed.Close()
	withMetadata := rule(t, "m", Field("x").Gt(1), WithMetadata(map[string]any{"a": 1}))
	cases := map[string]struct {
		doc     Document
		options []DumpOption
		want    error
	}{
		"value node":       {Field("x").Add(1), nil, ErrExpression},
		"broken node":      {Field("x").Add("a"), nil, ErrExpression},
		"rule set as CSV":  {docs["ruleset"], []DumpOption{WithFormat(FormatCSV)}, ErrUnsupported},
		"table as text":    {docs["table"], []DumpOption{WithFormat(FormatText)}, ErrUnsupported},
		"metadata as text": {withMetadata, []DumpOption{WithFormat(FormatText)}, ErrUnsupported},
		"unknown format":   {docs["rule"], []DumpOption{WithFormat("toml")}, ErrFormat},
		"nil option":       {docs["rule"], []DumpOption{nil}, ErrRuleDefinition},
		"nil":              {nil, nil, ErrUnsupported},
		"nil expr":         {(*Expr)(nil), nil, ErrExpression},
		"nil rule":         {(*Rule)(nil), nil, ErrRuleDefinition},
		"nil rule set":     {(*RuleSet)(nil), nil, ErrClosed},
		"nil table":        {(*DecisionTable)(nil), nil, ErrClosed},
		"closed":           {closed, nil, ErrClosed},
	}
	for name, c := range cases {
		if _, err := Dumps(c.doc, c.options...); !errors.Is(err, c.want) {
			t.Errorf("%s: %v, want %v", name, err, c.want)
		}
	}
}

// TestRuleFromText reads one rule, as fuzzytool's rule_from_text does.
func TestRuleFromText(t *testing.T) {
	r := must(RuleFromText("IF customer.years > 5\nTHEN discount = 0.15"))(t)
	if r.Name() != "rule_1" || r.When().String() != "customer.years > 5" {
		t.Errorf("%s: %s", r.Name(), r.When())
	}
	named := must(RuleFromText("RULE vip PRIORITY 3 IF x > 1 THEN y = 2 DO notify"))(t)
	if named.Name() != "vip" || named.Priority() != 3 || named.ActionName() != "notify" {
		t.Errorf("%s %d %s", named.Name(), named.Priority(), named.ActionName())
	}
	cases := map[string]struct {
		text    string
		options []TextOption
		code    Code
	}{
		"condition": {"x > 1", nil, CodeRuleSyntax},
		"two rules": {"IF x > 1 THEN y = 1\nRULE b IF x > 2 THEN y = 2", nil, CodeRuleSyntax},
		"syntax":    {"IF x > THEN y = 1", nil, CodeRuleSyntax},
		"size":      {"IF x > 1 THEN y = 1", []TextOption{WithLimits(Limits{MaxTextSize: 5})}, CodeLimitExceeded},
		"depth":     {"IF (((x > 1))) THEN y = 1", []TextOption{WithLimits(Limits{MaxDepth: 1})}, CodeLimitExceeded},
		"action":    {"IF x > 1 THEN y = 1 DO notify", []TextOption{WithActions(Actions{})}, CodeRegistry},
		"nil":       {"IF x > 1 THEN y = 1", []TextOption{nil}, CodeRuleDefinition},
		"empty":     {"", nil, CodeRuleSyntax},
	}
	for name, c := range cases {
		_, err := RuleFromText(c.text, c.options...)
		var e *Error
		if !errors.As(err, &e) || e.Code != c.code {
			t.Errorf("%s: %#v, want %s", name, err, c.code)
		} else if c.code == CodeRuleSyntax && name != "empty" && (e.Line == 0 || e.Column == 0) {
			t.Errorf("%s: no place in %s", name, e.JSON)
		}
	}
	if _, err := RuleFromText("IF x > 1 THEN y = 1 DO notify", WithActions(Actions{"notify": func(*ActionContext) error { return nil }})); err != nil {
		t.Error(err)
	}
}

// TestReadDecision reads a recorded decision back with its rule set.
func TestReadDecision(t *testing.T) {
	rs := documents(t)["ruleset"].(*RuleSet)
	data := map[string]any{"customer": map[string]any{"years": 7}, "country": "ES"}
	d := must(rs.Evaluate(data))(t)
	text := must(d.JSON())(t)
	back := must(rs.ReadDecision(text))(t)
	if again := must(back.JSON())(t); again != text {
		t.Errorf("read back:\n%s\nwrote:\n%s", again, text)
	}
	if out, _ := back.Output(); out["discount"] != MustDecimal("0.15") || back.Fired()[0] != "vip" {
		t.Errorf("%v %v", out, back.Fired())
	}
	if _, err := back.Explain(); err != nil {
		t.Error(err)
	}
	cases := map[string]struct {
		text    string
		options []ReadOption
		want    error
	}{
		"text":      {text, []ReadOption{WithFormat(FormatText)}, ErrUnsupported},
		"other":     {`{"version": 1, "decision": {"hit_policy": "first", "logic": "boolean", "results": [], "selected": []}}`, nil, ErrDecisionMismatch},
		"size":      {text, []ReadOption{WithLimits(Limits{MaxSize: 10})}, ErrLimitExceeded},
		"nesting":   {text, []ReadOption{WithLimits(Limits{MaxNesting: 2})}, ErrLimitExceeded},
		"format":    {text, []ReadOption{WithFormat("xml")}, ErrFormat},
		"nil":       {text, []ReadOption{nil}, ErrRuleDefinition},
		"not JSON":  {"{", nil, ErrFormat},
		"as YAML":   {text, []ReadOption{WithFormat(FormatYAML)}, nil},
		"big limit": {text, []ReadOption{WithLimits(Limits{MaxSize: len(text)})}, nil},
	}
	for name, c := range cases {
		if _, err := rs.ReadDecision(c.text, c.options...); !errors.Is(err, c.want) && (c.want != nil || err != nil) {
			t.Errorf("%s: %v, want %v", name, err, c.want)
		}
	}
	closed := ruleSet(t, nil, rule(t, "r", Field("x").Gt(1)))
	closed.Close()
	if _, err := closed.ReadDecision(text); !errors.Is(err, ErrClosed) {
		t.Errorf("closed: %v", err)
	}
}

// TestReadDecisionOpaque checks that an opaque value of a decision read
// back is nil: the document keeps only its type's name.
func TestReadDecisionOpaque(t *testing.T) {
	rs := ruleSet(t, nil, rule(t, "r", Const(true), Then("v", Field("x"))))
	opaque := make(chan int)
	d := must(rs.Evaluate(map[string]any{"x": opaque}))(t)
	if out, _ := d.Output(); out["v"] != opaque {
		t.Fatalf("evaluated: %v", out)
	}
	back := must(rs.ReadDecision(must(d.JSON())(t)))(t)
	if out, _ := back.Output(); out["v"] != nil || len(out) != 1 {
		t.Errorf("read back: %#v", out)
	}
}
