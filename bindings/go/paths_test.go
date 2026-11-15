package cleanrules

import (
	"errors"
	"runtime"
	"testing"
)

// The limit max_path_segments on the paths of fields (spec/json-format.md,
// Errors and limits), and the depth of the Go data read along them.

// longPath is a path of n segments, keys and indexes in turns, and data as
// deep along it, with 1 at its end.
func longPath(n int) ([]any, any) {
	path := make([]any, n)
	for i := range path {
		if i%2 == 0 {
			path[i] = "k"
		} else {
			path[i] = 0
		}
	}
	var data any = 1
	for i := n - 1; i >= 0; i-- {
		if i%2 == 0 {
			data = map[string]any{"k": data}
		} else {
			data = []any{data}
		}
	}
	return path, data
}

func pathLimitError(t *testing.T, what string, err error) {
	t.Helper()
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeLimitExceeded || e.Name != "max_path_segments" ||
		!errors.Is(err, ErrFormat) {
		t.Errorf("%s: %v, want limit_exceeded for max_path_segments", what, err)
	}
}

func TestPathSegments(t *testing.T) {
	if got := DefaultLimits().MaxPathSegments; got != 256 {
		t.Fatalf("default max_path_segments %d", got)
	}
	path, data := longPath(256)
	if err := Field(path...).Err(); err != nil {
		t.Fatal(err)
	}
	longer, _ := longPath(257)
	pathLimitError(t, "Field", Field(longer...).Err())
	// Readers take the limit, and lower or raise it.
	_, err := RuleFromText("IF a.b.c THEN y = 1", WithLimits(Limits{MaxPathSegments: 2}))
	pathLimitError(t, "RuleFromText", err)
	if _, err := RuleFromText("IF a.b.c THEN y = 1", WithLimits(Limits{MaxPathSegments: 3})); err != nil {
		t.Error(err)
	}
	// A rule set reading a path at the limit, over Go data as deep, on a
	// thread of the Go runtime that is not the main one.
	rule := must(NewRule("r", Field(path...).Eq(1), Then("y", Field(path...))))(t)
	rules := must(NewRuleSet([]*Rule{rule}))(t)
	done := make(chan error)
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		decision, err := rules.Evaluate(data)
		if err == nil && len(decision.Selected()) != 1 {
			err = errors.New("the rule did not fire")
		}
		done <- err
	}()
	if err := <-done; err != nil {
		t.Fatal(err)
	}
}
