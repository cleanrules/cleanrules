package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"slices"
	"unsafe"
)

// HitPolicy says which of the rules that fire a decision selects
// (spec/rules.md, Evaluation).
type HitPolicy string

// The hit policies.
const (
	HitPolicyFirst     HitPolicy = "first"
	HitPolicyPriority  HitPolicy = "priority"
	HitPolicyUnique    HitPolicy = "unique"
	HitPolicyAny       HitPolicy = "any"
	HitPolicyRuleOrder HitPolicy = "rule_order"
	HitPolicyCollect   HitPolicy = "collect"
)

// Aggregate combines the outputs of HitPolicyCollect (spec/rules.md, COLLECT
// aggregates).
type Aggregate string

// The aggregates.
const (
	AggregateSum   Aggregate = "sum"
	AggregateMin   Aggregate = "min"
	AggregateMax   Aggregate = "max"
	AggregateCount Aggregate = "count"
)

// RuleSet is rules evaluated together, with a hit policy, an aggregate, a
// logic and a strict mode (spec/rules.md). It never changes, so one may
// evaluate on many goroutines at once. Close frees its native memory now;
// otherwise it is freed once the rule set, every rule set Bind made of it and
// every RuleSet value are unreachable. Its decisions stay valid after Close.
type RuleSet struct {
	h         *handle
	rules     []*Rule
	hitPolicy HitPolicy
	aggregate Aggregate
	logic     string
	strict    bool
	paths     *pathTree
	actions   Actions
}

// NewRuleSet makes a rule set of rules, in order, and options. An unknown
// hit policy or aggregate, a repeated rule name or an aggregate without
// HitPolicyCollect is a rule_definition_error; an unknown logic, a registry_error.
func NewRuleSet(rules []*Rule, options ...RuleSetOption) (*RuleSet, error) {
	var settings ruleSetSettings
	for _, option := range options {
		if option == nil {
			return nil, nilOption(ruleSetOptions)
		}
		option.applyRuleSet(&settings)
	}
	pointers := make([]*C.cr_rule, len(rules))
	for i, rule := range rules {
		if rule == nil || rule.h == nil {
			return nil, errNilRule
		}
		p, err := rule.h.acquire()
		if err != nil {
			return nil, err
		}
		defer rule.h.release()
		pointers[i] = (*C.cr_rule)(p)
	}
	var first **C.cr_rule
	if len(pointers) > 0 {
		first = &pointers[0]
	}
	hp, hl := optionalText(settings.hitPolicy)
	ap, al := optionalText(settings.aggregate)
	lp, ll := optionalText(settings.logic)
	var out *C.cr_ruleset
	var e *C.cr_error
	status := C.cr_ruleset_new(first, C.size_t(len(pointers)), hp, hl, ap, al, lp, ll, C.bool(settings.strict), &out, &e)
	if err := check(status, e); err != nil {
		return nil, err
	}
	return wrapRuleSet(out, append([]*Rule(nil), rules...))
}

// optionalText is a setting by name, or NULL to leave it out.
func optionalText(name *string) (*C.char, C.size_t) {
	if name == nil {
		return nil, 0
	}
	if *name == "" {
		// An empty name is a name, which the core rejects, not NULL.
		return (*C.char)(unsafe.Pointer(&emptyName)), 0
	}
	return text(*name)
}

var emptyName = [1]byte{}

func wrapRuleSet(ptr *C.cr_ruleset, rules []*Rule) (*RuleSet, error) {
	rs := &RuleSet{
		rules:     rules,
		hitPolicy: HitPolicy(staticString(C.cr_ruleset_hit_policy(ptr))),
		aggregate: Aggregate(staticString(C.cr_ruleset_aggregate(ptr))),
		logic:     staticString(C.cr_ruleset_logic(ptr)),
		strict:    bool(C.cr_ruleset_strict(ptr)),
	}
	rs.h = newHandle(rs, unsafe.Pointer(ptr), func(p unsafe.Pointer) { C.cr_ruleset_free((*C.cr_ruleset)(p)) })
	var out *C.cr_value
	var e *C.cr_error
	if err := check(C.cr_ruleset_field_paths(ptr, &out, &e), e); err != nil {
		rs.h.close()
		return nil, err
	}
	rs.paths = newPathTree(pathsOf(takeValue(out, nil)))
	return rs, nil
}

// use runs f with the rule set's pointer, or gives ErrClosed.
func (rs *RuleSet) use(f func(*C.cr_ruleset) error) error {
	if rs == nil {
		return ErrClosed
	}
	p, err := rs.h.acquire()
	if err != nil {
		return err
	}
	defer rs.h.release()
	return f((*C.cr_ruleset)(p))
}

// Close frees the native rule set now, or when the calls using it return;
// later calls give ErrClosed. Bind's rule sets share it, so they are closed
// too. Closing twice does nothing.
func (rs *RuleSet) Close() error {
	if rs == nil {
		return nil
	}
	rs.h.close()
	return nil
}

// Len is the number of rules.
func (rs *RuleSet) Len() int {
	if rs == nil {
		return 0
	}
	return len(rs.rules)
}

// Rules are the rules, in order.
func (rs *RuleSet) Rules() []*Rule {
	if rs == nil {
		return nil
	}
	return slices.Clone(rs.rules)
}

// Rule looks a rule up by name; a name the rule set lacks is a
// rule_not_found error with the names it has.
func (rs *RuleSet) Rule(name string) (*Rule, error) {
	position, err := rs.position(name)
	if err != nil {
		return nil, err
	}
	return rs.rules[position], nil
}

func (rs *RuleSet) position(name string) (int, error) {
	var position int
	err := rs.use(func(ptr *C.cr_ruleset) error {
		p, n := text(name)
		var out C.size_t
		var e *C.cr_error
		if err := check(C.cr_ruleset_rule_position(ptr, p, n, &out, &e), e); err != nil {
			return err
		}
		position = int(out)
		return nil
	})
	return position, err
}

// HitPolicy is the rule set's hit policy.
func (rs *RuleSet) HitPolicy() HitPolicy {
	if rs == nil {
		return ""
	}
	return rs.hitPolicy
}

// Aggregate is the rule set's aggregate, or "" when it has none.
func (rs *RuleSet) Aggregate() Aggregate {
	if rs == nil {
		return ""
	}
	return rs.aggregate
}

// Logic is the name of the rule set's logic.
func (rs *RuleSet) Logic() string {
	if rs == nil {
		return ""
	}
	return rs.logic
}

// Strict reports whether the rule set is strict.
func (rs *RuleSet) Strict() bool {
	if rs == nil {
		return false
	}
	return rs.strict
}

// FieldPaths are the paths of the input that the rule set reads, in the
// order of its summary: string keys and int indexes. Evaluate reads only
// these from Go data. After Close, ErrClosed.
func (rs *RuleSet) FieldPaths() ([][]any, error) {
	var paths [][]any
	err := rs.use(func(ptr *C.cr_ruleset) error {
		var out *C.cr_value
		var e *C.cr_error
		if err := check(C.cr_ruleset_field_paths(ptr, &out, &e), e); err != nil {
			return err
		}
		paths = pathsOf(takeValue(out, nil))
		return nil
	})
	return paths, err
}

// Evaluate decides on data: Go values read as spec/data-model.md (Go) says,
// only along the paths the rule set reads. Maps with string keys, slices,
// arrays and structs are read through; a struct field is named by its
// cleanrules tag, else its json tag, else its name, and unexported fields are
// never read. In strict mode, a field that cannot be read is an error
// (missing_field, type_mismatch...); otherwise it is unknown, with a reason.
//
// What is read is counted against Limits.MaxValues, 1,000,000 values by
// default (WithLimits changes it): data beyond it is refused, strict or not,
// with a limit_exceeded error (an ErrFormat) whose Name is "max_values".
func (rs *RuleSet) Evaluate(data any, options ...EvaluateOption) (*Decision, error) {
	settings, err := evaluationSettings(options)
	if err != nil {
		return nil, err
	}
	var decision *Decision
	err = rs.use(func(ptr *C.cr_ruleset) error {
		r, err := evaluationReader(settings.limits)
		if err != nil {
			return err
		}
		input, err := r.read(data, rs.paths)
		if err != nil {
			return err
		}
		defer C.cr_value_free(input)
		var out *C.cr_decision
		var e *C.cr_error
		if err := check(C.cr_ruleset_evaluate_value(ptr, input, nil, &out, &e), e); err != nil {
			return err
		}
		decision = newDecision(rs, out, r.opaques, true)
		return nil
	})
	return decision, err
}

// EvaluateMany decides on each item of rows, a slice or an array, in order.
// The first item that fails gives the error, with its position as Row;
// rows that are not a slice or an array are an evaluation_error. Each item
// is an input of its own for Limits.MaxValues; all are read before any is
// evaluated, so the first item beyond it gives its limit_exceeded, with its
// Row, before any error of the evaluation.
func (rs *RuleSet) EvaluateMany(rows any, options ...EvaluateOption) ([]*Decision, error) {
	settings, err := evaluationSettings(options)
	if err != nil {
		return nil, err
	}
	var decisions []*Decision
	err = rs.use(func(ptr *C.cr_ruleset) error {
		r, err := evaluationReader(settings.limits)
		if err != nil {
			return err
		}
		inputs, err := r.readRows(rows, rs.paths)
		if err != nil {
			return err
		}
		defer C.cr_value_free(inputs)
		var batch *C.cr_batch
		var e *C.cr_error
		if err := check(C.cr_ruleset_evaluate_many(ptr, inputs, nil, &batch, &e), e); err != nil {
			return err
		}
		defer C.cr_batch_free(batch)
		n := int(C.cr_batch_len(batch))
		decisions = make([]*Decision, n)
		for i := range n {
			var copied *C.cr_decision
			var ce *C.cr_error
			if err := check(C.cr_decision_clone(C.cr_batch_get(batch, C.size_t(i)), &copied, &ce), ce); err != nil {
				return err
			}
			decisions[i] = newDecision(rs, copied, r.opaques, true)
		}
		return nil
	})
	return decisions, err
}

// Summary describes the rule set without evaluating it: its rules, the
// fields it reads, its outputs and tags (spec/explain.md, Summaries).
func (rs *RuleSet) Summary() (*Summary, error) {
	var summary *Summary
	err := rs.use(func(ptr *C.cr_ruleset) error {
		json, err := writeSummary(ptr, C.CR_FORMAT_JSON)
		if err != nil {
			return err
		}
		text, err := writeSummary(ptr, C.CR_FORMAT_TEXT)
		if err != nil {
			return err
		}
		summary, err = newSummary(json, text)
		return err
	})
	return summary, err
}

func writeSummary(ptr *C.cr_ruleset, format C.cr_format) (string, error) {
	var out *C.char
	var n C.size_t
	var e *C.cr_error
	if err := check(C.cr_ruleset_summary(ptr, format, &out, &n, &e), e); err != nil {
		return "", err
	}
	return takeString(out, n), nil
}
