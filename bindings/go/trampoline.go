package cleanrules

// This file holds the function the library calls for every action. A file
// with //export may only declare C in its preamble, so the registry that
// takes its address is in actions.go.

/*
#include <stdint.h>
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"runtime/cgo"
	"unsafe"
)

//export cleanrulesAction
func cleanrulesAction(data unsafe.Pointer, call *C.cr_action_call) C.int32_t {
	return C.int32_t(runAction(data, call))
}

// runAction runs the action whose cgo.Handle data holds. The action runs on
// a goroutine of its own (runOne), which this callback waits for: a panic or
// a runtime.Goexit there ends that goroutine, never this one, so nothing
// unwinds into the library's frames (cleanrules.h: an action "must not
// unwind or jump out of the call"). How the action ended is recorded in the
// run and given to the library as the action's failure, and the callback
// returns 1, which stops the run; Run then returns the error, or panics or
// calls runtime.Goexit again. What runs here on this goroutine is the
// package's own code, recovered as well, as a last guard.
func runAction(data unsafe.Pointer, call *C.cr_action_call) (status int32) {
	entry := cgo.Handle(uintptr(*(*C.uintptr_t)(data))).Value().(*actionEntry)
	run := entry.run
	defer func() {
		if value := recover(); value != nil {
			run.panicked, run.panicValue = true, value
			fail(call, "the action panicked: "+fmt.Sprint(value))
			status = 1
		}
	}()
	done := make(chan outcome, 1)
	go runOne(entry.action, entry.context(call), done)
	o := <-done
	switch {
	case o.panicked:
		run.panicked, run.panicValue = true, o.panicValue
	case o.exited:
		run.exited = true
	case o.err != nil:
		run.err = o.err
	default:
		return 0
	}
	fail(call, o.message)
	return 1
}

// fail tells the library why the action failed: the cause of the
// action_error.
func fail(call *C.cr_action_call, message string) {
	p, n := text(message)
	C.cr_action_call_fail(call, p, n)
}
