package cleanrules

// Options. Each function with optional parts takes functional options of its
// own type: NewRule and Rule.With a RuleOption, NewRuleSet a RuleSetOption,
// NewDecisionTable a TableOption, Expr.EvaluateTree a TreeOption and
// Expr.EvaluateValueTree a ValueTreeOption, Loads and Load a LoadOption,
// RuleFromText a TextOption, Dumps and Dump a DumpOption, and
// RuleSet.ReadDecision a ReadOption. An option that means the same for
// several of them is one value of all their types (WithLogic is a
// RuleSetOption, a TableOption and a TreeOption, but not a ValueTreeOption),
// so the compiler rejects an option where it does not apply. The option
// types are interfaces whose methods are the package's own: only the
// functions of the package make them. A nil option sets nothing: a
// rule_definition_error, whose hint lists the function's options.

// RuleSetOption sets a rule set's hit policy, aggregate, logic or strict
// mode in NewRuleSet: WithHitPolicy, WithAggregate, WithLogic and
// WithStrict.
type RuleSetOption interface {
	applyRuleSet(*ruleSetSettings)
}

// TableOption sets a decision table's hit policy, aggregate, logic, strict
// mode or limits in NewDecisionTable: WithTablePolicy, WithAggregate,
// WithLogic, WithStrict and WithLimits.
type TableOption interface {
	applyTable(*tableSettings)
}

// TreeOption sets the logic and strict mode of Expr.EvaluateTree: WithLogic
// and WithStrict.
type TreeOption interface {
	applyTree(*treeSettings)
}

// ValueTreeOption sets the strict mode of Expr.EvaluateValueTree:
// WithStrict. A value has no truth to combine, so it takes no logic.
type ValueTreeOption interface {
	applyValueTree(*treeSettings)
}

// LoadOption sets how Loads and Load read a document: WithFormat,
// WithLimits and WithActions.
type LoadOption interface {
	applyLoad(*documentSettings)
}

// TextOption sets how RuleFromText reads a rule: WithLimits and
// WithActions.
type TextOption interface {
	applyText(*documentSettings)
}

// DumpOption sets how Dumps and Dump write a document: WithFormat.
type DumpOption interface {
	applyDump(*documentSettings)
}

// ReadOption sets how RuleSet.ReadDecision reads a decision: WithFormat and
// WithLimits.
type ReadOption interface {
	applyRead(*documentSettings)
}

// AggregateOption is WithAggregate's option, for rule sets and decision
// tables.
type AggregateOption interface {
	RuleSetOption
	TableOption
}

// LogicOption is WithLogic's option, for rule sets, decision tables and
// the result trees of conditions.
type LogicOption interface {
	RuleSetOption
	TableOption
	TreeOption
}

// StrictOption is WithStrict's option, for rule sets, decision tables and
// result trees.
type StrictOption interface {
	RuleSetOption
	TableOption
	TreeOption
	ValueTreeOption
}

// FormatOption is WithFormat's option, for reading and writing documents
// and reading decisions.
type FormatOption interface {
	LoadOption
	DumpOption
	ReadOption
}

// LimitsOption is WithLimits's option, for reading documents, rules,
// decisions and decision tables.
type LimitsOption interface {
	LoadOption
	TextOption
	ReadOption
	TableOption
}

// ActionsOption is WithActions's option, for reading documents and rules.
type ActionsOption interface {
	LoadOption
	TextOption
}

type ruleSetSettings struct {
	hitPolicy, aggregate, logic *string
	strict                      bool
}

type tableSettings struct {
	hitPolicy, aggregate, logic *string
	strict                      bool
	limits                      *Limits
}

type treeSettings struct {
	logic  *string
	strict bool
}

type documentSettings struct {
	format  *Format
	limits  *Limits
	actions Actions
	bind    bool
}

// WithHitPolicy sets a rule set's hit policy, HitPolicyFirst by default.
func WithHitPolicy(policy HitPolicy) RuleSetOption { return hitPolicyOption(policy) }

type hitPolicyOption HitPolicy

func (o hitPolicyOption) applyRuleSet(s *ruleSetSettings) { s.hitPolicy = name(string(o)) }

// WithTablePolicy sets a decision table's hit policy, TablePolicyFirst by
// default.
func WithTablePolicy(policy TablePolicy) TableOption { return tablePolicyOption(policy) }

type tablePolicyOption TablePolicy

func (o tablePolicyOption) applyTable(s *tableSettings) { s.hitPolicy = name(string(o)) }

// WithAggregate sets the aggregate of HitPolicyCollect (or
// TablePolicyCollect); none by default.
func WithAggregate(aggregate Aggregate) AggregateOption { return aggregateOption(aggregate) }

type aggregateOption Aggregate

func (o aggregateOption) applyRuleSet(s *ruleSetSettings) { s.aggregate = name(string(o)) }
func (o aggregateOption) applyTable(s *tableSettings)     { s.aggregate = name(string(o)) }

// WithLogic sets the logic by name, "boolean" (Kleene's) by default.
func WithLogic(logic string) LogicOption { return logicOption(logic) }

type logicOption string

func (o logicOption) applyRuleSet(s *ruleSetSettings) { s.logic = name(string(o)) }
func (o logicOption) applyTable(s *tableSettings)     { s.logic = name(string(o)) }
func (o logicOption) applyTree(s *treeSettings)       { s.logic = name(string(o)) }

// WithStrict sets strict mode: a field that cannot be read is an error
// instead of unknown (spec/rules.md, Strict mode and errors).
func WithStrict(strict bool) StrictOption { return strictOption(strict) }

type strictOption bool

func (o strictOption) applyRuleSet(s *ruleSetSettings) { s.strict = bool(o) }
func (o strictOption) applyTable(s *tableSettings)     { s.strict = bool(o) }
func (o strictOption) applyTree(s *treeSettings)       { s.strict = bool(o) }
func (o strictOption) applyValueTree(s *treeSettings)  { s.strict = bool(o) }

// WithFormat sets the format of a document or a decision: FormatJSON by
// default, or the format of the file's extension for Load and Dump.
func WithFormat(format Format) FormatOption { return formatOption(format) }

type formatOption Format

func (o formatOption) applyLoad(s *documentSettings) { f := Format(o); s.format = &f }
func (o formatOption) applyDump(s *documentSettings) { f := Format(o); s.format = &f }
func (o formatOption) applyRead(s *documentSettings) { f := Format(o); s.format = &f }

// WithLimits sets the limits of reading (spec/json-format.md, Limits;
// spec/tables.md, Limits); the core's defaults otherwise. See Limits for
// how its fields combine.
func WithLimits(limits Limits) LimitsOption { return limitsOption(limits) }

type limitsOption Limits

func (o limitsOption) applyLoad(s *documentSettings) { l := Limits(o); s.limits = &l }
func (o limitsOption) applyText(s *documentSettings) { l := Limits(o); s.limits = &l }
func (o limitsOption) applyRead(s *documentSettings) { l := Limits(o); s.limits = &l }
func (o limitsOption) applyTable(s *tableSettings)   { l := Limits(o); s.limits = &l }

// WithActions binds the actions that the rules read name to Go functions
// (spec/rules.md, Actions), as RuleSet.Bind does: a rule set comes back
// bound, and a name the registry lacks is a registry_error. A *Rule holds
// its action's name and never a function (Go binds actions on rule sets), so
// for a rule WithActions only checks that its name is registered; a
// decision table has no actions. Without it, a document's actions stay
// unbound names.
func WithActions(actions Actions) ActionsOption { return actionsOption{actions} }

type actionsOption struct{ actions Actions }

func (o actionsOption) applyLoad(s *documentSettings) { s.actions, s.bind = o.actions, true }
func (o actionsOption) applyText(s *documentSettings) { s.actions, s.bind = o.actions, true }

// name is a setting by name: a pointer, since an empty name is a name (which
// the core rejects), not a missing one.
func name(s string) *string { return &s }

// nilOption is the error of a nil option, whose hint names the options the
// function takes.
func nilOption(options string) *Error {
	return newError(CodeRuleDefinition, "a nil option sets nothing", "make options with "+options)
}

// The options of each function, for nilOption.
const (
	ruleOptions      = "Then, WithPriority, WithTags, WithDescription, WithMetadata and WithAction"
	ruleSetOptions   = "WithHitPolicy, WithAggregate, WithLogic and WithStrict"
	tableOptions     = "WithTablePolicy, WithAggregate, WithLogic, WithStrict and WithLimits"
	treeOptions      = "WithLogic and WithStrict"
	valueTreeOptions = "WithStrict"
	loadOptions      = "WithFormat, WithLimits and WithActions"
	textOptions      = "WithLimits and WithActions"
	dumpOptions      = "WithFormat"
	readOptions      = "WithFormat and WithLimits"
)

// documentOptions applies the options of a reading or writing function, or
// gives the error of a nil one.
func documentOptions[T any](options []T, apply func(T, *documentSettings), names string) (documentSettings, error) {
	var settings documentSettings
	for _, option := range options {
		if any(option) == nil {
			return settings, nilOption(names)
		}
		apply(option, &settings)
	}
	return settings, nil
}
