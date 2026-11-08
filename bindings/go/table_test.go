package cleanrules

import (
	"errors"
	"math"
	"strings"
	"sync"
	"testing"
	"time"
)

// riskLevel is an enum as Go writes them: a named string type.
type riskLevel string

const riskHigh riskLevel = "high"

// riskTable is the table of docs/examples/risk.csv, built from cells of each
// kind.
func riskTable(t *testing.T) *DecisionTable {
	t.Helper()
	return must(NewDecisionTable(
		[]any{"RULE", Field("customer", "years"), "order.total", "THEN discount",
			TableOutput{Name: "risk", Values: []any{riskHigh, "medium", "low"}}, "DESCRIPTION"},
		[][]any{
			{"new", Field("customer", "years").Lt(1), "> 1000", 0, riskHigh, "New customers with large orders are checked first"},
			{"vip", "> 5", "-", MustDecimal("0.15"), "'low'", "Long-standing customers"},
			{"big", nil, "> 5000", MustDecimal("0.05"), "'medium'"},
			{"default", nil, nil, 0, "'low'"},
		},
		WithTablePolicy(TablePolicyPriority)))(t)
}

// TestTableCells checks each kind of cell against the CSV the core writes.
func TestTableCells(t *testing.T) {
	table := riskTable(t)
	csv := must(Dumps(table, WithFormat(FormatCSV)))(t)
	want := `HIT POLICY,priority
RULE,customer.years,order.total,THEN discount,"THEN risk IN ('high', 'medium', 'low')",DESCRIPTION
new,< 1,> 1000,0,'high',New customers with large orders are checked first
vip,> 5,-,0.15,'low',Long-standing customers
big,-,> 5000,0.05,'medium',
default,-,-,0,'low',
`
	if csv != want {
		t.Errorf("got:\n%s\nwant:\n%s", csv, want)
	}
	// The same table as the CSV file.
	read := must(Loads(want, WithFormat(FormatCSV)))(t).(*DecisionTable)
	if again := must(Dumps(read, WithFormat(FormatCSV)))(t); again != csv {
		t.Errorf("read back:\n%s", again)
	}

	level := riskLevel("it's")
	date := Date{2026, time.October, 31}
	cells := []struct {
		name string
		cell any
		csv  string
	}{
		{"text", "> 5", "> 5"},
		{"text string", "'VIP'", "'VIP'"},
		{"field name", "VIP", "VIP"},
		{"empty text", "", "-"},
		{"nil", nil, "-"},
		{"nil pointer", (*int)(nil), "-"},
		{"pointer to a string", ptr("> 5"), "> 5"},
		{"named string", riskHigh, "'high'"},
		{"named string with a quote", level, "'it''s'"},
		{"pointer to a named string", &level, "'it''s'"},
		{"int", 7, "7"},
		{"uint8", uint8(7), "7"},
		{"float", 0.5, "0.5e0"},
		{"decimal", MustDecimal("1.50"), "1.50"},
		{"bool", true, "TRUE"},
		{"date", date, `"DATE ""2026-10-31"""`},
		{"node", Field("x").Ge(3), ">= 3"},
	}
	for _, c := range cells {
		t.Run(c.name, func(t *testing.T) {
			table, err := NewDecisionTable([]any{"x", "THEN y"}, [][]any{{c.cell, 1}})
			if err != nil {
				t.Fatal(err)
			}
			csv := must(Dumps(table, WithFormat(FormatCSV)))(t)
			line := strings.Split(csv, "\n")[2]
			if got := strings.TrimPrefix(strings.TrimSuffix(line, ",1"), "row_1,"); got != c.csv {
				t.Errorf("%s: %q, want %q", c.name, line, c.csv)
			}
		})
	}
	// Under an output: a constant is the value, a node the value it
	// computes, a named string its string.
	outputs := must(NewDecisionTable([]any{"x", "THEN y"}, [][]any{
		{"> 1", Field("x").Mul(2)}, {"> 0", riskHigh}, {"-", "'s'"},
	}))(t)
	d := must(outputs.Evaluate(map[string]any{"x": 3}))(t)
	if out, _ := d.Output(); out["y"] != int64(6) {
		t.Errorf("node output: %v", out)
	}
	d = must(outputs.Evaluate(map[string]any{"x": 1}))(t)
	if out, _ := d.Output(); out["y"] != "high" {
		t.Errorf("named string output: %v", out)
	}
	// A PRIORITY column takes ints, and integral floats.
	prioritized := must(NewDecisionTable([]any{"RULE", "PRIORITY", "x", "THEN y"}, [][]any{
		{"a", 5, "> 1", 1}, {"b", 7.0, "> 1", 2},
	}, WithTablePolicy(TablePolicyPriority)))(t)
	if out, _ := must(prioritized.Evaluate(map[string]any{"x": 2}))(t).Output(); out["y"] != int64(2) {
		t.Errorf("priorities: %v", out)
	}
	// Rows may be shorter than the header; a table may have no rows.
	short := must(NewDecisionTable([]any{"x", "z", "THEN y"}, [][]any{{"> 1"}, {}}))(t)
	if short.Len() != 2 {
		t.Errorf("short rows: %d", short.Len())
	}
	empty := must(NewDecisionTable([]any{"x", "THEN y"}, nil))(t)
	if rows := must(empty.Rows())(t); empty.Len() != 0 || len(rows) != 0 {
		t.Errorf("no rows: %v", rows)
	}
	// A pointer to a TableOutput, and one without values: any value.
	any := must(NewDecisionTable([]any{"x", &TableOutput{Name: "y"}}, [][]any{{"> 1", 3}}))(t)
	if outputs := any.Outputs(); len(outputs) != 1 || outputs[0].Name != "y" || outputs[0].Values != nil {
		t.Errorf("outputs: %#v", outputs)
	}
}

// TestTableEnumCells checks a named string type (Go's enum) under each kind
// of column: the core reads its string by the column (cr_grid_push_row_cells).
func TestTableEnumCells(t *testing.T) {
	type label string
	table := must(NewDecisionTable(
		[]any{"RULE", "x", "THEN t", "DESCRIPTION"},
		[][]any{{label("vip"), riskHigh, riskHigh, label("it's high")}},
	))(t)
	rows := must(table.Rows())(t)
	if rows[0].Name != "vip" || rows[0].Description != "it's high" ||
		rows[0].Inputs[0].String() != `x = "high"` || rows[0].Outputs[0].String() != `"high"` {
		t.Errorf("row: %+v", rows[0])
	}
	if csv := must(Dumps(table, WithFormat(FormatCSV)))(t); csv != "HIT POLICY,first\nRULE,x,THEN t,DESCRIPTION\nvip,'high','high',it's high\n" {
		t.Errorf("csv: %q", csv)
	}
	// Under PRIORITY, a string value is not a number, whatever it says.
	for _, priority := range []any{label("5"), riskHigh} {
		_, err := NewDecisionTable([]any{"x", "PRIORITY", "THEN t"}, [][]any{{"> 1", priority, 1}})
		var e *Error
		if !errors.As(err, &e) || e.Code != CodeFormat || e.Pointer == nil || *e.Pointer != "/rows/0/1" {
			t.Errorf("priority %v: %#v", priority, err)
		}
	}
	// Text that is not UTF-8 is a value, which the core refuses as a constant.
	if _, err := NewDecisionTable([]any{"x", "THEN t"}, [][]any{{"\xff", 1}}); codeOf(err) != CodeExpression {
		t.Errorf("not UTF-8: %v", err)
	}
	// Header items follow pointers, and a named string type is text there.
	header := must(NewDecisionTable([]any{ptr("x"), label("THEN t"), Field("y")}, [][]any{{"> 1", 1, "< 2"}}))(t)
	if inputs := header.Inputs(); len(inputs) != 2 || inputs[0].String() != "x" || inputs[1].String() != "y" {
		t.Errorf("header: %v", inputs)
	}
}

func ptr[T any](v T) *T { return &v }

// TestTableAccessors reads what a table holds.
func TestTableAccessors(t *testing.T) {
	table := riskTable(t)
	if table.HitPolicy() != TablePolicyPriority || table.Aggregate() != "" || table.Logic() != "boolean" || table.Strict() || table.Len() != 4 {
		t.Errorf("%s %s %s %v %d", table.HitPolicy(), table.Aggregate(), table.Logic(), table.Strict(), table.Len())
	}
	if table.String() != "DecisionTable(priority: 4 rows, 4 columns)" {
		t.Error(table.String())
	}
	inputs := table.Inputs()
	if len(inputs) != 2 || !inputs[0].Equal(Field("customer", "years")) || inputs[1].String() != "order.total" {
		t.Errorf("inputs: %v", inputs)
	}
	outputs := table.Outputs()
	if len(outputs) != 2 || outputs[0].Name != "discount" || outputs[0].Values != nil || outputs[1].Name != "risk" ||
		len(outputs[1].Values) != 3 || outputs[1].Values[0] != "high" {
		t.Errorf("outputs: %#v", outputs)
	}
	outputs[1].Values[0] = "changed"
	if table.Outputs()[1].Values[0] != "high" {
		t.Error("Outputs gave the table's own slice")
	}
	rows := must(table.Rows())(t)
	if len(rows) != 4 {
		t.Fatalf("rows: %v", rows)
	}
	first := rows[0]
	if first.Name != "new" || first.Priority != nil || first.Description != "New customers with large orders are checked first" ||
		first.Inputs[0].String() != "customer.years < 1" || first.Inputs[1].String() != "order.total > 1000" ||
		first.Outputs[0].String() != "0" || first.Outputs[1].String() != `"high"` {
		t.Errorf("first row: %+v", first)
	}
	if rows[2].Inputs[0] != nil || rows[3].Description != "" {
		t.Errorf("empty cells: %+v %+v", rows[2], rows[3])
	}
	rows[0].Inputs[0] = nil
	if again := must(table.Rows())(t); again[0].Inputs[0] == nil {
		t.Error("Rows gave the table's own slices")
	}
	prioritized := must(Loads("RULE,PRIORITY,x,THEN y\na,5,> 1,1\n,,-,2\n", WithFormat(FormatCSV)))(t).(*DecisionTable)
	rows = must(prioritized.Rows())(t)
	if rows[0].Priority == nil || *rows[0].Priority != 5 || rows[1].Priority != nil || rows[1].Name != "" {
		t.Errorf("priorities and names: %+v", rows)
	}
	rs := must(prioritized.RuleSet())(t)
	if rs.Rules()[1].Name() != "row_2" || must(prioritized.RuleSet())(t) != rs {
		t.Errorf("rule set: %v", rs.Rules())
	}

	// Deciding.
	order := map[string]any{"customer": map[string]any{"years": 9}, "order": map[string]any{"total": 6000}}
	d := must(table.Evaluate(order))(t)
	if out, _ := d.Output(); out["risk"] != "medium" || out["discount"] != MustDecimal("0.05") {
		t.Errorf("decision: %v", out)
	}
	many := must(table.EvaluateMany([]any{order, map[string]any{}}))(t)
	if len(many) != 2 || many[1].Fired()[0] != "default" {
		t.Errorf("many: %v", many)
	}
	summary := must(table.Summary())(t)
	if len(summary.Rules) != 4 || summary.HitPolicy != HitPolicyPriority {
		t.Errorf("summary: %+v", summary)
	}
	json := must(table.JSON())(t)
	if !strings.Contains(json, `"decision_table"`) {
		t.Error(json)
	}

	// Close: what was read stays, the rest is ErrClosed.
	if err := table.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := table.Rows(); err != nil {
		t.Errorf("rows read before: %v", err)
	}
	if _, err := table.Evaluate(order); err != nil {
		// The rule set was compiled before Close, and closes with it.
		if !errors.Is(err, ErrClosed) {
			t.Errorf("evaluate: %v", err)
		}
	}
	if _, err := table.JSON(); !errors.Is(err, ErrClosed) {
		t.Errorf("JSON: %v", err)
	}
	if _, err := d.Explain(); err != nil {
		t.Errorf("a decision made before Close: %v", err)
	}
	if err := table.Close(); err != nil {
		t.Error(err)
	}
	fresh := riskTable(t)
	fresh.Close()
	for name, err := range map[string]error{
		"rows":     func() error { _, err := fresh.Rows(); return err }(),
		"rule set": func() error { _, err := fresh.RuleSet(); return err }(),
		"evaluate": func() error { _, err := fresh.Evaluate(order); return err }(),
		"many":     func() error { _, err := fresh.EvaluateMany(nil); return err }(),
		"summary":  func() error { _, err := fresh.Summary(); return err }(),
	} {
		if !errors.Is(err, ErrClosed) {
			t.Errorf("%s after Close: %v", name, err)
		}
	}
	var none *DecisionTable
	if none.Len() != 0 || none.HitPolicy() != "" || none.Aggregate() != "" || none.Logic() != "" || none.Strict() ||
		none.Inputs() != nil || none.Outputs() != nil || none.Close() != nil || none.String() != "DecisionTable(nil)" {
		t.Error("a nil table")
	}
	if _, err := none.Rows(); !errors.Is(err, ErrClosed) {
		t.Error(err)
	}
	if _, err := (&DecisionTable{}).RuleSet(); !errors.Is(err, ErrClosed) {
		t.Error(err)
	}
}

// TestTableSettings checks policies, aggregates, logic and strict mode.
func TestTableSettings(t *testing.T) {
	header := []any{"x", "THEN y"}
	rows := [][]any{{"> 1", 1}, {"> 2", 2}}
	collect := must(NewDecisionTable(header, rows, WithTablePolicy(TablePolicyCollect), WithAggregate(AggregateSum),
		WithLogic("boolean"), WithStrict(true)))(t)
	if collect.Aggregate() != AggregateSum || !collect.Strict() {
		t.Errorf("%s %v", collect.Aggregate(), collect.Strict())
	}
	if out, _ := must(collect.Evaluate(map[string]any{"x": 3}))(t).Output(); out["y"] != int64(3) {
		t.Errorf("sum: %v", out)
	}
	if _, err := collect.Evaluate(map[string]any{}); codeOf(err) != CodeMissingField {
		t.Errorf("strict: %v", err)
	}
	ordered := must(NewDecisionTable([]any{"x", "THEN y IN ('b', 'a')"}, [][]any{{"> 1", "'a'"}, {"> 2", "'b'"}},
		WithTablePolicy(TablePolicyOutputOrder)))(t)
	if ordered.HitPolicy() != TablePolicyOutputOrder {
		t.Error(ordered.HitPolicy())
	}
	// Every rule that fires, in the order of the output's values.
	if outs := must(ordered.Evaluate(map[string]any{"x": 3}))(t).Outputs(); len(outs) != 2 || outs[0]["y"] != "b" || outs[1]["y"] != "a" {
		t.Errorf("output order: %v", outs)
	}
	if _, err := NewDecisionTable(header, rows, WithLogic("fuzzy-ish")); codeOf(err) != CodeRegistry {
		t.Errorf("unknown logic: %v", err)
	}
	if _, err := NewDecisionTable(header, rows, nil); codeOf(err) != CodeRuleDefinition {
		t.Errorf("nil option: %v", err)
	}
	if _, err := NewDecisionTable(header, rows, WithTablePolicy("best")); codeOf(err) != CodeRuleDefinition {
		t.Errorf("unknown policy: %v", err)
	}
}

// TestTableErrors checks the errors of building a table and where they
// point.
func TestTableErrors(t *testing.T) {
	var nilExpr *Expr
	cases := []struct {
		name    string
		header  []any
		rows    [][]any
		options []TableOption
		code    Code
		pointer string
	}{
		{"header type", []any{"x", 5}, nil, nil, CodeFormat, "/header/1"},
		{"header nil expr", []any{nilExpr, "THEN y"}, nil, nil, CodeExpression, "/header/0"},
		{"header nil output", []any{"x", (*TableOutput)(nil)}, nil, nil, CodeFormat, "/header/1"},
		{"header text", []any{"x", "THEN"}, nil, nil, CodeRuleSyntax, "/header/1"},
		{"header empty", []any{"x", ""}, nil, nil, CodeFormat, "/header/1"},
		{"header condition", []any{Field("x").Gt(1), "THEN y"}, nil, nil, CodeExpression, "/header/0"},
		{"header broken node", []any{Field("x").Add("a"), "THEN y"}, nil, nil, CodeExpression, ""},
		{"output NaN", []any{"x", TableOutput{"y", []any{math.NaN()}}}, nil, nil, CodeExpression, "/header/1"},
		{"output repeated", []any{"x", TableOutput{"y", []any{1, 1}}}, nil, nil, CodeRuleDefinition, "/header/1"},
		{"no inputs", []any{"THEN y"}, nil, nil, CodeFormat, "/header"},
		{"cell syntax", []any{"x", "THEN y"}, [][]any{{"> > 1", 1}}, nil, CodeRuleSyntax, "/rows/0/0"},
		{"cell NaN", []any{"x", "THEN y"}, [][]any{{"> 1", 1}, {math.NaN(), 1}}, nil, CodeExpression, "/rows/1/0"},
		{"cell nanoseconds", []any{"x", "THEN y"}, [][]any{{time.Unix(0, 1), 1}}, nil, CodeExpression, "/rows/0/0"},
		{"cell nil expr", []any{"x", "THEN y"}, [][]any{{nilExpr, 1}}, nil, CodeExpression, "/rows/0/0"},
		{"cell broken node", []any{"x", "THEN y"}, [][]any{{Field("x").Add("a").Gt(1), 1}}, nil, CodeExpression, ""},
		{"cell other column", []any{"x", "THEN y"}, [][]any{{Field("z").Gt(1), 1}}, nil, CodeUnsupported, "/rows/0/0"},
		{"cell extra", []any{"x", "THEN y"}, [][]any{{"> 1", 1, 2}}, nil, CodeFormat, "/rows/0/2"},
		{"cell not listed", []any{"x", "THEN y IN (1, 2)"}, [][]any{{"> 1", 3}}, nil, CodeRuleDefinition, "/rows/0/1"},
		{"cell opaque", []any{"x", "THEN y"}, [][]any{{make(chan int), 1}}, nil, CodeExpression, "/rows/0/0"},
		{"rule name type", []any{"RULE", "x", "THEN y"}, [][]any{{5, "> 1", 1}}, nil, CodeFormat, "/rows/0/0"},
		{"aggregate", []any{"x", "THEN y"}, [][]any{{"> 1", 1}}, []TableOption{WithAggregate(AggregateSum)}, CodeRuleDefinition, "/aggregate"},
		{"table size", []any{"x", "THEN y"}, [][]any{{"> 1", 1}, {"> 2", 2}}, []TableOption{WithLimits(Limits{MaxTableSize: 2})}, CodeLimitExceeded, ""},
		{"text size", []any{"x", "THEN y"}, [][]any{{"> 1000000", 1}}, []TableOption{WithLimits(Limits{MaxTextSize: 4})}, CodeLimitExceeded, ""},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			_, err := NewDecisionTable(c.header, c.rows, c.options...)
			var e *Error
			if !errors.As(err, &e) {
				t.Fatalf("got %v", err)
			}
			if e.Code != c.code {
				t.Errorf("code %s, want %s: %s", e.Code, c.code, e.JSON)
			}
			if c.pointer != "" && (e.Pointer == nil || *e.Pointer != c.pointer) {
				t.Errorf("pointer %v, want %s: %s", e.Pointer, c.pointer, e.JSON)
			}
			if c.pointer != "" && !strings.Contains(e.JSON, `"pointer":"`+c.pointer+`"`) {
				t.Errorf("JSON form without the pointer: %s", e.JSON)
			}
		})
	}
}

// TestTableFormats writes a table in each format that holds it and reads
// it back with the same rows and outputs.
func TestTableFormats(t *testing.T) {
	table := riskTable(t)
	for _, format := range []Format{FormatCSV, FormatJSON, FormatYAML} {
		back := must(Loads(must(Dumps(table, WithFormat(format)))(t), WithFormat(format)))(t).(*DecisionTable)
		if back.Len() != table.Len() || back.HitPolicy() != table.HitPolicy() || len(back.Outputs()) != 2 {
			t.Errorf("%s: %v", format, back)
		}
		rows, original := must(back.Rows())(t), must(table.Rows())(t)
		for r := range rows {
			for c := range rows[r].Inputs {
				if (rows[r].Inputs[c] == nil) != (original[r].Inputs[c] == nil) ||
					(rows[r].Inputs[c] != nil && !rows[r].Inputs[c].Equal(original[r].Inputs[c])) {
					t.Errorf("%s: row %d input %d: %v", format, r, c, rows[r].Inputs[c])
				}
			}
		}
	}
	if _, err := Dumps(table, WithFormat(FormatText)); !errors.Is(err, ErrUnsupported) {
		t.Errorf("text: %v", err)
	}
}

// TestTableConcurrency reads a table's rows and rule set on many goroutines
// at once.
func TestTableConcurrency(t *testing.T) {
	table := riskTable(t)
	var wg sync.WaitGroup
	sets := make([]*RuleSet, 8)
	for i := range sets {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if _, err := table.Rows(); err != nil {
				t.Error(err)
			}
			sets[i], _ = table.RuleSet()
			if _, err := table.Evaluate(map[string]any{}); err != nil {
				t.Error(err)
			}
		}()
	}
	wg.Wait()
	for _, rs := range sets {
		if rs != sets[0] {
			t.Error("the rule set was compiled twice")
		}
	}
}
