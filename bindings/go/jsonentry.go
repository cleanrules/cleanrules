package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

// The C ABI's entry points for inputs given as JSON text, unexported: the
// conformance runner's JSON route (conformance/README.md, Runners) evaluates
// the vectors' inputs through them, as Python's private _evaluate does. The
// core reads the JSON (spec/json-format.md, Values) and evaluates; nothing
// here reads or decides anything in Go. Programs give Go data to Evaluate.

// evaluateJSON decides on an input given as JSON text, read by the core
// within its default limits.
func (rs *RuleSet) evaluateJSON(input string) (*Decision, error) {
	var decision *Decision
	err := rs.use(func(ptr *C.cr_ruleset) error {
		p, n := text(input)
		var out *C.cr_decision
		var e *C.cr_error
		if err := check(C.cr_ruleset_evaluate(ptr, p, n, nil, &out, &e), e); err != nil {
			return err
		}
		decision = newDecision(rs, out, nil, true)
		return nil
	})
	return decision, err
}

// evaluateManyJSON decides on each input of a batch given as JSON text.
func (rs *RuleSet) evaluateManyJSON(inputs string) ([]*Decision, error) {
	var decisions []*Decision
	err := rs.use(func(ptr *C.cr_ruleset) error {
		list, err := valueFromJSON(inputs)
		if err != nil {
			return err
		}
		defer C.cr_value_free(list)
		decisions, err = rs.evaluateBatch(ptr, list, nil)
		return err
	})
	return decisions, err
}

// evaluateTreeJSON evaluates the node as a condition on an input given as
// JSON text, under logic ("" for the default) and strict or not: its result
// tree in the JSON format (spec/json-format.md, Result trees).
func (e *Expr) evaluateTreeJSON(input, logic string, strict bool) (string, error) {
	return e.treeJSON(input, func(node *C.cr_node, value *C.cr_value, out **C.char, n *C.size_t, ce **C.cr_error) C.cr_status {
		var name *string
		if logic != "" {
			name = &logic
		}
		lp, ll := optionalText(name)
		return C.cr_node_evaluate_tree(node, value, lp, ll, C.bool(strict), out, n, ce)
	})
}

// evaluateValueTreeJSON evaluates the node as a value on an input given as
// JSON text: its result tree in the JSON format.
func (e *Expr) evaluateValueTreeJSON(input string, strict bool) (string, error) {
	return e.treeJSON(input, func(node *C.cr_node, value *C.cr_value, out **C.char, n *C.size_t, ce **C.cr_error) C.cr_status {
		return C.cr_node_evaluate_value_tree(node, value, C.bool(strict), out, n, ce)
	})
}

func (e *Expr) treeJSON(input string, evaluate func(*C.cr_node, *C.cr_value, **C.char, *C.size_t, **C.cr_error) C.cr_status) (string, error) {
	var result string
	err := e.use(func(node *C.cr_node) error {
		value, err := valueFromJSON(input)
		if err != nil {
			return err
		}
		defer C.cr_value_free(value)
		var out *C.char
		var n C.size_t
		var ce *C.cr_error
		if err := check(evaluate(node, value, &out, &n, &ce), ce); err != nil {
			return err
		}
		result = takeString(out, n)
		return nil
	})
	return result, err
}

// valueFromJSON reads a data value from JSON text, within the default
// limits; the caller frees it.
func valueFromJSON(input string) (*C.cr_value, error) {
	p, n := text(input)
	var value *C.cr_value
	var e *C.cr_error
	if err := check(C.cr_value_from_json(p, n, nil, &value, &e), e); err != nil {
		return nil, err
	}
	return value, nil
}
