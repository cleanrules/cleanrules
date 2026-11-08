package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// Format is a syntax documents are read from and written in.
type Format string

// The formats.
const (
	// FormatJSON is JSON (spec/json-format.md), compact when written.
	FormatJSON Format = "json"
	// FormatYAML is YAML (spec/json-format.md, YAML), block style when
	// written.
	FormatYAML Format = "yaml"
	// FormatText is the rule syntax (spec/text.md).
	FormatText Format = "text"
	// FormatCSV is CSV, for decision tables (spec/tables.md).
	FormatCSV Format = "csv"
)

// code is the format's number in the C ABI; another name is a format_error.
func (f Format) code() (C.cr_format, error) {
	switch f {
	case FormatJSON:
		return C.CR_FORMAT_JSON, nil
	case FormatYAML:
		return C.CR_FORMAT_YAML, nil
	case FormatText:
		return C.CR_FORMAT_TEXT, nil
	case FormatCSV:
		return C.CR_FORMAT_CSV, nil
	}
	return 0, newError(CodeFormat, fmt.Sprintf("unknown format %q", string(f)), "use FormatJSON, FormatYAML, FormatText or FormatCSV")
}

// extensions are the formats of the file extensions Load and Dump know.
var extensions = map[string]Format{
	".json": FormatJSON, ".yaml": FormatYAML, ".yml": FormatYAML, ".rules": FormatText, ".csv": FormatCSV,
}

// Document is what a document holds (spec/json-format.md, Documents): an
// *Expr (a condition), a *Rule, a *RuleSet or a *DecisionTable. Loads and
// Load give one, to read with a type switch; Dumps and Dump write one.
//
//	doc, err := cleanrules.Load("pricing.yaml")
//	switch doc := doc.(type) {
//	case *cleanrules.RuleSet:
//		decision, err := doc.Evaluate(order)
//	case *cleanrules.DecisionTable:
//		decision, err := doc.Evaluate(order)
//	}
//
// Only the package's four types are documents.
type Document interface {
	isDocument()
}

func (*Expr) isDocument()          {}
func (*Rule) isDocument()          {}
func (*RuleSet) isDocument()       {}
func (*DecisionTable) isDocument() {}

// ---- Reading.

// Loads reads a document in JSON (or the format of WithFormat: YAML, the
// rule syntax, or CSV for a decision table): a condition (an *Expr), a
// *Rule, a *RuleSet or a *DecisionTable. Give bytes as string(b), as
// RuleFromText and RuleSet.ReadDecision take them too.
// The core reads and checks it, strictly and within limits (WithLimits):
// invalid syntax, an unknown or duplicate key, a wrong type, an unknown
// version or a text that is not UTF-8 is a format_error with a JSON Pointer
// or a line and column, and a document beyond the limits a limit_exceeded.
// Rules keep the core's own rules, so they write back exactly. A rule's
// action is the name the document gives it: WithActions binds the names to
// Go functions.
//
//	doc, err := cleanrules.Loads(text, cleanrules.WithFormat(cleanrules.FormatYAML))
//	rules, ok := doc.(*cleanrules.RuleSet)
func Loads(text string, options ...LoadOption) (Document, error) {
	settings, err := documentOptions(options, LoadOption.applyLoad, loadOptions)
	if err != nil {
		return nil, err
	}
	return load(text, settings)
}

// Load reads a document from a file, in the format of its extension (.json,
// .yaml, .yml, .rules or .csv) unless WithFormat gives one; otherwise as
// Loads. A file larger than the size limit of its format (Limits.MaxSize for
// JSON, MaxYAMLSize for YAML, MaxTextSize for text and CSV) is not read: a
// limit_exceeded error. An extension that tells no format, and a file that
// cannot be read, are errors that are not *Error (the latter an
// *fs.PathError).
func Load(path string, options ...LoadOption) (Document, error) {
	settings, err := documentOptions(options, LoadOption.applyLoad, loadOptions)
	if err != nil {
		return nil, err
	}
	format, err := formatFor(path, settings.format)
	if err != nil {
		return nil, err
	}
	settings.format = &format
	text, err := readFile(path, format, settings.limits)
	if err != nil {
		return nil, err
	}
	return load(text, settings)
}

// formatFor is the format given, or else that of the file's extension.
func formatFor(path string, given *Format) (Format, error) {
	if given != nil {
		return *given, nil
	}
	if format, ok := extensions[strings.ToLower(filepath.Ext(path))]; ok {
		return format, nil
	}
	return "", fmt.Errorf("cleanrules: cannot tell the format of %q from its extension (.json, .yaml, .yml, .rules or .csv); pass one with WithFormat", filepath.Base(path))
}

// readFile reads a file of at most the size limit of its format.
func readFile(path string, format Format, limits *Limits) (string, error) {
	if _, err := format.code(); err != nil {
		return "", err
	}
	all, err := limits.effective()
	if err != nil {
		return "", err
	}
	limit, field := all.MaxSize, "MaxSize"
	switch format {
	case FormatYAML:
		limit, field = all.MaxYAMLSize, "MaxYAMLSize"
	case FormatText, FormatCSV:
		limit, field = all.MaxTextSize, "MaxTextSize"
	}
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	size := int64(-1)
	if info, err := f.Stat(); err == nil {
		size = info.Size()
	}
	var data []byte
	if size <= int64(limit) {
		// At most one byte more than the limit, in case the file grew.
		data, err = io.ReadAll(io.LimitReader(f, int64(limit)+1))
		if err != nil {
			return "", err
		}
		size = int64(len(data))
	}
	if size > int64(limit) {
		return "", formError(errorForm{
			Code:    CodeLimitExceeded,
			Message: fmt.Sprintf("%s has %d bytes, more than the limit of %d", filepath.Base(path), size, limit),
			Hint:    fmt.Sprintf("raise the limit with Limits.%s if the file is trusted", field),
		})
	}
	return string(data), nil
}

// load reads a document's text with the settings of Loads.
func load(source string, settings documentSettings) (Document, error) {
	format := FormatJSON
	if settings.format != nil {
		format = *settings.format
	}
	code, err := format.code()
	if err != nil {
		return nil, err
	}
	limits, err := settings.limits.native()
	if err != nil {
		return nil, err
	}
	defer C.cr_limits_free(limits)
	p, n := text(source)
	var document *C.cr_document
	var e *C.cr_error
	if err := check(C.cr_document_read(p, n, code, limits, &document, &e), e); err != nil {
		return nil, err
	}
	if C.cr_document_kind(document) == C.CR_DOCUMENT_DECISION_TABLE {
		// The table owns the document from here; a table has no actions.
		return newDecisionTable(document)
	}
	defer C.cr_document_free(document)
	var content Document
	switch C.cr_document_kind(document) {
	case C.CR_DOCUMENT_CONDITION:
		var node *C.cr_node
		if err := check(C.cr_document_condition(document, &node, &e), e); err != nil {
			return nil, err
		}
		return wrapNode(node), nil
	case C.CR_DOCUMENT_RULE:
		var rule *C.cr_rule
		if err := check(C.cr_document_rule(document, &rule, &e), e); err != nil {
			return nil, err
		}
		r := wrapRule(rule)
		if settings.bind {
			if err := checkActions(r, settings.actions); err != nil {
				return nil, err
			}
		}
		content = r
	default: // C.CR_DOCUMENT_RULESET
		var ruleSet *C.cr_ruleset
		if err := check(C.cr_ruleset_from_document(document, &ruleSet, &e), e); err != nil {
			return nil, err
		}
		rs, err := wrapReadRuleSet(ruleSet)
		if err != nil {
			return nil, err
		}
		if settings.bind {
			return rs.Bind(settings.actions)
		}
		content = rs
	}
	return content, nil
}

// checkActions checks a rule's action against actions, as Bind would in a
// rule set of that rule alone: a Go rule holds no function.
func checkActions(rule *Rule, actions Actions) error {
	rs, err := NewRuleSet([]*Rule{rule})
	if err != nil {
		return err
	}
	defer rs.Close()
	_, err = rs.Bind(actions)
	return err
}

// wrapReadRuleSet wraps a rule set the core read, with copies of its rules.
func wrapReadRuleSet(ptr *C.cr_ruleset) (*RuleSet, error) {
	n := int(C.cr_ruleset_len(ptr))
	rules := make([]*Rule, n)
	for i := range n {
		var rule *C.cr_rule
		var e *C.cr_error
		if err := check(C.cr_ruleset_rule(ptr, C.size_t(i), &rule, &e), e); err != nil {
			C.cr_ruleset_free(ptr)
			return nil, err
		}
		rules[i] = wrapRule(rule)
	}
	return wrapRuleSet(ptr, rules)
}

// RuleFromText reads one rule written in the rule syntax (spec/text.md), as
// fuzzytool's rule_from_text does: without a RULE line it is named rule_1,
// and line breaks are free. A text that does not follow the syntax, or that
// holds a condition or more than one rule, is a rule_syntax_error with its
// line and column. To read a condition or a rule set written as text, use
// Loads with WithFormat(FormatText).
//
//	vip, err := cleanrules.RuleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15")
func RuleFromText(source string, options ...TextOption) (*Rule, error) {
	settings, err := documentOptions(options, TextOption.applyText, textOptions)
	if err != nil {
		return nil, err
	}
	limits, err := settings.limits.native()
	if err != nil {
		return nil, err
	}
	defer C.cr_limits_free(limits)
	p, n := text(source)
	var out *C.cr_rule
	var e *C.cr_error
	if err := check(C.cr_rule_from_text(p, n, limits, &out, &e), e); err != nil {
		return nil, err
	}
	rule := wrapRule(out)
	if settings.bind {
		if err := checkActions(rule, settings.actions); err != nil {
			return nil, err
		}
	}
	return rule, nil
}

// ReadDecision reads a recorded decision, in JSON or YAML (WithFormat), with
// the rule set that made it (spec/json-format.md, Reading a decision): one
// that does not belong to it is a decision_mismatch. The rule syntax and CSV
// hold no decisions: unsupported. Its opaque values come back as nil: the
// document keeps only their type's name.
func (rs *RuleSet) ReadDecision(source string, options ...ReadOption) (*Decision, error) {
	settings, err := documentOptions(options, ReadOption.applyRead, readOptions)
	if err != nil {
		return nil, err
	}
	format := FormatJSON
	if settings.format != nil {
		format = *settings.format
	}
	code, err := format.code()
	if err != nil {
		return nil, err
	}
	var decision *Decision
	err = rs.use(func(ptr *C.cr_ruleset) error {
		limits, err := settings.limits.native()
		if err != nil {
			return err
		}
		defer C.cr_limits_free(limits)
		p, n := text(source)
		var out *C.cr_decision
		var e *C.cr_error
		if err := check(C.cr_ruleset_read_decision(ptr, p, n, code, limits, &out, &e), e); err != nil {
			return err
		}
		decision = newDecision(rs, out, nil, true)
		return nil
	})
	return decision, err
}

// ---- Writing.

// Dumps writes a condition, rule, rule set or decision table as a document,
// in canonical form: compact JSON by default, or the format of WithFormat
// (YAML in block style; the rule syntax, IF ... THEN ...; CSV, for decision
// tables only). The core writes it: CSV for anything but a decision table,
// and the rule syntax for a decision table or a rule with metadata, are
// unsupported, and a value node (Field("a").Add(1)) is not a condition, an
// expression_error. A rule's action is written as its name (a Go rule never
// holds code, so nothing is ever refused for its action).
//
// A nil Document holds nothing to write: an unsupported error. A nil
// pointer of one of the four types is that type's nil value, which gives
// that type's error, as its methods do: ErrClosed for a *RuleSet or a
// *DecisionTable, an expression_error for an *Expr, a rule_definition_error
// for a *Rule.
func Dumps(value Document, options ...DumpOption) (string, error) {
	settings, err := documentOptions(options, DumpOption.applyDump, dumpOptions)
	if err != nil {
		return "", err
	}
	format := FormatJSON
	if settings.format != nil {
		format = *settings.format
	}
	return dumps(value, format)
}

func dumps(value Document, format Format) (string, error) {
	code, err := format.code()
	if err != nil {
		return "", err
	}
	var out *C.char
	var n C.size_t
	var e *C.cr_error
	switch v := value.(type) {
	case *Expr:
		err = v.use(func(node *C.cr_node) error {
			var document *C.cr_document
			if err := check(C.cr_document_new_condition(node, &document, &e), e); err != nil {
				return err
			}
			defer C.cr_document_free(document)
			return check(C.cr_document_write(document, code, &out, &n, &e), e)
		})
	case *Rule:
		err = v.use(func(rule *C.cr_rule) error {
			var document *C.cr_document
			if err := check(C.cr_document_new_rule(rule, &document, &e), e); err != nil {
				return err
			}
			defer C.cr_document_free(document)
			return check(C.cr_document_write(document, code, &out, &n, &e), e)
		})
	case *RuleSet:
		err = v.use(func(ptr *C.cr_ruleset) error {
			return check(C.cr_ruleset_write(ptr, code, &out, &n, &e), e)
		})
	case *DecisionTable:
		err = v.use(func(document *C.cr_document) error {
			return check(C.cr_document_write(document, code, &out, &n, &e), e)
		})
	default: // nil
		err = errNilDocument
	}
	if err != nil {
		return "", err
	}
	return takeString(out, n), nil
}

var errNilDocument = newError(CodeUnsupported, "a nil Document cannot be written", "pass an *Expr, a *Rule, a *RuleSet or a *DecisionTable")

// Dump writes a condition, rule, rule set or decision table to a file, as
// Dumps does, in the format of its extension (.json, .yaml, .yml, .rules or
// .csv) unless WithFormat gives one. A rule text ends with a line break.
func Dump(value Document, path string, options ...DumpOption) error {
	settings, err := documentOptions(options, DumpOption.applyDump, dumpOptions)
	if err != nil {
		return err
	}
	format, err := formatFor(path, settings.format)
	if err != nil {
		return err
	}
	text, err := dumps(value, format)
	if err != nil {
		return err
	}
	if format == FormatText {
		text += "\n"
	}
	return os.WriteFile(path, []byte(text), 0o666)
}
