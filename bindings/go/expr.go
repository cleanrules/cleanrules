package cleanrules

/*
#include <stdlib.h>
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"math"
	"reflect"
	"time"
	"unsafe"
)

// Expr is a node of a condition or a value (spec/ast.md): a field, a
// constant, a comparison, a connective, a test or arithmetic. It never
// changes, so one may be shared between goroutines and reused in many rules.
//
// Go has no operator overloading, so nodes are built with functions and
// methods, as fuzzytool's &, | and ~ become And, Or and Not:
//
//	adult := cleanrules.Field("customer", "age").Ge(18)
//	iberian := cleanrules.Field("country").IsIn("ES", "PT")
//	cond := adult.And(iberian)
//
// The core checks each node as it is built (spec/ast.md, Construction rules).
// A node that breaks a rule does not stop the chain: it carries the error,
// which every node built on it carries too, and which NewRule returns (or Err
// gives). So an expression is built in one go and checked once.
type Expr struct {
	h   *handle
	err error
}

// Kind says where a node may appear (spec/ast.md, Node kinds).
type Kind string

// The node kinds.
const (
	KindCondition Kind = "condition"
	KindValue     Kind = "value"
	KindAny       Kind = "any"
)

// Closed says which ends of Between are included.
type Closed string

// The ends Between includes.
const (
	ClosedBoth  Closed = "both"
	ClosedLeft  Closed = "left"
	ClosedRight Closed = "right"
	ClosedNone  Closed = "none"
)

func wrapNode(ptr *C.cr_node) *Expr {
	e := &Expr{}
	e.h = newHandle(e, unsafe.Pointer(ptr), func(p unsafe.Pointer) { C.cr_node_free((*C.cr_node)(p)) })
	return e
}

func failed(err error) *Expr { return &Expr{err: err} }

// Err gives the error of the first node of the expression that could not be
// built, or nil.
func (e *Expr) Err() error {
	if e == nil || (e.h == nil && e.err == nil) {
		return errNilExpr
	}
	return e.err
}

var errNilExpr = newError(CodeExpression, "a nil or zero *Expr is not an expression", "build expressions with Field, Const and their methods")

// use runs f with the node's pointer.
func (e *Expr) use(f func(*C.cr_node) error) error {
	if err := e.Err(); err != nil {
		return err
	}
	p, err := e.h.acquire()
	if err != nil {
		return err
	}
	defer e.h.release()
	return f((*C.cr_node)(p))
}

// build makes a node from the nodes of operands, returning the first error
// among them, or the constructor's.
func build(operands []*Expr, construct func(nodes []*C.cr_node, out **C.cr_node, e **C.cr_error) C.cr_status) *Expr {
	nodes := make([]*C.cr_node, len(operands))
	for i, operand := range operands {
		if err := operand.Err(); err != nil {
			return failed(err)
		}
		p, err := operand.h.acquire()
		if err != nil {
			return failed(err)
		}
		defer operand.h.release()
		nodes[i] = (*C.cr_node)(p)
	}
	var out *C.cr_node
	var e *C.cr_error
	if err := check(construct(nodes, &out, &e), e); err != nil {
		return failed(err)
	}
	return wrapNode(out)
}

// operand is v as a node: an *Expr itself, anything else a constant.
func operand(v any) *Expr {
	if e, ok := v.(*Expr); ok {
		if e == nil {
			return failed(errNilExpr)
		}
		return e
	}
	return Const(v)
}

// Field reads the input along a path of string keys and int indexes, never
// split at dots: Field("items", 0, "price") is items[0].price, and
// Field("a.b") the key "a.b".
func Field(path ...any) *Expr {
	value, err := segments(path)
	if err != nil {
		return failed(err)
	}
	defer C.cr_value_free(value)
	var out *C.cr_node
	var e *C.cr_error
	if err := check(C.cr_node_new_field(value, &out, &e), e); err != nil {
		return failed(err)
	}
	return wrapNode(out)
}

// segments builds the list of a path; the core checks each segment.
func segments(path []any) (*C.cr_value, error) {
	if path == nil {
		path = []any{} // an empty path, which the core rejects
	}
	return newReader().read(path, wholePaths)
}

// Const is a constant (spec/ast.md, const) from a Go value, converted as data
// is (spec/data-model.md, Go): bool, integers, floats, string, Decimal, Date,
// DateTime, time.Time and nil, the null constant, which only IsNull accepts.
// A NaN, a time finer than a microsecond, an integer beyond 64 bits and
// anything that is not a scalar give an expression_error.
func Const(v any) *Expr {
	value, err := constant(v)
	if err != nil {
		return failed(err)
	}
	defer C.cr_value_free(value)
	var out *C.cr_node
	var e *C.cr_error
	if err := check(C.cr_node_new_constant(value, &out, &e), e); err != nil {
		return failed(err)
	}
	return wrapNode(out)
}

// constant converts a constant after the checks only Go can make: a value of
// the library cannot hold a NaN or a time finer than a microsecond, which
// data would read as a null or truncate (spec/data-model.md, Constants).
func constant(v any) (*C.cr_value, error) {
	inner, _ := deref(reflect.ValueOf(v))
	if inner.IsValid() {
		switch {
		case (inner.Kind() == reflect.Float32 || inner.Kind() == reflect.Float64) && math.IsNaN(inner.Float()):
			return nil, newError(CodeExpression, fmt.Sprintf("a constant cannot be NaN (%v)", inner.Interface()), "test for missing values with IsNull()")
		case inner.Type() == timeType && inner.Interface().(time.Time).Nanosecond()%1000 != 0:
			return nil, newError(CodeExpression, fmt.Sprintf("the datetime %v is finer than a microsecond", inner.Interface()), "datetimes hold microseconds; round the constant first")
		}
	}
	return newReader().read(v, wholePaths)
}

// constants converts the values of IsIn into a list, each checked as a
// constant.
func constants(values []any) (*C.cr_value, error) {
	if values == nil {
		values = []any{} // no values, which the core rejects
	}
	for _, v := range values {
		value, err := constant(v)
		if err != nil {
			return nil, err
		}
		C.cr_value_free(value)
	}
	return newReader().read(values, wholePaths)
}

// And is the conjunction of conditions (and, flattened).
func And(operands ...*Expr) *Expr {
	return build(operands, func(nodes []*C.cr_node, out **C.cr_node, e **C.cr_error) C.cr_status {
		return C.cr_node_new_and(nodePointers(nodes), C.size_t(len(nodes)), out, e)
	})
}

// Or is the disjunction of conditions (or, flattened).
func Or(operands ...*Expr) *Expr {
	return build(operands, func(nodes []*C.cr_node, out **C.cr_node, e **C.cr_error) C.cr_status {
		return C.cr_node_new_or(nodePointers(nodes), C.size_t(len(nodes)), out, e)
	})
}

// Not is the negation of a condition.
func Not(x *Expr) *Expr {
	return unary(x, func(n *C.cr_node, out **C.cr_node, e **C.cr_error) C.cr_status { return C.cr_node_new_not(n, out, e) })
}

// nodePointers gives the C ABI a Go slice of node pointers, which are C
// pointers, during the call.
func nodePointers(nodes []*C.cr_node) **C.cr_node {
	if len(nodes) == 0 {
		return nil
	}
	return &nodes[0]
}

func unary(x *Expr, construct func(*C.cr_node, **C.cr_node, **C.cr_error) C.cr_status) *Expr {
	if x == nil {
		return failed(errNilExpr)
	}
	return build([]*Expr{x}, func(nodes []*C.cr_node, out **C.cr_node, e **C.cr_error) C.cr_status {
		return construct(nodes[0], out, e)
	})
}

func (e *Expr) binary(op string, kind func(*C.char, C.size_t, *C.cr_node, *C.cr_node, **C.cr_node, **C.cr_error) C.cr_status, other any) *Expr {
	return build([]*Expr{e, operand(other)}, func(nodes []*C.cr_node, out **C.cr_node, err **C.cr_error) C.cr_status {
		p, n := text(op)
		return kind(p, n, nodes[0], nodes[1], out, err)
	})
}

func compare(p *C.char, n C.size_t, l, r *C.cr_node, out **C.cr_node, e **C.cr_error) C.cr_status {
	return C.cr_node_new_compare(p, n, l, r, out, e)
}

func arith(p *C.char, n C.size_t, l, r *C.cr_node, out **C.cr_node, e **C.cr_error) C.cr_status {
	return C.cr_node_new_arith(p, n, l, r, out, e)
}

// Eq is e == other; other is an *Expr or a constant, as for every operand.
func (e *Expr) Eq(other any) *Expr { return e.binary("eq", compare, other) }

// Ne is e != other.
func (e *Expr) Ne(other any) *Expr { return e.binary("ne", compare, other) }

// Lt is e < other.
func (e *Expr) Lt(other any) *Expr { return e.binary("lt", compare, other) }

// Le is e <= other.
func (e *Expr) Le(other any) *Expr { return e.binary("le", compare, other) }

// Gt is e > other.
func (e *Expr) Gt(other any) *Expr { return e.binary("gt", compare, other) }

// Ge is e >= other.
func (e *Expr) Ge(other any) *Expr { return e.binary("ge", compare, other) }

// Add is e + other.
func (e *Expr) Add(other any) *Expr { return e.binary("add", arith, other) }

// Sub is e - other.
func (e *Expr) Sub(other any) *Expr { return e.binary("sub", arith, other) }

// Mul is e * other.
func (e *Expr) Mul(other any) *Expr { return e.binary("mul", arith, other) }

// Div is e / other.
func (e *Expr) Div(other any) *Expr { return e.binary("div", arith, other) }

// And is the conjunction of e and others (fuzzytool's &).
func (e *Expr) And(others ...*Expr) *Expr { return And(append([]*Expr{e}, others...)...) }

// Or is the disjunction of e and others (fuzzytool's |).
func (e *Expr) Or(others ...*Expr) *Expr { return Or(append([]*Expr{e}, others...)...) }

// Not is the negation of e (fuzzytool's ~).
func (e *Expr) Not() *Expr { return Not(e) }

// Neg is -e.
func (e *Expr) Neg() *Expr {
	return unary(e, func(n *C.cr_node, out **C.cr_node, err **C.cr_error) C.cr_status {
		return C.cr_node_new_neg(n, out, err)
	})
}

// Abs is the absolute value of e.
func (e *Expr) Abs() *Expr {
	return unary(e, func(n *C.cr_node, out **C.cr_node, err **C.cr_error) C.cr_status {
		return C.cr_node_new_abs(n, out, err)
	})
}

// IsNull tests whether e is null: never unknown.
func (e *Expr) IsNull() *Expr {
	return unary(e, func(n *C.cr_node, out **C.cr_node, err **C.cr_error) C.cr_status {
		return C.cr_node_new_is_null(n, out, err)
	})
}

// IsIn tests membership in constants: IsIn("ES", "PT"), or IsIn(codes) with
// one slice of them. The core keeps them sorted and without repeats.
func (e *Expr) IsIn(values ...any) *Expr {
	if err := e.Err(); err != nil {
		return failed(err) // the first node that failed is the receiver
	}
	if len(values) == 1 {
		if v := reflect.ValueOf(values[0]); v.IsValid() && (v.Kind() == reflect.Slice || v.Kind() == reflect.Array) && !isBytes(v.Type()) {
			values = make([]any, v.Len())
			for i := range values {
				values[i] = v.Index(i).Interface()
			}
		}
	}
	list, err := constants(values)
	if err != nil {
		return failed(err)
	}
	defer C.cr_value_free(list)
	return unary(e, func(n *C.cr_node, out **C.cr_node, err **C.cr_error) C.cr_status {
		return C.cr_node_new_is_in(n, list, out, err)
	})
}

// Between tests low <= e <= high, both ends included.
func (e *Expr) Between(low, high any) *Expr { return e.BetweenClosed(low, high, ClosedBoth) }

// BetweenClosed tests that e is between low and high, with the ends that
// closed includes.
func (e *Expr) BetweenClosed(low, high any, closed Closed) *Expr {
	switch closed {
	case ClosedBoth, ClosedLeft, ClosedRight, ClosedNone:
	default:
		if err := e.Err(); err != nil {
			return failed(err)
		}
		return failed(newError(CodeExpression,
			fmt.Sprintf("closed must be 'both', 'left', 'right' or 'none', got %q", string(closed)),
			"use ClosedBoth, ClosedLeft, ClosedRight or ClosedNone"))
	}
	return build([]*Expr{e, operand(low), operand(high)}, func(nodes []*C.cr_node, out **C.cr_node, err **C.cr_error) C.cr_status {
		p, n := text(string(closed))
		return C.cr_node_new_between(nodes[0], nodes[1], nodes[2], p, n, out, err)
	})
}

// Matches searches e for a regular expression in the syntax of Rust's regex
// crate: no lookaround, no backreferences.
func (e *Expr) Matches(pattern string) *Expr {
	return unary(e, func(n *C.cr_node, out **C.cr_node, err **C.cr_error) C.cr_status {
		p, l := text(pattern)
		return C.cr_node_new_matches(n, p, l, out, err)
	})
}

func (e *Expr) stringTest(op, s string) *Expr {
	return unary(e, func(n *C.cr_node, out **C.cr_node, err **C.cr_error) C.cr_status {
		o, ol := text(op)
		p, l := text(s)
		return C.cr_node_new_string_test(o, ol, n, p, l, out, err)
	})
}

// StartsWith tests that the string e starts with prefix.
func (e *Expr) StartsWith(prefix string) *Expr { return e.stringTest("starts_with", prefix) }

// EndsWith tests that the string e ends with suffix.
func (e *Expr) EndsWith(suffix string) *Expr { return e.stringTest("ends_with", suffix) }

// Contains tests that the string e contains part.
func (e *Expr) Contains(part string) *Expr { return e.stringTest("contains", part) }

// ---- Reading a node.

// String is the node in the rule syntax (spec/text.md), as the core writes
// it, such as customer.age >= 18 AND country IN ("ES", "PT"); for an
// expression that could not be built, its error.
func (e *Expr) String() string {
	var s string
	err := e.use(func(n *C.cr_node) error {
		var out *C.char
		var length C.size_t
		var ce *C.cr_error
		if err := check(C.cr_node_write(n, C.CR_FORMAT_TEXT, &out, &length, &ce), ce); err != nil {
			return err
		}
		s = takeString(out, length)
		return nil
	})
	if err != nil {
		return "!(" + err.Error() + ")"
	}
	return s
}

// Tag is the node's tag (spec/ast.md, Catalogue), such as "gt" or "and";
// "" for an expression that could not be built.
func (e *Expr) Tag() string {
	var tag string
	_ = e.use(func(n *C.cr_node) error { tag = staticString(C.cr_node_tag(n)); return nil })
	return tag
}

// Kind says where the node may appear; "" for an expression that could not
// be built.
func (e *Expr) Kind() Kind {
	var kind Kind
	_ = e.use(func(n *C.cr_node) error { kind = Kind(staticString(C.cr_node_kind(n))); return nil })
	return kind
}

// Children are the node's direct children, in order (a comparison's left
// then right; between's operand, low and high).
func (e *Expr) Children() []*Expr {
	var children []*Expr
	_ = e.use(func(n *C.cr_node) error {
		count := int(C.cr_node_children_len(n))
		children = make([]*Expr, 0, count)
		for i := range count {
			var out *C.cr_node
			if C.cr_node_child(n, C.size_t(i), &out, nil) == C.CR_OK {
				children = append(children, wrapNode(out))
			}
		}
		return nil
	})
	return children
}

// FieldPaths are the paths the node reads, in pre-order and each once:
// string keys and int indexes.
func (e *Expr) FieldPaths() ([][]any, error) {
	var paths [][]any
	err := e.use(func(n *C.cr_node) error {
		var out *C.cr_value
		var ce *C.cr_error
		if err := check(C.cr_node_field_paths(n, &out, &ce), ce); err != nil {
			return err
		}
		paths = pathsOf(takeValue(out, nil))
		return nil
	})
	return paths, err
}

// Equal reports whether two expressions are structurally identical
// (spec/ast.md, Structural identity): the same shape, tags and constants, so
// the int 1 and the float 1.0 differ, and 0.0 and -0.0 do not. Go has no
// operator overloading, so Eq builds a node and Equal compares.
func (e *Expr) Equal(other *Expr) bool {
	same := false
	_ = e.use(func(a *C.cr_node) error {
		return other.use(func(b *C.cr_node) error {
			same = bool(C.cr_node_same(a, b))
			return nil
		})
	})
	return same
}

// Hash is a hash of the node's structure, equal for nodes that are Equal;
// stable for one version of the library, so never store it.
func (e *Expr) Hash() uint64 {
	var hash uint64
	_ = e.use(func(n *C.cr_node) error { hash = uint64(C.cr_node_hash(n)); return nil })
	return hash
}

// readNode reads a node from its JSON form, as the core wrote it (the
// condition of a cause).
func readNode(json string) (*Expr, error) {
	limits := C.cr_limits_new()
	defer C.cr_limits_free(limits)
	// The core wrote the node: it reads back, however deep.
	C.cr_limits_set(limits, C.CR_LIMIT_MAX_SIZE, C.size_t(max(len(json), 1)), nil)
	C.cr_limits_set(limits, C.CR_LIMIT_MAX_NESTING, C.size_t(math.MaxInt32), nil)
	C.cr_limits_set(limits, C.CR_LIMIT_MAX_DEPTH, C.size_t(math.MaxInt32), nil)
	p, n := text(json)
	var out *C.cr_node
	var e *C.cr_error
	if err := check(C.cr_node_read(p, n, C.CR_FORMAT_JSON, limits, &out, &e), e); err != nil {
		return nil, err
	}
	return wrapNode(out), nil
}

// pathsOf turns a list of paths given back ([]any of []any of string and
// int64) into paths of strings and ints.
func pathsOf(v any) [][]any {
	list, _ := v.([]any)
	paths := make([][]any, len(list))
	for i, item := range list {
		paths[i] = pathOfValue(item)
	}
	return paths
}

// pathOfValue turns one path given back into strings and ints.
func pathOfValue(v any) []any {
	segments, _ := v.([]any)
	path := make([]any, len(segments))
	for i, s := range segments {
		if n, ok := s.(int64); ok {
			path[i] = int(n)
		} else {
			path[i] = s
		}
	}
	return path
}
