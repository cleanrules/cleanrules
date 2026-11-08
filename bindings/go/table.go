package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"reflect"
	"slices"
	"sync"
	"sync/atomic"
	"unicode/utf8"
	"unsafe"
)

// TablePolicy is a decision table's hit policy (spec/tables.md, Model): a
// rule set's, or TablePolicyOutputOrder.
type TablePolicy string

// The hit policies of decision tables.
const (
	TablePolicyFirst     TablePolicy = "first"
	TablePolicyPriority  TablePolicy = "priority"
	TablePolicyUnique    TablePolicy = "unique"
	TablePolicyAny       TablePolicy = "any"
	TablePolicyRuleOrder TablePolicy = "rule_order"
	// TablePolicyOutputOrder selects every row that fires, sorted by output
	// priority (spec/tables.md, Output priority): its rule set's policy is
	// HitPolicyRuleOrder, so read Decision.Outputs.
	TablePolicyOutputOrder TablePolicy = "output_order"
	TablePolicyCollect     TablePolicy = "collect"
)

// TableOutput is an output column of a decision table: its payload key and
// its output values, highest priority first (the column THEN key IN (...)),
// or none for any value. As a header item of NewDecisionTable, the values
// are constants, as Const reads them.
type TableOutput struct {
	Name   string
	Values []any
}

// TableRow is a row of a decision table, apart from its settings: its name
// ("" when it has none: its rule is then row_<n>, counting from 1), its
// priority (nil when it has none), its description ("" when it has none),
// and its cells: an input's condition, which tests its column, or an
// output's value, and nil for an empty cell.
type TableRow struct {
	Name        string
	Priority    *int64
	Description string
	Inputs      []*Expr
	Outputs     []*Expr
}

// DecisionTable is a decision table and the rule set it compiles into
// (spec/tables.md): a grid of inputs, outputs and rows. Build one with
// NewDecisionTable, or read one with Load or Loads (CSV, JSON or YAML), and
// write it with Dumps. The core reads, checks and compiles every table, so
// a table means the same in every language. It never changes, so one may be
// used on many goroutines at once. Close frees its native memory now;
// otherwise it is freed once the table is unreachable.
type DecisionTable struct {
	h         *handle
	hitPolicy TablePolicy
	aggregate Aggregate
	logic     string
	strict    bool
	inputs    []*Expr
	outputs   []TableOutput
	rowCount  int

	mu   sync.Mutex // guards rows
	rows []TableRow

	compile    sync.Once
	ruleSet    atomic.Pointer[RuleSet]
	compileErr error
}

// NewDecisionTable makes a decision table of a header and rows (spec/tables.md,
// Grids), with TablePolicyFirst, no aggregate, Boolean logic and default
// limits unless options say otherwise.
//
//	table, err := cleanrules.NewDecisionTable(
//		[]any{"RULE", "customer.years", cleanrules.Field("order", "total"), "THEN discount"},
//		[][]any{
//			{"vip", "> 5", "> 1000", cleanrules.MustDecimal("0.15")},
//			{"loyal", "> 2", nil, cleanrules.MustDecimal("0.05")},
//		},
//		cleanrules.WithTablePolicy(cleanrules.TablePolicyFirst))
//
// Each header item is a string, read as a CSV header field ("RULE",
// "PRIORITY", "DESCRIPTION", "THEN key", "THEN key IN (...)" or an input
// expression), an input *Expr (a value node, such as a field), or a
// TableOutput (or a pointer to one). Each row holds cells in column order,
// and may be shorter than the header. A cell is:
//
//   - a string: text, read as an unquoted CSV field in the rule syntax:
//     "> 5", "-", "'VIP'" for the string VIP, since "VIP" is the field named
//     VIP;
//   - nil (or a nil pointer): an empty cell;
//   - an *Expr: under an input, the cell's condition, which must test its
//     column (Field("age").Lt(18) under age); under an output, its value;
//   - a value of a named string type (type Level string, Go's enums): a
//     constant, its string, written as a quoted literal ('high'), never a
//     field name;
//   - any other value: a constant, as Const reads it: under an input x, the
//     test x = value; under an output, the value; under PRIORITY, an int.
//
// Errors point into the grid, counting from 0: /header/<c> for a column,
// /rows/<r>/<c> for a cell, /rows/<r> for a row, /aggregate for an aggregate
// without TablePolicyCollect. An unknown policy, aggregate or logic has no
// pointer.
func NewDecisionTable(header []any, rows [][]any, options ...TableOption) (*DecisionTable, error) {
	var settings tableSettings
	for _, option := range options {
		if option == nil {
			return nil, nilOption(tableOptions)
		}
		option.applyTable(&settings)
	}
	hp, hl := optionalText(settings.hitPolicy)
	ap, al := optionalText(settings.aggregate)
	lp, ll := optionalText(settings.logic)
	limits, err := settings.limits.native()
	if err != nil {
		return nil, err
	}
	defer C.cr_limits_free(limits)
	var grid *C.cr_grid
	var e *C.cr_error
	if err := check(C.cr_grid_new_open(hp, hl, ap, al, lp, ll, C.bool(settings.strict), limits, &grid, &e), e); err != nil {
		return nil, err
	}
	finished := false
	defer func() {
		if !finished {
			C.cr_grid_free(grid)
		}
	}()
	for c, item := range header {
		if err := addColumn(grid, c, item); err != nil {
			return nil, err
		}
	}
	for r, row := range rows {
		if err := pushRow(grid, r, row); err != nil {
			return nil, err
		}
	}
	finished = true // cr_grid_finish consumes the grid, even when it fails
	var document *C.cr_document
	if err := check(C.cr_grid_finish(grid, &document, &e), e); err != nil {
		return nil, err
	}
	return newDecisionTable(document)
}

// addColumn adds one header item to an open grid.
func addColumn(grid *C.cr_grid, c int, item any) error {
	pointer := fmt.Sprintf("/header/%d", c)
	var e *C.cr_error
	switch item := item.(type) {
	case string:
		p, n := text(item)
		return check(C.cr_grid_add_column_text(grid, p, n, &e), e)
	case *Expr:
		if item == nil {
			return at(errNilExpr, pointer)
		}
		return item.use(func(node *C.cr_node) error {
			return check(C.cr_grid_add_column_input(grid, node, &e), e)
		})
	case *TableOutput:
		if item == nil {
			return formError(errorForm{Code: CodeFormat, Message: "a nil *TableOutput is not a column", Hint: "give a TableOutput", Pointer: &pointer})
		}
		return addOutput(grid, pointer, *item)
	case TableOutput:
		return addOutput(grid, pointer, item)
	}
	// Text, as cells read it: through pointers, a named string type too.
	if inner, _ := deref(reflect.ValueOf(item)); inner.IsValid() && inner.Kind() == reflect.String {
		p, n := text(inner.String())
		return check(C.cr_grid_add_column_text(grid, p, n, &e), e)
	}
	return formError(errorForm{
		Code:    CodeFormat,
		Message: fmt.Sprintf("a header item is a string, an *Expr or a TableOutput, not %T", item),
		Hint:    "give the column's CSV header text, an input expression or a TableOutput",
		Pointer: &pointer,
	})
}

func addOutput(grid *C.cr_grid, pointer string, output TableOutput) error {
	var values *C.cr_value // NULL: any value
	if len(output.Values) > 0 {
		for _, v := range output.Values {
			if err := checkConstant(v); err != nil {
				return at(err, pointer)
			}
		}
		var err error
		if values, err = newReader().read(output.Values, wholePaths); err != nil {
			return err
		}
		defer C.cr_value_free(values)
	}
	p, n := text(output.Name)
	var e *C.cr_error
	return check(C.cr_grid_add_column_output(grid, p, n, values, &e), e)
}

// pushRow adds one row of cells to a grid.
func pushRow(grid *C.cr_grid, r int, cells []any) error {
	list := newList(len(cells))
	defer C.cr_value_free(list)
	kinds := make([]C.cr_cell_kind, len(cells))
	nodes := make([]*C.cr_node, len(cells))
	for c, cell := range cells {
		pointer := fmt.Sprintf("/rows/%d/%d", r, c)
		if node, ok := cell.(*Expr); ok {
			if node == nil {
				return at(errNilExpr, pointer)
			}
			if err := node.Err(); err != nil {
				return err
			}
			p, err := node.h.acquire()
			if err != nil {
				return err
			}
			defer node.h.release()
			kinds[c], nodes[c] = C.CR_CELL_NODE, (*C.cr_node)(p)
			setItem(list, c, C.cr_value_new_null())
			continue
		}
		kind, value, err := cellValue(cell)
		if err != nil {
			return at(err, pointer)
		}
		kinds[c] = kind
		setItem(list, c, value)
	}
	var kind *C.cr_cell_kind
	var node **C.cr_node
	if len(cells) > 0 {
		kind, node = &kinds[0], &nodes[0]
	}
	var e *C.cr_error
	return check(C.cr_grid_push_row_cells(grid, list, kind, node, C.size_t(len(cells)), &e), e)
}

// cellValue is a cell that is not a node, and its kind (spec/tables.md,
// Grids): a string, through pointers, is text; anything else is a value,
// which the core reads by its column: nil an empty cell, and a named string
// type (Go's enum) a string value, a constant under an input or an output
// and a name or description under RULE or DESCRIPTION, never text.
func cellValue(cell any) (C.cr_cell_kind, *C.cr_value, error) {
	if inner, _ := deref(reflect.ValueOf(cell)); inner.IsValid() && inner.Type() == stringType && utf8.ValidString(inner.String()) {
		value, err := newReader().read(inner.String(), wholePaths)
		return C.CR_CELL_TEXT, value, err
	}
	if err := checkConstant(cell); err != nil {
		return 0, nil, err
	}
	value, err := newReader().read(cell, wholePaths)
	return C.CR_CELL_VALUE, value, err
}

// at gives an error made in Go a JSON Pointer into the grid.
func at(err error, pointer string) error {
	e, ok := err.(*Error)
	if !ok || e.Pointer != nil {
		return err
	}
	return formError(errorForm{Code: e.Code, Message: e.Message, Hint: e.Hint, Pointer: &pointer})
}

// newDecisionTable wraps a document holding a decision table, which it owns
// from here (and frees, if reading it fails).
func newDecisionTable(document *C.cr_document) (*DecisionTable, error) {
	t := &DecisionTable{}
	t.h = newHandle(t, unsafe.Pointer(document), func(p unsafe.Pointer) { C.cr_document_free((*C.cr_document)(p)) })
	var out *C.cr_value
	var e *C.cr_error
	if err := check(C.cr_document_table_value(document, &out, &e), e); err != nil {
		t.h.close()
		return nil, err
	}
	table, _ := takeValue(out, nil).(map[string]any)
	t.hitPolicy = TablePolicy(str(table["hit_policy"]))
	t.aggregate = Aggregate(str(table["aggregate"]))
	t.logic = str(table["logic"])
	t.strict, _ = table["strict"].(bool)
	count, _ := table["input_count"].(int64)
	rowCount, _ := table["row_count"].(int64)
	t.rowCount = int(rowCount)
	for i := range int(count) {
		var node *C.cr_node
		if err := check(C.cr_document_table_input(document, C.size_t(i), &node, &e), e); err != nil {
			t.h.close()
			return nil, err
		}
		t.inputs = append(t.inputs, wrapNode(node))
	}
	outputs, _ := table["outputs"].([]any)
	for _, item := range outputs {
		output, _ := item.(map[string]any)
		values, _ := output["values"].([]any)
		if len(values) == 0 {
			values = nil // any value
		}
		t.outputs = append(t.outputs, TableOutput{Name: str(output["name"]), Values: values})
	}
	return t, nil
}

// use runs f with the table's document, or gives ErrClosed.
func (t *DecisionTable) use(f func(*C.cr_document) error) error {
	if t == nil {
		return ErrClosed
	}
	p, err := t.h.acquire()
	if err != nil {
		return err
	}
	defer t.h.release()
	return f((*C.cr_document)(p))
}

// Close frees the table's native memory now, or when the calls using it
// return, and closes the rule set it compiled into (RuleSet); later calls
// give ErrClosed, but the rows read before stay readable and the decisions
// made before stay valid. Closing twice does nothing.
func (t *DecisionTable) Close() error {
	if t == nil {
		return nil
	}
	t.h.close()
	return t.ruleSet.Load().Close()
}

// HitPolicy is the table's hit policy.
func (t *DecisionTable) HitPolicy() TablePolicy {
	if t == nil {
		return ""
	}
	return t.hitPolicy
}

// Aggregate is the table's aggregate, or "" when it has none.
func (t *DecisionTable) Aggregate() Aggregate {
	if t == nil {
		return ""
	}
	return t.aggregate
}

// Logic is the name of the table's logic.
func (t *DecisionTable) Logic() string {
	if t == nil {
		return ""
	}
	return t.logic
}

// Strict reports whether the table evaluates in strict mode.
func (t *DecisionTable) Strict() bool {
	return t != nil && t.strict
}

// Inputs are the input expressions, in column order.
func (t *DecisionTable) Inputs() []*Expr {
	if t == nil {
		return nil
	}
	return slices.Clone(t.inputs)
}

// Outputs are the output columns, in order, with their values as Go values
// (spec/data-model.md, Go, Values given back).
func (t *DecisionTable) Outputs() []TableOutput {
	if t == nil {
		return nil
	}
	outputs := make([]TableOutput, len(t.outputs))
	for i, output := range t.outputs {
		var values []any // nil: any value
		if output.Values != nil {
			values, _ = copyValue(output.Values).([]any)
		}
		outputs[i] = TableOutput{Name: output.Name, Values: values}
	}
	return outputs
}

// Len is the number of rows.
func (t *DecisionTable) Len() int {
	if t == nil {
		return 0
	}
	return t.rowCount
}

// Rows are the rows, in order, read once.
func (t *DecisionTable) Rows() ([]TableRow, error) {
	if t == nil {
		return nil, ErrClosed
	}
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.rows == nil {
		err := t.use(func(document *C.cr_document) error {
			rows, err := t.readRows(document)
			t.rows = rows
			return err
		})
		if err != nil {
			return nil, err
		}
	}
	rows := make([]TableRow, len(t.rows))
	for i, row := range t.rows {
		if row.Priority != nil {
			priority := *row.Priority
			row.Priority = &priority
		}
		row.Inputs = slices.Clone(row.Inputs)
		row.Outputs = slices.Clone(row.Outputs)
		rows[i] = row
	}
	return rows, nil
}

func (t *DecisionTable) readRows(document *C.cr_document) ([]TableRow, error) {
	columns := len(t.inputs) + len(t.outputs)
	rows := make([]TableRow, t.rowCount)
	for r := range t.rowCount {
		var out *C.cr_value
		var e *C.cr_error
		if err := check(C.cr_document_table_row(document, C.size_t(r), &out, &e), e); err != nil {
			return nil, err
		}
		row, _ := takeValue(out, nil).(map[string]any)
		cells := make([]*Expr, columns)
		for c := range columns {
			var node *C.cr_node
			if err := check(C.cr_document_table_cell(document, C.size_t(r), C.size_t(c), &node, &e), e); err != nil {
				return nil, err
			}
			if node != nil {
				cells[c] = wrapNode(node)
			}
		}
		rows[r] = TableRow{
			Name:        str(row["name"]),
			Description: str(row["description"]),
			Inputs:      cells[:len(t.inputs):len(t.inputs)],
			Outputs:     cells[len(t.inputs):],
		}
		if priority, ok := row["priority"].(int64); ok {
			rows[r].Priority = &priority
		}
	}
	return rows, nil
}

// RuleSet is the rule set the table compiles into, compiled once, on the
// first call: one rule per row, named as the row (spec/tables.md,
// Compiling). Close closes it with the table; compiled before, it is still
// given after Close, and its methods give ErrClosed.
func (t *DecisionTable) RuleSet() (*RuleSet, error) {
	if t == nil {
		return nil, ErrClosed
	}
	t.compile.Do(func() {
		t.compileErr = t.use(func(document *C.cr_document) error {
			var out *C.cr_ruleset
			var e *C.cr_error
			if err := check(C.cr_ruleset_from_document(document, &out, &e), e); err != nil {
				return err
			}
			rs, err := wrapReadRuleSet(out)
			if err == nil {
				t.ruleSet.Store(rs)
			}
			return err
		})
	})
	if t.compileErr != nil {
		return nil, t.compileErr
	}
	return t.ruleSet.Load(), nil
}

// Evaluate decides on data with the table's rule set, as RuleSet.Evaluate
// does.
func (t *DecisionTable) Evaluate(data any) (*Decision, error) {
	rs, err := t.RuleSet()
	if err != nil {
		return nil, err
	}
	return rs.Evaluate(data)
}

// EvaluateMany decides on each item of rows with the table's rule set, as
// RuleSet.EvaluateMany does.
func (t *DecisionTable) EvaluateMany(rows any) ([]*Decision, error) {
	rs, err := t.RuleSet()
	if err != nil {
		return nil, err
	}
	return rs.EvaluateMany(rows)
}

// Summary describes the table's rule set (spec/explain.md, Summaries).
func (t *DecisionTable) Summary() (*Summary, error) {
	rs, err := t.RuleSet()
	if err != nil {
		return nil, err
	}
	return rs.Summary()
}

// JSON is the table's document in the JSON format (spec/tables.md, JSON);
// Dumps writes it in the other formats.
func (t *DecisionTable) JSON() (string, error) {
	return dumps(t, FormatJSON)
}

// String describes the table, such as "DecisionTable(priority: 4 rows, 5
// columns)".
func (t *DecisionTable) String() string {
	if t == nil {
		return "DecisionTable(nil)"
	}
	return fmt.Sprintf("DecisionTable(%s: %d rows, %d columns)", t.hitPolicy, t.rowCount, len(t.inputs)+len(t.outputs))
}
