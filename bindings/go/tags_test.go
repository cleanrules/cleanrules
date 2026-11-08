package cleanrules

import "strings"

// buildTags are the build tags of the package's own tags (set by the
// tags_*_test.go files that they select), which the tests that build
// programs pass on: a program built without them would link another
// library.
var buildTags []string

// tagsFlag is the -tags flag of buildTags, or "".
func tagsFlag() string {
	return "-tags=" + strings.Join(buildTags, ",")
}
