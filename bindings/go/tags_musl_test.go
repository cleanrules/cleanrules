//go:build cleanrules_musl

package cleanrules

func init() { buildTags = append(buildTags, "cleanrules_musl") }
