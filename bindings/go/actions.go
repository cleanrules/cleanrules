package cleanrules

/*
#include <stdlib.h>
#include "cleanrules.h"

// The trampoline of every action, written in Go (trampoline.go).
extern int32_t cleanrulesAction(void *data, cr_action_call *call);
*/
import "C"

import (
	"fmt"
	"runtime"
	"runtime/cgo"
	"slices"
	"unsafe"
)

// Action is a Go function that a rule names (spec/rules.md, Actions), run
// by RuleSet.Run for each selected rule that names it. An error it returns
// stops the run: Run gives an action_error that wraps it.
type Action func(ctx *ActionContext) error

// Actions is a registry: action names and their functions. It is the only
// place names are looked up, so a rule loaded from a file can only call what
// the program registers.
type Actions map[string]Action

// ActionContext is what an action is called with: the rule, the data Run
// was given and the decision.
type ActionContext struct {
	rule     *Rule
	position int
	data     any
	decision *Decision
	tree     actionTree
}

// Rule is the rule whose action runs.
func (c *ActionContext) Rule() *Rule { return c.rule }

// Position is the rule's position in its rule set.
func (c *ActionContext) Position() int { return c.position }

// Data is the data Run was given, as given.
func (c *ActionContext) Data() any { return c.data }

// Result is the rule's result in the decision.
func (c *ActionContext) Result() RuleResult { return c.decision.results[c.position].copied() }

// Output is the rule's payload, evaluated: its keys and values; empty for a
// rule without one.
func (c *ActionContext) Output() map[string]any {
	if output := c.decision.results[c.position].Output; output != nil {
		return copyMap(output)
	}
	return map[string]any{}
}

// Decision is the decision being run, the same one Run gives back.
func (c *ActionContext) Decision() *Decision { return c.decision }

// Bind gives this rule set with its named actions resolved in actions
// (spec/rules.md, Actions), for Run. Every name a rule gives must be
// registered, whether or not the rule would fire; the first that is not is a
// registry_error with the rule, the name and the registered names. A nil
// function counts as not registered, and the name "", which no rule can
// give, is left out. The rule set Bind gives shares this
// one's native rule set: closing either closes both.
func (rs *RuleSet) Bind(actions Actions) (*RuleSet, error) {
	bound := make(Actions, len(actions))
	for name, action := range actions {
		// No rule can name "" (WithAction rejects it), so it is never called.
		if action != nil && name != "" {
			bound[name] = action
		}
	}
	err := rs.use(func(ptr *C.cr_ruleset) error {
		registry, err := newRegistry(bound, nil)
		if err != nil {
			return err
		}
		defer registry.free()
		var e *C.cr_error
		return check(C.cr_ruleset_check_actions(ptr, registry.actions, &e), e)
	})
	if err != nil {
		return nil, err
	}
	copied := *rs
	copied.actions = bound
	own(rs.h, &copied)
	return &copied, nil
}

// Run evaluates like Evaluate, then runs the actions of the selected rules
// in rule order, each with an ActionContext; a selected
// rule without an action is skipped. Evaluate never runs actions, so a
// decision can be made any number of times.
//
// A rule naming an action that Bind did not register is a registry_error,
// checked before evaluating, so nothing runs. The first action that returns
// an error stops the run: Run gives an action_error whose Executed lists the
// rules whose actions had run, whose Decision is the decision and which
// wraps the action's error (errors.Unwrap, errors.Is and errors.As reach
// it).
//
// Each action runs on a goroutine of its own, which Run's goroutine waits
// for, one action at a time: so nothing an action does can unwind through
// the core's frames. An action that panics, or calls runtime.Goexit
// (t.FailNow and t.Fatal do), stops the run too; once the native frames are
// gone, Run panics again with the same value, or calls runtime.Goexit, on
// its own goroutine.
func (rs *RuleSet) Run(data any) (*Decision, error) {
	var decision *Decision
	var stopped *runState
	err := rs.use(func(ptr *C.cr_ruleset) error {
		r := newReader()
		input, err := r.read(data, rs.paths)
		if err != nil {
			return err
		}
		defer C.cr_value_free(input)
		run := &runState{ruleSet: rs, data: data, opaques: r.opaques}
		registry, err := newRegistry(rs.actions, run)
		if err != nil {
			return err
		}
		defer registry.free()
		var out *C.cr_decision
		var e *C.cr_error
		status := C.cr_ruleset_run(ptr, registry.actions, input, nil, &out, &e)
		if status == C.CR_OK {
			if run.decision != nil {
				// The handle the actions saw is the one given back.
				run.decision.h.adopt(freeDecision)
				decision = run.decision
			} else {
				decision = newDecision(rs, out, r.opaques, true)
			}
			return nil
		}
		failure := errorOf(status, e)
		if status != C.CR_ACTION_ERROR {
			C.cr_error_free(e)
			return failure
		}
		// The decision lives in the error, which it now owns.
		d := run.decision
		if d == nil {
			d = newDecision(rs, C.cr_error_decision(e), r.opaques, false)
		}
		d.h.adopt(func(unsafe.Pointer) { C.cr_error_free(e) })
		failure.Decision = d
		failure.err = run.err
		if run.panicked || run.exited {
			stopped = run
		}
		return failure
	})
	switch {
	case stopped == nil:
	case stopped.panicked:
		panic(stopped.panicValue)
	default:
		runtime.Goexit()
	}
	return decision, err
}

// runState is one run of Run, which its actions share.
type runState struct {
	ruleSet *RuleSet
	data    any
	opaques []any
	// decision is the decision being run, once an action saw it.
	decision *Decision
	// err is the error an action returned; panicValue what one panicked
	// with; exited, whether one called runtime.Goexit.
	err        error
	panicked   bool
	panicValue any
	exited     bool
}

// actionEntry is what the trampoline finds through an action's data: the
// function and its run.
type actionEntry struct {
	action Action
	run    *runState
}

// context is the context of the action the library calls, read on the
// trampoline's thread while the call is valid.
func (entry *actionEntry) context(call *C.cr_action_call) *ActionContext {
	run := entry.run
	if run.decision == nil {
		ptr := C.cr_action_call_decision(call)
		run.decision = newDecision(run.ruleSet, (*C.cr_decision)(unsafe.Pointer(ptr)), run.opaques, false)
	}
	position := int(C.cr_action_call_rule(call))
	return &ActionContext{
		rule:     run.ruleSet.rules[position],
		position: position,
		data:     run.data,
		decision: run.decision,
	}
}

// outcome is how one action ended: returned (with an error, or nil),
// panicked, or called runtime.Goexit; message is what the run's error says
// of it.
type outcome struct {
	err        error
	panicked   bool
	panicValue any
	exited     bool
	message    string
}

// runOne runs one action on the goroutine it is called on, and sends how it
// ended. Every user method it calls (the action, the error's Error) runs
// inside the recovered region; fmt.Sprint itself recovers from a panicking
// Error or String method and writes a nil receiver as <nil>.
func runOne(action Action, ctx *ActionContext, done chan<- outcome) {
	var o outcome
	returned := false
	defer func() {
		if !returned {
			// recover() is nil only when the goroutine is exiting through
			// runtime.Goexit: a panic(nil) is a *runtime.PanicNilError.
			if value := recover(); value != nil {
				o = outcome{panicked: true, panicValue: value, message: "the action panicked: " + fmt.Sprint(value)}
			} else {
				o = outcome{exited: true, message: "the action called runtime.Goexit (t.FailNow or t.Fatal, for example)"}
			}
		}
		done <- o
	}()
	if err := action(ctx); err != nil {
		o = outcome{err: err, message: fmt.Sprint(err)}
	}
	returned = true
}

// registry is a registry of the C ABI for one run, with the Go handles and
// the C memory that carry its actions; free releases all of them.
type registry struct {
	actions *C.cr_actions
	handles []cgo.Handle
	slots   []unsafe.Pointer
}

// newRegistry registers each action under its name, in name order, with the
// trampoline as the function and, as its data, C memory holding a
// cgo.Handle of the action and its run: the library never holds a Go
// pointer.
func newRegistry(actions Actions, run *runState) (*registry, error) {
	reg := &registry{actions: C.cr_actions_new()}
	names := make([]string, 0, len(actions))
	for name := range actions {
		names = append(names, name)
	}
	slices.Sort(names)
	for _, name := range names {
		h := cgo.NewHandle(&actionEntry{actions[name], run})
		reg.handles = append(reg.handles, h)
		slot := C.malloc(C.size_t(unsafe.Sizeof(C.uintptr_t(0))))
		if slot == nil {
			panic("cleanrules: out of memory registering an action")
		}
		reg.slots = append(reg.slots, slot)
		*(*C.uintptr_t)(slot) = C.uintptr_t(h)
		p, n := text(name)
		var e *C.cr_error
		status := C.cr_actions_add(reg.actions, p, n, C.cr_action(C.cleanrulesAction), slot, &e)
		if err := check(status, e); err != nil {
			reg.free()
			return nil, err
		}
	}
	return reg, nil
}

func (reg *registry) free() {
	C.cr_actions_free(reg.actions)
	for _, h := range reg.handles {
		h.Delete()
	}
	for _, slot := range reg.slots {
		C.free(slot)
	}
	reg.handles, reg.slots = nil, nil
}
