package cleanrules_test

import (
	"errors"
	"fmt"

	cr "github.com/cleanrules/cleanrules/bindings/go"
)

// Conditions are built with functions and methods; String is the core's
// text of them.
func ExampleField() {
	adult := cr.Field("customer", "age").Ge(18)
	cond := adult.And(cr.Field("country").IsIn("PT", "ES"))
	fmt.Println(cond)
	fmt.Println(cr.Not(cr.Field("blocked")).Or(cr.Field("vip")))
	fmt.Println(cr.Field("order", "total").Mul(0.9).Gt(cr.Field("limit")))
	// Output:
	// customer.age >= 18 AND country IN ("ES", "PT")
	// NOT blocked OR vip
	// order.total * 0.9e0 > limit
}

// An expression that breaks a construction rule carries its error, which
// NewRule returns.
func ExampleExpr_Err() {
	cond := cr.Field("x").Add("a").Gt(1)
	_, err := cr.NewRule("r", cond)
	fmt.Println(errors.Is(err, cr.ErrExpression), err == cond.Err())
	// Output: true true
}

type Order struct {
	Customer struct {
		Years int `json:"years"`
	} `json:"customer"`
	Total cr.Decimal `cleanrules:"total"`
}

// A rule set decides on Go data and gives Go values back.
func ExampleRuleSet_Evaluate() {
	vip := cr.Must(cr.NewRule("vip",
		cr.Field("customer", "years").Gt(5).And(cr.Field("total").Gt(1000)),
		cr.Then("discount", cr.MustDecimal("0.15"))))
	base := cr.Must(cr.NewRule("base", cr.Const(true), cr.Then("discount", cr.MustDecimal("0"))))
	rules := cr.Must(cr.NewRuleSet([]*cr.Rule{vip, base}))

	var order Order
	order.Customer.Years = 7
	order.Total = cr.MustDecimal("1500.00")
	decision := cr.Must(rules.Evaluate(order))
	output := cr.Must(decision.Output())
	fmt.Println(decision.Fired(), output["discount"])
	// Output: [vip base] 0.15
}

// WhyNot explains a rule that did not fire.
func ExampleDecision_WhyNot() {
	rules := cr.Must(cr.NewRuleSet([]*cr.Rule{
		cr.Must(cr.NewRule("big", cr.Field("x").Gt(10))),
	}))
	decision := cr.Must(rules.Evaluate(map[string]int{"x": 8}))
	why := cr.Must(decision.WhyNot("big"))
	fmt.Println(why.Status)
	fmt.Println(why.Causes()[0])
	fmt.Println(why.Causes()[0].Counterfactual.Gap)
	// Output:
	// not_fired
	// x > 10 is false (x = 8); short by 2
	// 2
}

// Run calls the Go functions bound to the actions the selected rules name.
func ExampleRuleSet_Run() {
	notify := cr.Must(cr.NewRule("notify", cr.Field("total").Gt(100), cr.WithAction("email")))
	rules := cr.Must(cr.NewRuleSet([]*cr.Rule{notify}))
	bound := cr.Must(rules.Bind(cr.Actions{
		"email": func(ctx *cr.ActionContext) error {
			fmt.Println("email for", ctx.Rule().Name())
			return nil
		},
	}))
	cr.Must(bound.Run(map[string]any{"total": 150}))
	// Output: email for notify
}

// Errors are values: an *Error with a code and its context, matched with
// errors.Is and errors.As.
func ExampleError() {
	rules := cr.Must(cr.NewRuleSet(
		[]*cr.Rule{cr.Must(cr.NewRule("adult", cr.Field("customer", "age").Ge(18)))},
		cr.WithStrict(true)))
	_, err := rules.Evaluate(map[string]any{"customer": map[string]any{}})
	var e *cr.Error
	if errors.As(err, &e) {
		fmt.Println(e.Code, e.Rule, e.Path)
	}
	fmt.Println(errors.Is(err, cr.ErrEvaluation))
	// Output:
	// missing_field adult [customer age]
	// true
}
