package cleanrules

import (
	"errors"
	"runtime"
	"sync"
	"testing"
	"time"
	"unsafe"
)

// TestClose checks Close: twice does nothing, a closed object gives
// ErrClosed, and decisions outlive their rule set.
func TestClose(t *testing.T) {
	rs := pricing(t)
	d := must(rs.Evaluate(map[string]any{"customer": map[string]any{"years": 3}}))(t)
	if err := rs.Close(); err != nil {
		t.Fatal(err)
	}
	if err := rs.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := rs.Evaluate(nil); !errors.Is(err, ErrClosed) {
		t.Errorf("Evaluate after Close: %v", err)
	}
	for name, err := range map[string]error{
		"Run":     func() error { _, err := rs.Run(nil); return err }(),
		"Summary": func() error { _, err := rs.Summary(); return err }(),
		"Rule":    func() error { _, err := rs.Rule("vip"); return err }(),
		"Bind":    func() error { _, err := rs.Bind(nil); return err }(),
		"Many":    func() error { _, err := rs.EvaluateMany([]any{}); return err }(),
	} {
		if !errors.Is(err, ErrClosed) {
			t.Errorf("%s after Close: %v", name, err)
		}
	}
	// What needs no native call still works.
	if rs.Len() != 3 || rs.HitPolicy() != HitPolicyFirst {
		t.Error("a closed rule set's settings")
	}
	if _, err := rs.FieldPaths(); !errors.Is(err, ErrClosed) {
		t.Errorf("FieldPaths after Close: %v", err)
	}
	// The decision keeps its own reference to the native rule set.
	explanation := must(d.Explain())(t)
	if explanation.Selected[0] != "loyal" {
		t.Error("a decision after its rule set closed")
	}
	// Closing a decision keeps what it says, and what was explained.
	d2 := must(pricing(t).Evaluate(map[string]any{}))(t)
	if err := d2.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := d2.Explain(); !errors.Is(err, ErrClosed) || len(d2.Results()) != 3 {
		t.Errorf("Explain after Close: %v", err)
	}
	if err := d.Close(); err != nil || must(d.Explain())(t) != explanation {
		t.Error("an explanation made before Close")
	}
	// Bind shares the native rule set.
	shared := pricing(t)
	bound := must(shared.Bind(Actions{}))(t)
	_ = bound.Close()
	if _, err := shared.Evaluate(nil); !errors.Is(err, ErrClosed) {
		t.Errorf("Bind's rule set shares Close: %v", err)
	}
}

// TestCleanup checks that the native objects are freed once their Go
// objects are unreachable, and only then (runtime.AddCleanup).
func TestCleanup(t *testing.T) {
	handles := func() []*handle {
		rs := pricing(t)
		bound := must(rs.Bind(Actions{}))(t)
		d := must(rs.Evaluate(nil))(t)
		e := Field("x").Gt(1)
		r := rule(t, "r", e)
		runtime.KeepAlive(bound)
		return []*handle{rs.h, d.h, e.h, r.h}
	}()
	deadline := time.Now().Add(5 * time.Second)
	for {
		runtime.GC()
		freed := 0
		for _, h := range handles {
			h.mu.Lock()
			if h.ptr == nil {
				freed++
			}
			h.mu.Unlock()
		}
		if freed == len(handles) {
			return
		}
		if time.Now().After(deadline) {
			t.Fatalf("%d of %d handles freed", freed, len(handles))
		}
		time.Sleep(10 * time.Millisecond)
	}
}

// TestHandleUses checks that closing waits for the calls that use a handle.
func TestHandleUses(t *testing.T) {
	freed := 0
	owner := &Expr{}
	h2 := newHandle(owner, unsafe.Pointer(new(int)), func(unsafe.Pointer) { freed++ })
	ptr, err := h2.acquire()
	if err != nil || ptr == nil {
		t.Fatal(err)
	}
	h2.close()
	if freed != 0 {
		t.Error("freed during a call")
	}
	if _, err := h2.acquire(); !errors.Is(err, ErrClosed) {
		t.Error("acquired after close")
	}
	h2.release()
	if freed != 1 {
		t.Error("not freed when the call returned")
	}
	h2.close()
	if freed != 1 {
		t.Error("freed twice")
	}
	runtime.KeepAlive(owner)
}

// TestConcurrency evaluates one rule set on many goroutines, then closes it
// while they still evaluate.
func TestConcurrency(t *testing.T) {
	rs := pricing(t, WithHitPolicy(HitPolicyPriority))
	var wg sync.WaitGroup
	start := make(chan struct{})
	errs := make(chan error, 8)
	for g := range 8 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			for i := range 200 {
				years := (g + i) % 10
				d, err := rs.Evaluate(map[string]any{"customer": map[string]any{"years": years}, "order": map[string]any{"total": 2000}})
				if errors.Is(err, ErrClosed) {
					return
				}
				if err != nil {
					errs <- err
					return
				}
				out, _ := d.Output()
				want := MustDecimal("0")
				switch {
				case years > 5:
					want = MustDecimal("0.15")
				case years > 2:
					want = MustDecimal("0.05")
				}
				if out["discount"] != want {
					errs <- errors.New("wrong discount")
					return
				}
				if _, err := d.Explain(); err != nil {
					errs <- err
					return
				}
				if i == 100 && g == 0 {
					_ = rs.Close()
				}
			}
		}()
	}
	close(start)
	wg.Wait()
	close(errs)
	for err := range errs {
		t.Error(err)
	}
}
