package cleanrules

/*
#include "cleanrules.h"
*/
import "C"

import (
	"fmt"
	"sync"
)

// Limits bound what reading a document, a rule, a decision or a decision
// table may take (spec/json-format.md, Limits; spec/tables.md, Limits), and
// how many values an evaluation reads from Go data (MaxValues), as the C
// ABI's cr_limits does. Pass them with WithLimits. A field left at 0 keeps
// the core's default, so Limits{MaxDepth: 32} changes one limit; a negative
// field is a format_error that names it.
//
// MaxSize is the size limit of every format at once, and MaxYAMLSize and
// MaxTextSize then change those of YAML and of text and CSV:
//
//	cleanrules.Limits{MaxSize: 1 << 20}                    // JSON, YAML and text: 1 MiB
//	cleanrules.Limits{MaxSize: 1 << 20, MaxYAMLSize: 4096} // JSON and text 1 MiB, YAML 4 KiB
//	cleanrules.Limits{MaxDepth: 32}                        // sizes as by default
//
// DefaultLimits gives every field set, to the defaults, so a change of
// MaxSize alone does not reach YAML and text there:
//
//	limits := cleanrules.DefaultLimits()
//	limits.MaxSize = 64 << 20 // JSON only: MaxYAMLSize and MaxTextSize are set
type Limits struct {
	// MaxSize is the largest text, in bytes: of every format, unless
	// MaxYAMLSize or MaxTextSize is set; read from DefaultLimits, the JSON
	// one.
	MaxSize int
	// MaxNesting is the deepest nesting of JSON arrays and objects.
	MaxNesting int
	// MaxDepth is the deepest condition or payload value, in nodes.
	MaxDepth int
	// MaxTableSize is the most cells and nodes a decision table may build.
	MaxTableSize int
	// MaxYAMLSize is the largest YAML text, in bytes.
	MaxYAMLSize int
	// MaxTextSize is the largest text in the rule syntax or CSV, in bytes.
	MaxTextSize int
	// MaxValues is the most values an evaluation reads from the Go data of
	// one input, 1,000,000 by default (spec/data-model.md, Reading native
	// data, Limits): each value placed where a path ends or stops, with
	// everything inside it, and each position of a slice or an array the
	// paths walk through. Beyond it the input is refused, strict or not,
	// with a limit_exceeded error whose Name is "max_values" (and Row, in a
	// batch). It bounds RuleSet.Evaluate, EvaluateMany and Run,
	// DecisionTable.Evaluate and EvaluateMany, and Expr.EvaluateTree and
	// EvaluateValueTree; constants and metadata are read under the default.
	MaxValues int
}

// limitCodes are the limits in the order they are set: MaxSize first, since
// it sets the YAML and text limits too.
var limitCodes = []C.cr_limit{
	C.CR_LIMIT_MAX_SIZE, C.CR_LIMIT_MAX_NESTING, C.CR_LIMIT_MAX_DEPTH,
	C.CR_LIMIT_MAX_TABLE_SIZE, C.CR_LIMIT_MAX_YAML_SIZE, C.CR_LIMIT_MAX_TEXT_SIZE, C.CR_LIMIT_MAX_VALUES,
}

var limitNames = []string{"MaxSize", "MaxNesting", "MaxDepth", "MaxTableSize", "MaxYAMLSize", "MaxTextSize", "MaxValues"}

func (l *Limits) fields() []*int {
	return []*int{&l.MaxSize, &l.MaxNesting, &l.MaxDepth, &l.MaxTableSize, &l.MaxYAMLSize, &l.MaxTextSize, &l.MaxValues}
}

// DefaultLimits are the core's default limits, every field set (MaxSize
// the JSON one).
func DefaultLimits() Limits {
	native := C.cr_limits_new()
	defer C.cr_limits_free(native)
	return limitsOf(native)
}

// defaultMaxValues is the core's default max_values, for constants and
// metadata and for evaluations given no MaxValues.
var defaultMaxValues = sync.OnceValue(func() int { return DefaultLimits().MaxValues })

func limitsOf(native *C.cr_limits) Limits {
	var l Limits
	for i, field := range l.fields() {
		var value C.size_t
		C.cr_limits_get(native, limitCodes[i], &value, nil)
		*field = int(value)
	}
	return l
}

// native gives the limits to the C ABI, freed with cr_limits_free; nil (the
// defaults) for nil. A negative field is a format_error.
func (l *Limits) native() (*C.cr_limits, error) {
	if l == nil {
		return nil, nil
	}
	for i, field := range l.fields() {
		if *field < 0 {
			return nil, newError(CodeFormat,
				fmt.Sprintf("Limits.%s is %d: a limit is 0 or more", limitNames[i], *field),
				"leave it at 0 for the default, or give the most it allows")
		}
	}
	native := C.cr_limits_new()
	for i, field := range l.fields() {
		if *field > 0 {
			C.cr_limits_set(native, limitCodes[i], C.size_t(*field), nil)
		}
	}
	return native, nil
}

// effective is what the limits are once the defaults fill the fields left
// at 0.
func (l *Limits) effective() (Limits, error) {
	native, err := l.native()
	if err != nil {
		return Limits{}, err
	}
	if native == nil {
		return DefaultLimits(), nil
	}
	defer C.cr_limits_free(native)
	return limitsOf(native), nil
}
