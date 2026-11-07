package cleanrules

import (
	"bytes"
	"encoding/json"
	"strconv"
)

// Summary describes a rule set without evaluating it (spec/explain.md,
// Summaries). String is its text, the same in every language, and JSON its
// JSON document.
type Summary struct {
	HitPolicy HitPolicy
	// Aggregate is "" when the rule set has none.
	Aggregate Aggregate
	Logic     string
	Strict    bool
	// Rules are the rules, in rule order.
	Rules []RuleSummary
	// Fields are the fields read, in order of first appearance.
	Fields []FieldUsage
	// Outputs are the payload keys, in order of first appearance.
	Outputs []string
	// Tags are the tags, sorted, each with the rules that carry it.
	Tags []TagUsage

	json, text string
}

// RuleSummary is one rule of a summary.
type RuleSummary struct {
	Name     string
	Priority int64
	// Outputs are the rule's payload keys.
	Outputs []string
	// Action reports that the rule has an action, and ActionName names it
	// ("" for an action without a name).
	Action     bool
	ActionName string
}

// FieldUsage is a field a rule set reads, and where.
type FieldUsage struct {
	// Path is the field's path: string keys and int indexes.
	Path []any
	// Conditions are the rules whose condition reads it, in rule order.
	Conditions []string
	// Payloads are the payload entries whose value reads it.
	Payloads []PayloadUse
}

// PayloadUse is a payload entry that reads a field: the rule and the key.
type PayloadUse struct {
	Rule, Key string
}

// TagUsage is a tag and the rules that carry it, in rule order.
type TagUsage struct {
	Tag   string
	Rules []string
}

// String is the summary's text.
func (s *Summary) String() string { return s.text }

// JSON is the summary's JSON document (spec/json-format.md, Summaries).
func (s *Summary) JSON() string { return s.json }

type summaryDocument struct {
	Summary struct {
		HitPolicy string `json:"hit_policy"`
		Aggregate string `json:"aggregate"`
		Logic     string `json:"logic"`
		Strict    bool   `json:"strict"`
		Rules     []struct {
			Name     string          `json:"name"`
			Priority json.RawMessage `json:"priority"`
			Outputs  []string        `json:"outputs"`
			Action   bool            `json:"action"`
			Do       string          `json:"do"`
		} `json:"rules"`
		Fields []struct {
			Path       []any        `json:"path"`
			Conditions []string     `json:"conditions"`
			Payloads   []PayloadUse `json:"payloads"`
		} `json:"fields"`
		Outputs []string   `json:"outputs"`
		Tags    []TagUsage `json:"tags"`
	} `json:"summary"`
}

// newSummary reads the summary document the core wrote.
func newSummary(document, text string) (*Summary, error) {
	var doc summaryDocument
	decoder := json.NewDecoder(bytes.NewReader([]byte(document)))
	decoder.UseNumber()
	if err := decoder.Decode(&doc); err != nil {
		return nil, newError(CodeInternal, "cleanrules: unreadable summary: "+err.Error(), "please report it")
	}
	d := doc.Summary
	s := &Summary{
		HitPolicy: HitPolicy(d.HitPolicy),
		Aggregate: Aggregate(d.Aggregate),
		Logic:     d.Logic,
		Strict:    d.Strict,
		Outputs:   d.Outputs,
		Tags:      d.Tags,
		json:      document,
		text:      text,
	}
	for _, rule := range d.Rules {
		s.Rules = append(s.Rules, RuleSummary{rule.Name, intValue(rule.Priority), rule.Outputs, rule.Action, rule.Do})
	}
	for _, f := range d.Fields {
		s.Fields = append(s.Fields, FieldUsage{pathOf(f.Path), f.Conditions, f.Payloads})
	}
	return s, nil
}

// intValue reads an int value of the JSON format: a number, or
// {"$int": "..."} beyond 2^53.
func intValue(raw json.RawMessage) int64 {
	var typed struct {
		Int json.Number `json:"$int"`
	}
	text := string(raw)
	if json.Unmarshal(raw, &typed) == nil && typed.Int != "" {
		text = string(typed.Int)
	}
	n, _ := strconv.ParseInt(text, 10, 64)
	return n
}
