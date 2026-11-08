package cleanrules

import (
	"errors"
	"runtime"
	"sync"
	"unsafe"
)

// ErrClosed is returned by a method of a RuleSet, a Decision or a
// DecisionTable used after Close, and of a nil or zero one (one not made by
// this package). It is not an *Error, nor are the errors of the files Load
// and Dump cannot read, write or tell the format of.
var ErrClosed = errors.New("cleanrules: the object is closed")

// handle owns one object of the C ABI and frees it once: when Close is
// called, or when every Go object that holds it is unreachable (a cleanup of
// runtime.AddCleanup), and never during a call that uses it. Each call
// brackets its use with acquire and release, so closing while another
// goroutine evaluates frees the object when that call returns, and a cleanup
// that runs during a call cannot free it either. The library's handles
// never change once made (cleanrules.h, "Threads"), so any number of calls
// may use one at once.
type handle struct {
	mu     sync.Mutex
	ptr    unsafe.Pointer
	free   func(unsafe.Pointer) // nil while the object is borrowed
	users  int                  // calls using ptr now
	owners int                  // Go objects whose cleanup has not run
	closed bool
}

// newHandle makes a handle of ptr, freed with free, owned by owner.
func newHandle[T any](owner *T, ptr unsafe.Pointer, free func(unsafe.Pointer)) *handle {
	h := &handle{ptr: ptr, free: free}
	own(h, owner)
	return h
}

// own adds owner as a holder of h: h is freed, at the latest, once every
// owner is unreachable. The cleanup refers to h, never to the owner.
func own[T any](h *handle, owner *T) {
	h.mu.Lock()
	h.owners++
	h.mu.Unlock()
	runtime.AddCleanup(owner, (*handle).drop, h)
}

// acquire gives the pointer for one call, or ErrClosed; every successful
// acquire is followed by one release. A nil handle, that of a zero value
// such as &RuleSet{}, acts as a closed one.
func (h *handle) acquire() (unsafe.Pointer, error) {
	if h == nil {
		return nil, ErrClosed
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.closed {
		return nil, ErrClosed
	}
	h.users++
	return h.ptr, nil
}

// release ends one call, freeing the object if it was closed meanwhile.
func (h *handle) release() {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.users--
	h.freeIfDone()
}

// close frees the object now, or when the calls using it return. Closing
// twice does nothing.
func (h *handle) close() {
	if h == nil {
		return
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	h.closed = true
	h.freeIfDone()
}

// drop is the cleanup of one owner: the last one closes the handle.
func (h *handle) drop() {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.owners--
	if h.owners <= 0 {
		h.closed = true
		h.freeIfDone()
	}
}

// adopt gives a borrowed handle the function that frees it, once the object
// becomes the caller's (a decision an action saw, when the run ends).
func (h *handle) adopt(free func(unsafe.Pointer)) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.free = free
	h.freeIfDone()
}

func (h *handle) freeIfDone() {
	if h.closed && h.users == 0 && h.ptr != nil && h.free != nil {
		h.free(h.ptr)
		h.ptr = nil
	}
}
