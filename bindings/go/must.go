package cleanrules

// Must gives v, and panics if err is not nil: for rule sets written in the
// program, which cannot fail once they are right, as template.Must and
// regexp.MustCompile do. Check err where rules come from users or data.
//
//	rules := cleanrules.Must(cleanrules.NewRuleSet(
//		[]*cleanrules.Rule{cleanrules.Must(cleanrules.NewRule("adult", cleanrules.Field("age").Ge(18)))}))
func Must[T any](v T, err error) T {
	if err != nil {
		panic(err)
	}
	return v
}
