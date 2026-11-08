// Package cleanrules is the Go binding of cleanrules, a rules engine whose
// single implementation is a Rust core, reached here through its C ABI with
// cgo. Conditions are data (an AST the core checks, evaluates, explains and
// summarizes), and the same rules give the same decisions, explanations and
// summaries in every language (the specification in spec/ and the
// conformance vectors in conformance/ of the repository).
//
// # Building rules
//
// Go has no operator overloading, so conditions are built with functions
// and methods, as fuzzytool's &, | and ~ become And, Or and Not:
//
//	cond := cleanrules.Field("customer", "age").Ge(18).
//		And(cleanrules.Field("country").IsIn("ES", "PT"))
//
// Rules and rule sets take functional options, and return errors as values:
//
//	vip, err := cleanrules.NewRule("vip", cond,
//		cleanrules.Then("discount", cleanrules.MustDecimal("0.15")),
//		cleanrules.WithPriority(10))
//	rules, err := cleanrules.NewRuleSet([]*cleanrules.Rule{vip},
//		cleanrules.WithHitPolicy(cleanrules.HitPolicyPriority))
//
// # Deciding
//
// RuleSet.Evaluate reads Go data (maps by their string keys, slices,
// structs by their exported fields, pointers, time.Time, math/big, Decimal,
// Date and DateTime; spec/data-model.md, Go) along the paths the rule set
// reads only, and gives a Decision with Go values back, its Explain and
// WhyNot, and its JSON document. RuleSet.Summary describes a rule set;
// RuleSet.Bind and Run call Go functions for the actions the selected rules
// name.
//
// # Documents, tables and result trees
//
// Loads and Load read a condition, rule, rule set or decision table from
// JSON, YAML, the rule syntax or CSV, as a Document to read with a type
// switch; Dumps and Dump write one; RuleFromText reads one rule, and
// RuleSet.ReadDecision a recorded decision. NewDecisionTable builds a
// decision table from a header and rows of cells. Expr.EvaluateTree,
// Expr.EvaluateValueTree and ActionContext.Tree give a ResultTree. Each
// function takes options of its own type (WithFormat, WithLimits,
// WithActions, WithTablePolicy...).
//
// # Errors
//
// Every error is an *Error with a stable Code and its context, except
// ErrClosed, for a RuleSet, Decision or DecisionTable used after Close, and
// the errors of files that Load and Dump cannot read, write or tell the
// format of; errors.Is matches
// an *Error with the sentinel of its code or of an ancestor (ErrEvaluation
// for a missing_field), and errors.As reads its fields.
//
// # Goroutines and memory
//
// Expr, Rule, RuleSet and Decision never change once made, so any number of
// goroutines may use one at once, also while another closes it: Close frees
// the native memory when the calls using it return, and later calls give
// ErrClosed. Without Close, the memory is freed once the Go object is
// unreachable (runtime.AddCleanup). The maps and slices a Decision gives are
// copies. Each action runs on a goroutine of its own while Run's goroutine
// waits, so that a panic or runtime.Goexit never unwinds through the
// core's frames. Calls into the core run on the thread's stack, not the
// goroutine's (as every cgo call does). At the default limits a call needs at
// most 128 KiB of it with a release build of the library, as the prebuilt
// ones are, and 512 KiB with a debug build, which is what cargo build gives
// by default in the repository: well within the default thread stacks of
// Linux (8 MiB), but at the edge of macOS's 512 KiB for threads other than
// the main one.
//
// # Linking
//
// The module Go users get links a prebuilt static library of the core for
// its platform: macOS and Linux on x64 and arm64, Linux with musl under the
// tag cleanrules_musl (Go has no GOOS for it), and Windows on x64 (MinGW-w64)
// and arm64 (llvm-mingw); another platform stops the build with an error
// that names it. In the repository, the package links the static library
// cargo builds (cargo build -p cleanrules-ffi), and the tag
// cleanrules_prebuilt the prebuilt ones; with the tag cleanrules_external,
// CGO_LDFLAGS gives the library. See README.md.
package cleanrules
