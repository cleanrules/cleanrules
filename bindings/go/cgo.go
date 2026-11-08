package cleanrules

/*
#include <stdlib.h>
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"unsafe"
)

// FormatVersion is the version of the JSON format this package reads and
// writes (spec/json-format.md).
const FormatVersion = 1

// ABIVersion is the version of the C ABI this package is written for.
const ABIVersion = 1

func init() {
	if C.CR_ABI_VERSION != ABIVersion {
		panic(fmt.Sprintf("cleanrules: cleanrules.h is for the C ABI version %d, this package for %d", C.CR_ABI_VERSION, ABIVersion))
	}
	// The library is linked statically, so a mismatch is a stale build of
	// it: say so at once rather than misbehave later.
	if abi := C.cr_abi_version(); abi != C.CR_ABI_VERSION {
		panic(fmt.Sprintf("cleanrules: the linked C ABI is version %d, this package needs %d: rebuild it with cargo build -p cleanrules-ffi", abi, ABIVersion))
	}
	if format := C.cr_format_version(); format != FormatVersion {
		panic(fmt.Sprintf("cleanrules: the linked library writes format version %d, this package reads %d: rebuild it with cargo build -p cleanrules-ffi", format, FormatVersion))
	}
}

// Version is the version of the linked library, such as "0.1.0".
func Version() string {
	return C.GoString(C.cr_version())
}

// text gives a Go string to the C ABI for the duration of a call: a pointer
// to its bytes, which hold no Go pointers, and its length. The library only
// reads it (cleanrules.h, "Texts in"). An empty string is NULL with length 0.
func text(s string) (*C.char, C.size_t) {
	if len(s) == 0 {
		return nil, 0
	}
	return (*C.char)(unsafe.Pointer(unsafe.StringData(s))), C.size_t(len(s))
}

// goString copies len bytes at p, borrowed from the library, into a string.
func goString(p *C.char, n C.size_t) string {
	if p == nil || n == 0 {
		return ""
	}
	return string(unsafe.Slice((*byte)(unsafe.Pointer(p)), int(n)))
}

// takeString copies a text the library gave through a char ** out-parameter
// and frees it.
func takeString(p *C.char, n C.size_t) string {
	defer C.cr_string_free(p)
	return goString(p, n)
}

// staticString reads a static NUL-terminated string of the library, or "".
func staticString(p *C.char) string {
	if p == nil {
		return ""
	}
	return C.GoString(p)
}
