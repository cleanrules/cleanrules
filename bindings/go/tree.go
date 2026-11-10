package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"strconv"
	"sync"
	"unsafe"
)

// ResultTree is what evaluating a node gave, and each child's result
// (spec/ast.md, Evaluating one node; spec/json-format.md, Result trees): the
// node's tag and, for a field, its path; a truth value for a condition, or
// a value for any other node; why it is unknown or null; and its children's
// results, in the order of the node's children (a between gives its two
// halves, the comparisons it made). The tree is the caller's, as are its
// slices.
type ResultTree struct {
	// Tag is the tag of the node evaluated, such as "gt" or "field" (the
	// "op" of its JSON form, as Expr.Tag gives it).
	Tag string
	// Path is a field's path: string keys and int indexes; nil for other
	// nodes.
	Path []any
	// Truth is the truth value of a condition, and of a field or constant in
	// a condition position (a rule's when, an operand of And, Or or Not); nil
	// for a value.
	Truth *Truth
	// Value is the value of a node that is not a condition, as Go values are
	// given back (spec/data-model.md, Go); nil for a condition, and for a
	// null.
	Value any
	// Reason says why a truth value is unknown or a value null; nil
	// otherwise.
	Reason *Reason
	// Children are the children's results, in order.
	Children []*ResultTree

	shared *treeShared
	index  int   // in pre-order, for cr_tree_node
	route  []int // the children's positions from the root, for JSON
}

// treeShared holds the native tree and its value, which every result of
// the tree refers to: freed once no result is reachable.
type treeShared struct {
	tree, value *handle
}

// Node is the node evaluated (the halves of a between are the comparisons
// the evaluation made).
func (t *ResultTree) Node() *Expr {
	if t == nil || t.shared == nil {
		return failed(errNilTree)
	}
	p, err := t.shared.tree.acquire()
	if err != nil {
		return failed(err)
	}
	defer t.shared.tree.release()
	var out *C.cr_node
	var e *C.cr_error
	if err := check(C.cr_tree_node((*C.cr_tree)(p), C.size_t(t.index), &out, &e), e); err != nil {
		return failed(err)
	}
	return wrapNode(out)
}

var errNilTree = newError(CodeExpression, "a nil or zero *ResultTree has no node", "make result trees with Expr.EvaluateTree")

// JSON is the result tree from this result down in the JSON format
// (spec/json-format.md, Result trees), as the core writes it; "" for a nil
// or zero ResultTree.
func (t *ResultTree) JSON() string {
	if t == nil || t.shared == nil {
		return ""
	}
	p, err := t.shared.value.acquire()
	if err != nil {
		return ""
	}
	defer t.shared.value.release()
	found := (*C.cr_value)(p)
	children := "children"
	for _, step := range t.route {
		k, n := text(children)
		found = C.cr_value_list_get(C.cr_value_map_get(found, k, n), C.size_t(step))
	}
	var out *C.char
	var n C.size_t
	var e *C.cr_error
	if err := check(C.cr_value_to_json(found, &out, &n, &e), e); err != nil {
		// The core wrote the tree, so it writes back: a bug otherwise.
		return ""
	}
	return takeString(out, n)
}

// String describes the result, such as "x > 1: true" or "x: 5".
func (t *ResultTree) String() string {
	if t == nil {
		return "<nil>"
	}
	outcome := fmt.Sprint(t.Value)
	switch {
	case t.Truth != nil:
		outcome = t.Truth.String()
	case t.Value == nil:
		outcome = "null"
	default:
		if s, ok := t.Value.(string); ok {
			outcome = strconv.Quote(s)
		}
	}
	return t.Node().String() + ": " + outcome
}

// newResultTree reads a native tree, which it owns from here; opaques are
// the native objects of the evaluation, by token.
func newResultTree(tree *C.cr_tree, opaques []any) (*ResultTree, error) {
	var value *C.cr_value
	var e *C.cr_error
	if err := check(C.cr_tree_value(tree, &value, &e), e); err != nil {
		C.cr_tree_free(tree)
		return nil, err
	}
	shared := &treeShared{}
	shared.tree = newHandle(shared, unsafe.Pointer(tree), func(p unsafe.Pointer) { C.cr_tree_free((*C.cr_tree)(p)) })
	shared.value = newHandle(shared, unsafe.Pointer(value), func(p unsafe.Pointer) { C.cr_value_free((*C.cr_value)(p)) })
	root, _ := goValue(value, opaques).(map[string]any)
	next := 0
	return readResult(shared, root, nil, &next), nil
}

// readResult reads one result of the tree's value and its children, counting
// the results in pre-order.
func readResult(shared *treeShared, m map[string]any, route []int, next *int) *ResultTree {
	t := &ResultTree{Tag: str(m["op"]), Reason: reasonOf(m["reason"]), shared: shared, index: *next, route: route}
	*next++
	if path, ok := m["path"]; ok {
		t.Path = pathOfValue(path)
	}
	if truth, ok := m["truth"]; ok {
		value := truthValue(truth)
		t.Truth = &value
	} else {
		t.Value = m["value"]
	}
	children, _ := m["children"].([]any)
	for i, child := range children {
		m, _ := child.(map[string]any)
		t.Children = append(t.Children, readResult(shared, m, append(route[:len(route):len(route)], i), next))
	}
	return t
}

// EvaluateTree evaluates the node as a condition on data (spec/ast.md,
// Evaluating one node), under the logic of WithLogic ("boolean" by
// default), strict or not (WithStrict): its result tree, whose root holds
// the truth value. Data is read as RuleSet.Evaluate reads it, along the
// paths the node reads, within Limits.MaxValues (WithLimits). A value node,
// such as Field("a").Add(1), is an expression_error; an unknown logic a
// registry_error.
func (e *Expr) EvaluateTree(data any, options ...TreeOption) (*ResultTree, error) {
	var settings treeSettings
	for _, option := range options {
		if option == nil {
			return nil, nilOption(treeOptions)
		}
		option.applyTree(&settings)
	}
	return e.evaluateTree(data, settings, true)
}

// EvaluateValueTree evaluates the node as a value on data, strict or not
// (WithStrict), read within Limits.MaxValues (WithLimits): its result tree,
// whose root holds the value. A condition,
// such as Field("a").Gt(1), is an expression_error.
func (e *Expr) EvaluateValueTree(data any, options ...ValueTreeOption) (*ResultTree, error) {
	var settings treeSettings
	for _, option := range options {
		if option == nil {
			return nil, nilOption(valueTreeOptions)
		}
		option.applyValueTree(&settings)
	}
	return e.evaluateTree(data, settings, false)
}

func (e *Expr) evaluateTree(data any, settings treeSettings, condition bool) (*ResultTree, error) {
	paths, err := e.FieldPaths()
	if err != nil {
		return nil, err
	}
	var result *ResultTree
	err = e.use(func(node *C.cr_node) error {
		r, err := evaluationReader(settings.limits)
		if err != nil {
			return err
		}
		input, err := r.read(data, newPathTree(paths))
		if err != nil {
			return err
		}
		defer C.cr_value_free(input)
		var tree *C.cr_tree
		var ce *C.cr_error
		var status C.cr_status
		if condition {
			lp, ll := optionalText(settings.logic)
			status = C.cr_tree_new_condition(node, input, lp, ll, C.bool(settings.strict), &tree, &ce)
		} else {
			status = C.cr_tree_new_value(node, input, C.bool(settings.strict), &tree, &ce)
		}
		if err := check(status, ce); err != nil {
			return err
		}
		result, err = newResultTree(tree, r.opaques)
		return err
	})
	return result, err
}

// actionTree is the result tree of an action's rule, made once.
type actionTree struct {
	once sync.Once
	tree *ResultTree
	err  error
}

// Tree is the result tree of the rule's condition in the decision (as its
// explanation reads it), made once. After the decision is closed, if it was
// not made before, ErrClosed.
func (c *ActionContext) Tree() (*ResultTree, error) {
	c.tree.once.Do(func() {
		c.tree.err = c.decision.use(func(ptr *C.cr_decision) error {
			var tree *C.cr_tree
			var e *C.cr_error
			if err := check(C.cr_decision_rule_tree(ptr, C.size_t(c.position), &tree, &e), e); err != nil {
				return err
			}
			var err error
			c.tree.tree, err = newResultTree(tree, c.decision.opaques)
			return err
		})
	})
	return c.tree.tree, c.tree.err
}
