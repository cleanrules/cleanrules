//go:build ignore

// The rules of discounts.rules, built in code with the Go binding (task
// H.1a): conditions with functions and methods, rules with options and a rule
// set, Go data in and Go values out (a cleanrules.Decimal discount), the
// explanation as objects and as text, why a rule did not fire, the summary, a
// named action bound to a Go function, and a strict-mode error as a value.
//
//	cargo build -p cleanrules-ffi
//	cd bindings/go && go run ../../docs/examples/discounts.go
//
// bindings/go's tests run it too (TestDiscountsExample).
package main

import (
	"errors"
	"fmt"
	"log"

	cr "github.com/cleanrules/cleanrules-go"
)

// Customer is read through its exported fields, named by their tags.
type Customer struct {
	Years      int  `json:"years"`
	Newsletter bool `json:"newsletter"`
	Blocked    bool `json:"blocked"`
	notes      string
}

func main() {
	blocked := cr.Must(cr.NewRule("blocked", cr.Field("customer", "blocked"),
		cr.Then("discount", cr.MustDecimal("0")),
		cr.WithPriority(99),
		cr.WithDescription("blocked customers never get a discount")))
	vip := cr.Must(cr.NewRule("vip",
		cr.Field("customer", "years").Gt(5).And(cr.Field("order", "total").Gt(1000)),
		cr.Then("discount", cr.MustDecimal("0.15")),
		cr.WithPriority(10),
		cr.WithTags("loyalty"),
		cr.WithAction("notify_sales")))
	loyal := cr.Must(cr.NewRule("loyal",
		cr.Field("customer", "years").Gt(2).Or(cr.Field("customer", "newsletter")),
		cr.Then("discount", cr.MustDecimal("0.05")),
		cr.WithPriority(5),
		cr.WithTags("loyalty")))
	rules := cr.Must(cr.NewRuleSet([]*cr.Rule{blocked, vip, loyal}, cr.WithHitPolicy(cr.HitPolicyPriority)))

	// Go data in, Go values out: the discount is a cleanrules.Decimal.
	order := map[string]any{
		"customer": Customer{Years: 7, notes: "never read"},
		"order":    map[string]any{"total": cr.MustDecimal("1500.00")},
	}
	decision := cr.Must(rules.Evaluate(order))
	output := cr.Must(decision.Output())
	fmt.Println("discount:", output["discount"])
	fmt.Println("fired:", decision.Fired())
	fmt.Println()
	fmt.Println(cr.Must(decision.Explain()))
	fmt.Println()

	// Why a rule did not fire, as objects.
	small := cr.Must(rules.Evaluate(map[string]any{
		"customer": &Customer{Years: 7},
		"order":    map[string]any{"total": cr.MustDecimal("900.00")},
	}))
	for _, cause := range cr.Must(small.WhyNot("vip")).Causes() {
		needed := ""
		if c := cause.Counterfactual; c != nil {
			needed = fmt.Sprintf("; needs %s, short by %v", c.Op, c.Gap)
		}
		fmt.Printf("vip: %v is %v%s\n", cr.Must(cause.Node()), cause.Truth, needed)
	}
	fmt.Println()
	fmt.Println(cr.Must(rules.Summary()))
	fmt.Println()

	// A named action is data; the program binds it to a Go function.
	bound := cr.Must(rules.Bind(cr.Actions{
		"notify_sales": func(ctx *cr.ActionContext) error {
			fmt.Printf("notify_sales for %s: %v\n", ctx.Rule().Name(), ctx.Output()["discount"])
			return nil
		},
	}))
	cr.Must(bound.Run(order))

	// Strict mode gives an error instead of unknown.
	strict := cr.Must(cr.NewRuleSet(
		[]*cr.Rule{cr.Must(cr.NewRule("adult", cr.Field("customer", "age").Ge(18)))},
		cr.WithStrict(true)))
	_, err := strict.Evaluate(map[string]any{"customer": map[string]any{}})
	var e *cr.Error
	if !errors.As(err, &e) || !errors.Is(err, cr.ErrMissingField) {
		log.Fatalf("expected a missing_field error, got %v", err)
	}
	fmt.Println()
	fmt.Printf("%s in rule %s: %v\n", e.Code, e.Rule, e.Path)
}
