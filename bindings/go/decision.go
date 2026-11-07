package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"sync"
	"unsafe"
)

// Truth is a truth value of Kleene's logic.
type Truth int

// The truth values; the zero Truth is TruthUnknown.
const (
	TruthUnknown Truth = iota
	TruthTrue
	TruthFalse
)

// MarshalText writes the truth value as String does, so encoding/json
// writes "true", "false" or "unknown".
func (t Truth) MarshalText() ([]byte, error) { return []byte(t.String()), nil }

// UnmarshalText reads "true", "false" or "unknown".
func (t *Truth) UnmarshalText(text []byte) error {
	for _, truth := range []Truth{TruthTrue, TruthFalse, TruthUnknown} {
		if string(text) == truth.String() {
			*t = truth
			return nil
		}
	}
	return fmt.Errorf("cleanrules: %q is not a truth value (true, false or unknown)", text)
}

// String gives "true", "false" or "unknown".
func (t Truth) String() string {
	switch t {
	case TruthTrue:
		return "true"
	case TruthFalse:
		return "false"
	default:
		return "unknown"
	}
}

func truthOf(t C.cr_truth) Truth {
	switch t {
	case C.CR_TRUTH_TRUE:
		return TruthTrue
	case C.CR_TRUTH_FALSE:
		return TruthFalse
	default:
		return TruthUnknown
	}
}

// RuleResult is what one rule gave in a decision.
type RuleResult struct {
	// Rule is the rule.
	Rule *Rule
	// Fired reports whether its condition passed the logic's activation test
	// (true, under Kleene's logic).
	Fired bool
	// Truth is its condition's truth value.
	Truth Truth
	// Output is its payload's keys and values, when the payload was
	// evaluated (spec/rules.md, When payloads are evaluated), and nil
	// otherwise.
	Output map[string]any
}

// Decision is what a rule set made of one input (spec/rules.md): every
// rule's result, the rules the hit policy selected and the outputs, as Go
// values (spec/data-model.md, Go, Values given back), its explanation and
// its JSON document. It never changes (its methods give copies), so one may
// be read on many goroutines at once. Close frees its native memory now;
// otherwise it is freed once the decision is unreachable. It keeps the
// native rule set it came from, so it outlives RuleSet.Close.
type Decision struct {
	h         *handle
	ruleSet   *RuleSet
	opaques   []any
	results   []RuleResult
	selected  []int
	outputs   []map[string]any
	output    map[string]any
	outputErr error

	explained   sync.Once
	explanation *Explanation
	explainErr  error
}

// newDecision reads a decision of the library, owned (freed with the
// decision) or borrowed (an action's, adopted when the run ends).
func newDecision(rs *RuleSet, ptr *C.cr_decision, opaques []any, owned bool) *Decision {
	d := &Decision{ruleSet: rs, opaques: opaques}
	// The values a decision keeps mark their opaque values, so that the
	// copies its methods give copy the containers of cleanrules and give
	// the native objects themselves (copyValue).
	marked := make([]any, len(opaques))
	for i, object := range opaques {
		marked[i] = opaqueRef{object}
	}
	var free func(unsafe.Pointer)
	if owned {
		free = freeDecision
	}
	d.h = newHandle(d, unsafe.Pointer(ptr), free)
	// What a decision says is kept when it is made, so reading it does not
	// evaluate again (cleanrules.h, cr_decision).
	n := int(C.cr_decision_len(ptr))
	d.results = make([]RuleResult, n)
	for i := range n {
		var fired C.bool
		var truth C.cr_truth
		var output *C.cr_value
		C.cr_decision_fired(ptr, C.size_t(i), &fired, nil)
		C.cr_decision_truth(ptr, C.size_t(i), &truth, nil)
		result := RuleResult{Fired: bool(fired), Truth: truthOf(truth)}
		if i < len(rs.rules) {
			result.Rule = rs.rules[i]
		}
		if C.cr_decision_rule_output(ptr, C.size_t(i), &output, nil) == C.CR_OK {
			result.Output, _ = takeValue(output, marked).(map[string]any)
		}
		d.results[i] = result
	}
	for i := range int(C.cr_decision_selected_len(ptr)) {
		var position C.size_t
		if C.cr_decision_selected(ptr, C.size_t(i), &position, nil) == C.CR_OK {
			d.selected = append(d.selected, int(position))
		}
	}
	var outputs *C.cr_value
	if C.cr_decision_outputs_value(ptr, &outputs, nil) == C.CR_OK {
		for _, item := range takeValue(outputs, marked).([]any) {
			m, _ := item.(map[string]any)
			d.outputs = append(d.outputs, m)
		}
	}
	var output *C.cr_value
	var e *C.cr_error
	if err := check(C.cr_decision_output_value(ptr, &output, &e), e); err != nil {
		d.outputErr = err
	} else {
		d.output, _ = takeValue(output, marked).(map[string]any)
	}
	return d
}

func freeDecision(p unsafe.Pointer) { C.cr_decision_free((*C.cr_decision)(p)) }

// Close frees the native decision now, or when the calls using it return.
// What the decision says stays readable, and so does an explanation made
// before; Explain (if not called before) and JSON give ErrClosed. Closing
// twice does nothing.
func (d *Decision) Close() error {
	if d != nil {
		d.h.close()
	}
	return nil
}

// RuleSet is the rule set that decided.
func (d *Decision) RuleSet() *RuleSet {
	if d == nil {
		return nil
	}
	return d.ruleSet
}

// HitPolicy is the hit policy that decided.
func (d *Decision) HitPolicy() HitPolicy { return d.RuleSet().HitPolicy() }

// Results are every rule's result, in rule order. They, and every map and
// slice the decision gives, are copies: changing them changes nothing else.
func (d *Decision) Results() []RuleResult {
	if d == nil {
		return nil
	}
	results := make([]RuleResult, len(d.results))
	for i, result := range d.results {
		results[i] = result.copied()
	}
	return results
}

func (r RuleResult) copied() RuleResult {
	r.Output = copyMap(r.Output)
	return r
}

// Selected are the results of the rules the hit policy selected, in rule
// order.
func (d *Decision) Selected() []RuleResult {
	if d == nil {
		return nil
	}
	selected := make([]RuleResult, len(d.selected))
	for i, position := range d.selected {
		selected[i] = d.results[position].copied()
	}
	return selected
}

// Fired are the names of the rules that fired, in rule order, selected or
// not.
func (d *Decision) Fired() []string {
	if d == nil {
		return nil
	}
	var names []string
	for _, result := range d.results {
		if result.Fired {
			names = append(names, result.Rule.Name())
		}
	}
	return names
}

// Outputs are the payloads of the selected rules, in rule order; an empty
// map for a rule without one.
func (d *Decision) Outputs() []map[string]any {
	if d == nil {
		return nil
	}
	outputs := make([]map[string]any, len(d.outputs))
	for i, output := range d.outputs {
		outputs[i] = copyMap(output)
	}
	return outputs
}

// Output is the single output: the selected rule's payload, or the
// aggregate; nil when no rule was selected. HitPolicyRuleOrder, and
// HitPolicyCollect without an aggregate, have several outputs (read
// Outputs): a hit_policy_error.
func (d *Decision) Output() (map[string]any, error) {
	if d == nil {
		return nil, ErrClosed
	}
	return copyMap(d.output), d.outputErr
}

// Aggregated is, with HitPolicyCollect and an aggregate, the aggregated
// values; nil without an aggregate or when no rule was selected.
func (d *Decision) Aggregated() map[string]any {
	if d == nil || d.ruleSet.aggregate == "" {
		return nil
	}
	return copyMap(d.output)
}

// JSON is the decision's document in the JSON format (spec/json-format.md,
// Decisions), which the explanation's Cause.At points into. After Close,
// ErrClosed.
func (d *Decision) JSON() (string, error) {
	var text string
	err := d.use(func(ptr *C.cr_decision) error {
		var out *C.char
		var n C.size_t
		var e *C.cr_error
		if err := check(C.cr_decision_write(ptr, C.CR_FORMAT_JSON, &out, &n, &e), e); err != nil {
			return err
		}
		text = takeString(out, n)
		return nil
	})
	return text, err
}

func (d *Decision) use(f func(*C.cr_decision) error) error {
	if d == nil {
		return ErrClosed
	}
	p, err := d.h.acquire()
	if err != nil {
		return err
	}
	defer d.h.release()
	return f((*C.cr_decision)(p))
}

// Explain explains every rule: selected, fired but not selected, not fired
// or unknown, with what it needed, the values read and counterfactuals
// (spec/explain.md). The explanation is made once and kept.
func (d *Decision) Explain() (*Explanation, error) {
	if d == nil {
		return nil, ErrClosed
	}
	d.explained.Do(func() {
		d.explanation, d.explainErr = d.explain()
	})
	return d.explanation, d.explainErr
}

func (d *Decision) explain() (*Explanation, error) {
	var explanation *Explanation
	err := d.use(func(ptr *C.cr_decision) error {
		var value *C.cr_value
		var e *C.cr_error
		if err := check(C.cr_decision_explain_value(ptr, &value, &e), e); err != nil {
			return err
		}
		data, _ := takeValue(value, d.opaques).(map[string]any)
		texts := [2]string{}
		for i, format := range []C.cr_format{C.CR_FORMAT_TEXT, C.CR_FORMAT_JSON} {
			var out *C.char
			var n C.size_t
			if err := check(C.cr_decision_explain(ptr, format, &out, &n, &e), e); err != nil {
				return err
			}
			texts[i] = takeString(out, n)
		}
		explanation = newExplanation(data, texts[0], texts[1])
		return nil
	})
	return explanation, err
}

// WhyNot is the explanation of one rule, by name: what it needed and why it
// did not fire (or was not selected); Explain().ForRule(name). It needs no
// call to the rule set, so it works after the rule set is closed. A name the
// rule set lacks is a rule_not_found error.
func (d *Decision) WhyNot(name string) (*RuleExplanation, error) {
	explanation, err := d.Explain()
	if err != nil {
		return nil, err
	}
	return explanation.ForRule(name)
}

// copyMap copies a map given back, and the maps and slices inside it.
func copyMap(m map[string]any) map[string]any {
	if m == nil {
		return nil
	}
	out := make(map[string]any, len(m))
	for k, v := range m {
		out[k] = copyValue(v)
	}
	return out
}

// opaqueRef marks a native object among the values a decision keeps.
type opaqueRef struct{ object any }

func copyValue(v any) any {
	switch v := v.(type) {
	case opaqueRef:
		return v.object
	case map[string]any:
		return copyMap(v)
	case []any:
		out := make([]any, len(v))
		for i, item := range v {
			out[i] = copyValue(item)
		}
		return out
	}
	return v
}
