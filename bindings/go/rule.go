package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"unsafe"
)

// Rule is a named condition with a payload and descriptive data
// (spec/rules.md). It never changes: With gives a new rule. One may be shared
// between goroutines and rule sets.
type Rule struct {
	h    *handle
	name string
}

// RuleOption sets a part of a rule in NewRule and Rule.With: Then,
// WithPriority, WithTags, WithDescription, WithMetadata and WithAction.
type RuleOption struct {
	apply func(rule *C.cr_rule, out **C.cr_rule, e **C.cr_error) error
}

// NewRule makes a rule: a name, its condition and the options, applied in
// order. The core checks each part, so the error is the first of: the
// condition's own (Expr.Err), the rule's, then each option's.
//
//	vip, err := cleanrules.NewRule("vip",
//		cleanrules.Field("customer", "years").Gt(5),
//		cleanrules.Then("discount", cleanrules.MustDecimal("0.15")),
//		cleanrules.WithPriority(10))
func NewRule(name string, when *Expr, options ...RuleOption) (*Rule, error) {
	var out *C.cr_rule
	err := when.use(func(node *C.cr_node) error {
		p, n := text(name)
		var e *C.cr_error
		return check(C.cr_rule_new(p, n, node, &out, &e), e)
	})
	if err != nil {
		return nil, err
	}
	return applyOptions(out, true, options)
}

// With gives this rule with options applied in order, such as a new payload
// entry or another priority; r does not change.
func (r *Rule) With(options ...RuleOption) (*Rule, error) {
	if len(options) == 0 && r != nil {
		return r, nil
	}
	var made *Rule
	err := r.use(func(rule *C.cr_rule) error {
		var err error
		made, err = applyOptions(rule, false, options)
		return err
	})
	return made, err
}

// applyOptions applies options to rule, freeing the intermediate rules (and
// rule itself when owned), and wraps the last one.
func applyOptions(rule *C.cr_rule, owned bool, options []RuleOption) (*Rule, error) {
	current := rule
	for _, option := range options {
		if option.apply == nil {
			if owned {
				C.cr_rule_free(current)
			}
			return nil, errZeroOption
		}
		var next *C.cr_rule
		var e *C.cr_error
		err := option.apply(current, &next, &e)
		if owned {
			C.cr_rule_free(current)
		}
		if err != nil {
			return nil, err
		}
		current, owned = next, true
	}
	return wrapRule(current), nil
}

func wrapRule(ptr *C.cr_rule) *Rule {
	r := &Rule{}
	var name *C.char
	var n C.size_t
	C.cr_rule_name(ptr, &name, &n, nil)
	r.name = goString(name, n)
	r.h = newHandle(r, unsafe.Pointer(ptr), func(p unsafe.Pointer) { C.cr_rule_free((*C.cr_rule)(p)) })
	return r
}

func (r *Rule) use(f func(*C.cr_rule) error) error {
	if r == nil || r.h == nil {
		return errNilRule
	}
	p, err := r.h.acquire()
	if err != nil {
		return err
	}
	defer r.h.release()
	return f((*C.cr_rule)(p))
}

var errNilRule = newError(CodeRuleDefinition, "a nil or zero *Rule is not a rule", "make rules with NewRule")

// errZeroOption is the error of a zero RuleOption or RuleSetOption, which
// sets nothing.
var errZeroOption = newError(CodeRuleDefinition, "a zero option sets nothing", "make options with Then, WithPriority, WithHitPolicy and the other functions of the package")

// Then adds key to the payload with value: an *Expr (a value node, such as
// Field("order", "total").Mul(0.9)) or a constant. A new key goes last; a key
// the payload has keeps its place and takes the new value.
func Then(key string, value any) RuleOption {
	node := operand(value)
	return RuleOption{func(rule *C.cr_rule, out **C.cr_rule, e **C.cr_error) error {
		return node.use(func(n *C.cr_node) error {
			p, l := text(key)
			return check(C.cr_rule_with_payload(rule, p, l, n, out, e), *e)
		})
	}}
}

// WithPriority sets the priority, for HitPolicyPriority.
func WithPriority(priority int64) RuleOption {
	return RuleOption{func(rule *C.cr_rule, out **C.cr_rule, e **C.cr_error) error {
		return check(C.cr_rule_with_priority(rule, C.int64_t(priority), out, e), *e)
	}}
}

// WithTags sets the tags, which the rule keeps sorted and without repeats.
func WithTags(tags ...string) RuleOption {
	return RuleOption{func(rule *C.cr_rule, out **C.cr_rule, e **C.cr_error) error {
		list, err := newReader().read(tags, wholePaths)
		if err != nil {
			return err
		}
		defer C.cr_value_free(list)
		return check(C.cr_rule_with_tags(rule, list, out, e), *e)
	}}
}

// WithDescription sets the description.
func WithDescription(description string) RuleOption {
	return RuleOption{func(rule *C.cr_rule, out **C.cr_rule, e **C.cr_error) error {
		p, n := text(description)
		return check(C.cr_rule_with_description(rule, p, n, out, e), *e)
	}}
}

// WithMetadata sets the metadata: data values, read as data is (a Go map is
// sorted by key). An opaque value in it is a rule_definition_error.
func WithMetadata(metadata map[string]any) RuleOption {
	return RuleOption{func(rule *C.cr_rule, out **C.cr_rule, e **C.cr_error) error {
		value, err := newReader().read(metadata, wholePaths)
		if err != nil {
			return err
		}
		if C.cr_value_kind(value) != C.CR_VALUE_MAP {
			// A nil map: no metadata.
			C.cr_value_free(value)
			value = C.cr_value_new_map()
		}
		defer C.cr_value_free(value)
		return check(C.cr_rule_with_metadata(rule, value, out, e), *e)
	}}
}

// WithAction names the rule's action, which RuleSet.Bind resolves in a
// registry of Go functions (spec/rules.md, Actions).
func WithAction(name string) RuleOption {
	return RuleOption{func(rule *C.cr_rule, out **C.cr_rule, e **C.cr_error) error {
		p, n := text(name)
		return check(C.cr_rule_with_action(rule, p, n, out, e), *e)
	}}
}

// ---- Reading a rule.

// Name is the rule's name.
func (r *Rule) Name() string {
	if r == nil {
		return ""
	}
	return r.name
}

// When is the rule's condition.
func (r *Rule) When() *Expr {
	var when *Expr
	err := r.use(func(rule *C.cr_rule) error {
		var out *C.cr_node
		var e *C.cr_error
		if err := check(C.cr_rule_when(rule, &out, &e), e); err != nil {
			return err
		}
		when = wrapNode(out)
		return nil
	})
	if err != nil {
		return failed(err)
	}
	return when
}

// PayloadEntry is one entry of a rule's payload: its key and value node.
type PayloadEntry struct {
	Key   string
	Value *Expr
}

// Payload is the rule's payload, in order.
func (r *Rule) Payload() []PayloadEntry {
	var payload []PayloadEntry
	_ = r.use(func(rule *C.cr_rule) error {
		n := int(C.cr_rule_payload_len(rule))
		payload = make([]PayloadEntry, 0, n)
		for i := range n {
			var key *C.char
			var length C.size_t
			var node *C.cr_node
			if C.cr_rule_payload_key(rule, C.size_t(i), &key, &length, nil) != C.CR_OK ||
				C.cr_rule_payload_value(rule, C.size_t(i), &node, nil) != C.CR_OK {
				continue
			}
			payload = append(payload, PayloadEntry{goString(key, length), wrapNode(node)})
		}
		return nil
	})
	return payload
}

// Priority is the rule's priority, 0 by default.
func (r *Rule) Priority() int64 {
	var priority int64
	_ = r.use(func(rule *C.cr_rule) error { priority = int64(C.cr_rule_priority(rule)); return nil })
	return priority
}

// Tags are the rule's tags, sorted and without repeats.
func (r *Rule) Tags() []string {
	var tags []string
	_ = r.use(func(rule *C.cr_rule) error {
		var out *C.cr_value
		if C.cr_rule_tags(rule, &out, nil) != C.CR_OK {
			return nil
		}
		for _, tag := range takeValue(out, nil).([]any) {
			tags = append(tags, tag.(string))
		}
		return nil
	})
	return tags
}

// Description is the rule's description, or "".
func (r *Rule) Description() string {
	var description string
	_ = r.use(func(rule *C.cr_rule) error {
		var p *C.char
		var n C.size_t
		C.cr_rule_description(rule, &p, &n, nil)
		description = goString(p, n)
		return nil
	})
	return description
}

// ActionName is the name of the rule's action, or "".
func (r *Rule) ActionName() string {
	var name string
	_ = r.use(func(rule *C.cr_rule) error {
		var p *C.char
		var n C.size_t
		C.cr_rule_action(rule, &p, &n, nil)
		name = goString(p, n)
		return nil
	})
	return name
}

// Metadata is the rule's metadata, as Go values (spec/data-model.md, Go,
// Values given back).
func (r *Rule) Metadata() map[string]any {
	var metadata map[string]any
	_ = r.use(func(rule *C.cr_rule) error {
		var out *C.cr_value
		if C.cr_rule_metadata(rule, &out, nil) == C.CR_OK {
			metadata, _ = takeValue(out, nil).(map[string]any)
		}
		return nil
	})
	return metadata
}
