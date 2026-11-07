package cleanrules

// Explanation is the explanation of a decision (spec/explain.md): each rule's
// status and what it needed, made by the core. String is its text, the same
// in every language.
type Explanation struct {
	// HitPolicy and Logic are those that decided.
	HitPolicy HitPolicy
	Logic     string
	// Selected are the names of the selected rules, in rule order.
	Selected []string
	// Rules are each rule's explanation, in rule order.
	Rules []*RuleExplanation

	text, json string
}

// String is the explanation's text (spec/explain.md, Text).
func (e *Explanation) String() string { return e.text }

// JSON is the explanation's document in the JSON format
// (spec/json-format.md, Explanations), whose Cause.At pointers point into
// Decision.JSON.
func (e *Explanation) JSON() string { return e.json }

// ForRule is the explanation of the rule named name; a name the rule set
// lacks is a rule_not_found error, with the names it has.
func (e *Explanation) ForRule(name string) (*RuleExplanation, error) {
	names := make([]string, len(e.Rules))
	for i, rule := range e.Rules {
		if rule.Rule == name {
			return rule, nil
		}
		names[i] = rule.Rule
	}
	return nil, ruleNotFound(name, names)
}

// Status is what happened to a rule in a decision.
type Status string

// The statuses of a rule.
const (
	StatusSelected Status = "selected"
	StatusFired    Status = "fired"
	StatusNotFired Status = "not_fired"
	StatusUnknown  Status = "unknown"
)

// RuleExplanation is the explanation of one rule. String is its part of the
// explanation's text.
type RuleExplanation struct {
	// Rule is the rule's name.
	Rule string
	// Status is what happened to it.
	Status Status
	// Requirement is what the rule needed: a *Cause, or a *CauseGroup.
	Requirement Requirement
	// Note says why a rule that fired was not selected; nil otherwise.
	Note *Note
	// Achievable says, for a rule that did not fire or is unknown, whether
	// changing the listed conditions can make it fire; nil otherwise.
	Achievable *bool

	text string
}

// String is the rule's part of the explanation's text.
func (r *RuleExplanation) String() string { return r.text }

// Causes are the causes of the requirement, in order, without their
// grouping.
func (r *RuleExplanation) Causes() []*Cause {
	var causes []*Cause
	stack := []Requirement{r.Requirement}
	for len(stack) > 0 {
		top := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		switch req := top.(type) {
		case *Cause:
			causes = append(causes, req)
		case *CauseGroup:
			for i := len(req.Items) - 1; i >= 0; i-- {
				stack = append(stack, req.Items[i])
			}
		}
	}
	return causes
}

// Requirement is what a rule needed (spec/explain.md, Requirements): a
// *Cause, or a *CauseGroup of requirements.
type Requirement interface{ requirement() }

// GroupKind says how the requirements of a group combine.
type GroupKind string

// The kinds of groups.
const (
	// GroupAll needs every requirement.
	GroupAll GroupKind = "all"
	// GroupAny needs one of them.
	GroupAny GroupKind = "any"
)

// CauseGroup is requirements that must all hold, or of which one suffices.
type CauseGroup struct {
	Kind  GroupKind
	Items []Requirement
}

func (*CauseGroup) requirement() {}

// Cause is a condition in a rule's requirement, with what was read
// (spec/explain.md, Causes). String is its line of the explanation, such as
// "x > 10 is false (x = 8); short by 2".
type Cause struct {
	// Condition is the condition in its JSON form (a between half is the
	// comparison it is); Node reads it.
	Condition string
	// At is a JSON Pointer to the result in the decision's document that the
	// cause comes from.
	At string
	// Truth is the condition's truth value.
	Truth Truth
	// Negated reports that the condition is under a negation, so the rule
	// needed it false.
	Negated bool
	// Holds reports whether the condition went the way the rule needed.
	Holds bool
	// Values are the fields the condition read, with their values, in order.
	Values []FieldValue
	// Reason says why the condition is unknown; nil otherwise.
	Reason *Reason
	// Counterfactual is, for a comparison that did not hold, what it needed;
	// nil otherwise.
	Counterfactual *Counterfactual

	text string
}

func (*Cause) requirement() {}

// String is the cause's line in the explanation's text.
func (c *Cause) String() string { return c.text }

// Node is the condition as an expression.
func (c *Cause) Node() (*Expr, error) { return readNode(c.Condition) }

// FieldValue is a field a cause read, and its value.
type FieldValue struct {
	// Path is the field's path: string keys and int indexes.
	Path []any
	// Value is its value, as Go values are given back.
	Value any
}

// Reason says why a result is unknown or null (spec/ast.md, Results and
// reasons).
type Reason struct {
	// Code is the reason's code, such as "missing" or "type_mismatch".
	Code string
	// Paths are the paths involved, and Types the type groups.
	Paths [][]any
	Types []string
	// Detail is informative text, such as a read error's message.
	Detail string
}

// Counterfactual is what a comparison needed: Path Op Target, and how far
// the value was (spec/explain.md, Counterfactuals).
type Counterfactual struct {
	Path []any
	// Op is a comparison tag, such as "gt", already flipped and negated.
	Op string
	// Target is the value to compare with.
	Target any
	// Gap is how far the value was: an int64, a float64 or a Decimal; a
	// GapDays between dates, a GapMicroseconds between datetimes; nil when
	// there is none.
	Gap any
}

// NoteKind says why a rule that fired was not selected.
type NoteKind string

// The kinds of notes.
const (
	NoteLowerPriority NoteKind = "lower_priority"
	NoteSamePriority  NoteKind = "same_priority"
	NoteAnyAgrees     NoteKind = "any_agrees"
	NoteEarlierRule   NoteKind = "earlier_rule"
)

// Note says why a rule that fired was not selected: which rule won, and the
// priorities for the two priority kinds (nil otherwise).
type Note struct {
	Kind           NoteKind
	Winner         string
	Priority       *int64
	WinnerPriority *int64
}

// newExplanation reads the core's explanation value (cr_decision_explain_value):
// the explanation document's structure, with the values themselves, and
// text and holds.
func newExplanation(value map[string]any, text, json string) *Explanation {
	e := &Explanation{
		HitPolicy: HitPolicy(str(value["hit_policy"])),
		Logic:     str(value["logic"]),
		Selected:  strs(value["selected"]),
		text:      text,
		json:      json,
	}
	for _, item := range list(value["rules"]) {
		rule, _ := item.(map[string]any)
		explained := &RuleExplanation{
			Rule:        str(rule["rule"]),
			Status:      Status(str(rule["status"])),
			Requirement: requirementOf(rule["requirement"]),
			text:        str(rule["text"]),
		}
		if achievable, ok := rule["achievable"].(bool); ok {
			explained.Achievable = &achievable
		}
		if note, ok := rule["note"].(map[string]any); ok {
			explained.Note = &Note{
				Kind:           NoteKind(str(note["kind"])),
				Winner:         str(note["winner"]),
				Priority:       optionalInt(note["priority"]),
				WinnerPriority: optionalInt(note["winner_priority"]),
			}
		}
		e.Rules = append(e.Rules, explained)
	}
	return e
}

func requirementOf(v any) Requirement {
	m, _ := v.(map[string]any)
	for _, kind := range []GroupKind{GroupAll, GroupAny} {
		if items, ok := m[string(kind)].([]any); ok {
			group := &CauseGroup{Kind: kind}
			for _, item := range items {
				group.Items = append(group.Items, requirementOf(item))
			}
			return group
		}
	}
	cause := &Cause{
		Condition: str(m["condition"]),
		At:        str(m["at"]),
		Truth:     truthValue(m["truth"]),
		Negated:   m["negated"] == true,
		Holds:     m["holds"] == true,
		Reason:    reasonOf(m["reason"]),
		text:      str(m["text"]),
	}
	for _, item := range list(m["values"]) {
		read, _ := item.(map[string]any)
		cause.Values = append(cause.Values, FieldValue{pathOfValue(read["path"]), read["value"]})
	}
	if c, ok := m["counterfactual"].(map[string]any); ok {
		counterfactual := &Counterfactual{Path: pathOfValue(c["path"]), Op: str(c["op"]), Target: c["target"], Gap: c["gap"]}
		if distance, ok := c["gap"].(map[string]any); ok {
			if days, ok := distance["days"].(int64); ok {
				counterfactual.Gap = GapDays(days)
			} else if us, ok := distance["microseconds"].(int64); ok {
				counterfactual.Gap = GapMicroseconds(us)
			}
		}
		cause.Counterfactual = counterfactual
	}
	return cause
}

func reasonOf(v any) *Reason {
	m, ok := v.(map[string]any)
	if !ok {
		return nil
	}
	reason := &Reason{Code: str(m["code"]), Types: strs(m["types"]), Detail: str(m["detail"])}
	if paths, ok := m["paths"].([]any); ok {
		reason.Paths = pathsOf(paths)
	}
	return reason
}

// truthValue reads a truth written as a boolean, or null for unknown.
func truthValue(v any) Truth {
	switch v {
	case true:
		return TruthTrue
	case false:
		return TruthFalse
	default:
		return TruthUnknown
	}
}

func str(v any) string {
	s, _ := v.(string)
	return s
}

func list(v any) []any {
	l, _ := v.([]any)
	return l
}

func strs(v any) []string {
	var out []string
	for _, item := range list(v) {
		out = append(out, str(item))
	}
	return out
}

func optionalInt(v any) *int64 {
	if n, ok := v.(int64); ok {
		return &n
	}
	return nil
}
