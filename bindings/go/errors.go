package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"bytes"
	"encoding/json"
	"fmt"
	"slices"
	"strconv"
	"strings"
)

// Code is an error's stable code (spec/errors.md, Codes), the same in every
// language.
type Code string

// The error codes of spec/errors.md, and the two statuses of the C ABI for a
// misuse and for a bug in cleanrules.
const (
	CodeFormat             Code = "format_error"
	CodeUnsupportedVersion Code = "unsupported_version"
	CodeLimitExceeded      Code = "limit_exceeded"
	CodeDecisionMismatch   Code = "decision_mismatch"
	CodeExpression         Code = "expression_error"
	CodeRuleDefinition     Code = "rule_definition_error"
	CodeRuleSyntax         Code = "rule_syntax_error"
	CodeRegistry           Code = "registry_error"
	CodeRuleNotFound       Code = "rule_not_found"
	CodeEvaluation         Code = "evaluation_error"
	CodeMissingField       Code = "missing_field"
	CodeTypeMismatch       Code = "type_mismatch"
	CodeHitPolicy          Code = "hit_policy_error"
	CodeAction             Code = "action_error"
	CodeUnsupported        Code = "unsupported"
	// CodeInvalidArgument is an argument the C ABI refuses. The package
	// checks what it is given first, so this means a bug in it: please
	// report it.
	CodeInvalidArgument Code = "invalid_argument"
	// CodeInternal is a bug in cleanrules: please report it.
	CodeInternal Code = "internal_error"
)

// A sentinel stands for one code in errors.Is.
type sentinel struct{ code Code }

func (s *sentinel) Error() string { return "cleanrules: " + string(s.code) }

// Sentinels for errors.Is, one per code. An *Error matches the sentinel of its
// own code and those of the code's ancestors in the hierarchy of
// spec/errors.md, so errors.Is(err, ErrEvaluation) holds for a missing_field
// error, and errors.Is(err, ErrFormat) for a limit_exceeded one.
var (
	ErrFormat             error = &sentinel{CodeFormat}
	ErrUnsupportedVersion error = &sentinel{CodeUnsupportedVersion}
	ErrLimitExceeded      error = &sentinel{CodeLimitExceeded}
	ErrDecisionMismatch   error = &sentinel{CodeDecisionMismatch}
	ErrExpression         error = &sentinel{CodeExpression}
	ErrRuleDefinition     error = &sentinel{CodeRuleDefinition}
	ErrRuleSyntax         error = &sentinel{CodeRuleSyntax}
	ErrRegistry           error = &sentinel{CodeRegistry}
	ErrRuleNotFound       error = &sentinel{CodeRuleNotFound}
	ErrEvaluation         error = &sentinel{CodeEvaluation}
	ErrMissingField       error = &sentinel{CodeMissingField}
	ErrTypeMismatch       error = &sentinel{CodeTypeMismatch}
	ErrHitPolicy          error = &sentinel{CodeHitPolicy}
	ErrAction             error = &sentinel{CodeAction}
	ErrUnsupported        error = &sentinel{CodeUnsupported}
	ErrInvalidArgument    error = &sentinel{CodeInvalidArgument}
	ErrInternal           error = &sentinel{CodeInternal}
)

// Error is every error of cleanrules (spec/errors.md): its code, the message
// and hint the core wrote, and its context, with the fields that do not
// apply left empty. Test for a code with errors.Is and a sentinel, or read
// the fields after errors.As:
//
//	var e *cleanrules.Error
//	if errors.As(err, &e) && e.Code == cleanrules.CodeMissingField {
//		fmt.Println(e.Rule, e.Path)
//	}
type Error struct {
	// Code is the error's code.
	Code Code
	// Message says what failed, naming the rule or node, and Hint how to fix
	// it ("" when there is none).
	Message, Hint string
	// Pointer is a JSON Pointer into a document, nil when it does not apply
	// ("" is the document's root, so the empty string cannot say that).
	Pointer *string
	// Line and Column (1-based, in code points) are a place in a text; 0
	// when they do not apply.
	Line, Column int
	// Rule is the rule that failed; Rules, for a hit_policy_error, the rules
	// involved; Executed, for an action_error, the rules whose actions ran.
	Rule     string
	Rules    []string
	Executed []string
	// Path is the full path of the field being read: string keys and int
	// indexes.
	Path []any
	// Types are the type groups of a type_mismatch.
	Types []string
	// Row is the position of the input in a batch, or -1.
	Row int
	// Name is the name looked up (a logic, an action, a rule), and Available
	// the names that exist, sorted; for a limit_exceeded of the Go data an
	// evaluation reads (Limits.MaxValues), the limit's name, "max_values".
	Name      string
	Available []string
	// Node is the failing node in its JSON form, when it can be written.
	Node string
	// Operation and Reason say what was not supported, and why.
	Operation, Reason string
	// Cause is the message of the error that caused this one (for an
	// action_error, what the action returned or the panic).
	Cause string
	// Decision is, for an action_error, the decision whose action failed.
	Decision *Decision
	// JSON is the error's JSON form (spec/errors.md, JSON form).
	JSON string

	err error // the action's error, for Unwrap
}

// Error gives the message.
func (e *Error) Error() string { return e.Message }

// Unwrap gives the error an action returned, for an action_error.
func (e *Error) Unwrap() error { return e.err }

// Is matches the sentinel of e's code and of its ancestors.
func (e *Error) Is(target error) bool {
	s, ok := target.(*sentinel)
	if !ok {
		return false
	}
	for code := e.Code; code != ""; code = parentCode(code) {
		if code == s.code {
			return true
		}
	}
	return false
}

// statusCodes maps the numbers of the C ABI to codes, and codeStatuses back,
// from the library's own table.
var statusCodes, codeStatuses = func() (map[C.cr_status]Code, map[Code]C.cr_status) {
	codes := map[C.cr_status]Code{}
	statuses := map[Code]C.cr_status{}
	for status := C.cr_status(-2); status < 64; status++ {
		if name := C.cr_status_name(status); name != nil && status != C.CR_OK {
			codes[status] = Code(C.GoString(name))
			statuses[Code(C.GoString(name))] = status
		}
	}
	return codes, statuses
}()

// parentCode is the code's parent in the hierarchy of spec/errors.md, as the
// library gives it, or "" for a root.
func parentCode(code Code) Code {
	status, ok := codeStatuses[code]
	if !ok {
		return ""
	}
	return statusCodes[C.cr_error_parent(status)]
}

// errorForm is the JSON form of an error (spec/errors.md, JSON form), in its
// order.
type errorForm struct {
	Code      Code            `json:"code"`
	Message   string          `json:"message"`
	Hint      string          `json:"hint,omitempty"`
	Pointer   *string         `json:"pointer,omitempty"`
	Line      int             `json:"line,omitempty"`
	Column    int             `json:"column,omitempty"`
	Rule      string          `json:"rule,omitempty"`
	Rules     []string        `json:"rules,omitempty"`
	Path      []any           `json:"path,omitempty"`
	Types     []string        `json:"types,omitempty"`
	Row       *int            `json:"row,omitempty"`
	Name      string          `json:"name,omitempty"`
	Available []string        `json:"available,omitempty"`
	Executed  []string        `json:"executed,omitempty"`
	Node      json.RawMessage `json:"node,omitempty"`
	Operation string          `json:"operation,omitempty"`
	Reason    string          `json:"reason,omitempty"`
	Cause     string          `json:"cause,omitempty"`
}

// errorOf reads an error of the C ABI, which stays the caller's.
func errorOf(status C.cr_status, e *C.cr_error) *Error {
	if e == nil {
		code := statusCodes[status]
		if code == "" {
			code = CodeInternal
		}
		return newError(code, fmt.Sprintf("cleanrules: the C ABI failed with status %d", int(status)), "")
	}
	out := &Error{Row: -1}
	text := staticString(C.cr_error_json(e))
	var form errorForm
	decoder := json.NewDecoder(bytes.NewReader([]byte(text)))
	decoder.UseNumber()
	if err := decoder.Decode(&form); err != nil {
		// The core writes the form; it always reads back.
		return newError(CodeInternal, "cleanrules: unreadable error: "+err.Error(), "")
	}
	out.fill(form)
	out.Message = staticString(C.cr_error_message(e))
	out.Hint = staticString(C.cr_error_hint(e))
	out.JSON = text
	return out
}

func (e *Error) fill(form errorForm) {
	e.Code = form.Code
	e.Message = form.Message
	e.Hint = form.Hint
	e.Pointer = form.Pointer
	e.Line, e.Column = form.Line, form.Column
	e.Rule, e.Rules = form.Rule, form.Rules
	e.Path = pathOf(form.Path)
	e.Types = form.Types
	if form.Row != nil {
		e.Row = *form.Row
	}
	e.Name, e.Available, e.Executed = form.Name, form.Available, form.Executed
	if len(form.Node) > 0 {
		e.Node = string(form.Node)
	}
	e.Operation, e.Reason, e.Cause = form.Operation, form.Reason, form.Cause
}

// pathOf turns the segments of a decoded JSON path (strings, and
// json.Number indexes) into strings and ints.
func pathOf(segments []any) []any {
	if segments == nil {
		return nil
	}
	path := make([]any, len(segments))
	for i, segment := range segments {
		if n, ok := segment.(json.Number); ok {
			index, err := strconv.ParseInt(string(n), 10, 64)
			if err == nil {
				path[i] = int(index)
				continue
			}
		}
		path[i] = segment
	}
	return path
}

// newError is an error that this package finds itself, with its JSON form.
func newError(code Code, message, hint string) *Error {
	return formError(errorForm{Code: code, Message: message, Hint: hint})
}

// formError is an error that this package finds itself, from its JSON form.
func formError(form errorForm) *Error {
	text, _ := json.Marshal(form) // strings and lists of them: it cannot fail
	e := &Error{Row: -1, JSON: string(text)}
	e.fill(form)
	return e
}

// ruleNotFound is the rule_not_found error of the core
// (RuleSet::rule_position), for a name among names, made in Go where the
// rule set may be closed.
func ruleNotFound(name string, names []string) *Error {
	available := slices.Clone(names)
	slices.Sort(available)
	form := errorForm{Code: CodeRuleNotFound, Message: "no rule named " + strconv.Quote(name), Name: name, Available: available}
	if len(available) > 0 {
		form.Hint = "the rules are: " + strings.Join(available, ", ")
	}
	return formError(form)
}

// check turns a status and its error into a Go error, freeing the error.
func check(status C.cr_status, e *C.cr_error) error {
	if status == C.CR_OK {
		return nil
	}
	defer C.cr_error_free(e)
	return errorOf(status, e)
}
