//go:build ignore

// The decision table of risk.csv with the Go binding (task H.1b), as
// RiskTable.java and risk.mjs do: read from its file, built again from rows
// in code, evaluated and explained, written as YAML; then one rule read from
// text, a result tree, and a rule set written as a document and read back
// with its action bound by name.
//
//	cargo build -p cleanrules-ffi
//	(cd bindings/go && go run ../../docs/examples/risk_table.go ../../docs/examples/risk.csv)
package main

import (
	"fmt"
	"log"
	"os"
	"strings"

	cr "github.com/cleanrules/cleanrules/bindings/go"
)

func main() {
	// A decision table from its CSV file (the format comes from the extension).
	doc, err := cr.Load(os.Args[1])
	if err != nil {
		log.Fatal(err)
	}
	table, ok := doc.(*cr.DecisionTable)
	if !ok {
		log.Fatalf("%s holds a %T, not a decision table", os.Args[1], doc)
	}
	order := map[string]any{"customer": map[string]any{"years": 9}, "order": map[string]any{"total": 6000}}
	decision, err := table.Evaluate(order)
	if err != nil {
		log.Fatal(err)
	}
	output, err := decision.Output()
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("risk: %v, discount: %v\n", output["risk"], output["discount"])
	explanation, err := decision.Explain()
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(strings.SplitN(explanation.String(), "\n", 2)[0])

	// The same table built from rows: strings are cells as the CSV writes
	// them, nil is an empty cell, other values are constants, and a
	// TableOutput lists an output's values.
	built, err := cr.NewDecisionTable(
		[]any{"RULE", cr.Field("customer", "years"), "order.total", "THEN discount",
			cr.TableOutput{Name: "risk", Values: []any{"high", "medium", "low"}}, "DESCRIPTION"},
		[][]any{
			{"new", cr.Field("customer", "years").Lt(1), "> 1000", 0, "'high'", "New customers with large orders are checked first"},
			{"vip", "> 5", "-", cr.MustDecimal("0.15"), "'low'", "Long-standing customers"},
			{"big", nil, "> 5000", cr.MustDecimal("0.05"), "'medium'"},
			{"default", nil, nil, 0, "'low'"},
		},
		cr.WithTablePolicy(cr.TablePolicyPriority))
	if err != nil {
		log.Fatal(err)
	}
	fromRows := cr.Must(cr.Dumps(built, cr.WithFormat(cr.FormatCSV)))
	fromFile := cr.Must(cr.Dumps(table, cr.WithFormat(cr.FormatCSV)))
	fmt.Println("same table:", fromRows == fromFile)
	var names []string
	for _, row := range cr.Must(built.Rows()) {
		names = append(names, row.Name)
	}
	fmt.Println("rows:", strings.Join(names, ", "))
	yaml := cr.Must(cr.Dumps(built, cr.WithFormat(cr.FormatYAML)))
	fmt.Println(strings.Join(strings.SplitN(yaml, "\n", 4)[:3], "\n"))

	// One rule from text (fuzzytool's verb), and why its condition is unknown.
	rule := cr.Must(cr.RuleFromText("RULE vip IF customer.years > 5 AND order.total > 1000 THEN discount = 0.15"))
	tree := cr.Must(rule.When().EvaluateTree(map[string]any{"customer": map[string]any{"years": 7}}))
	total := tree.Children[1]
	fmt.Printf("%s: %s; %s is %s (%s)\n", rule.Name(), tree.Truth, total.Node(), total.Truth, total.Reason.Code)

	// A rule set written as a document and read back, its action bound by name.
	rules := cr.Must(cr.NewRuleSet([]*cr.Rule{
		cr.Must(cr.RuleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15 DO notify")),
	}))
	text := cr.Must(cr.Dumps(rules, cr.WithFormat(cr.FormatYAML)))
	read, err := cr.Loads(text, cr.WithFormat(cr.FormatYAML), cr.WithActions(cr.Actions{
		"notify": func(ctx *cr.ActionContext) error {
			why, err := ctx.Tree()
			if err != nil {
				return err
			}
			fmt.Printf("notify for %s: %s is %s\n", ctx.Rule().Name(), why.Node(), why.Truth)
			return nil
		},
	}))
	if err != nil {
		log.Fatal(err)
	}
	switch back := read.(type) {
	case *cr.RuleSet:
		if _, err := back.Run(order); err != nil {
			log.Fatal(err)
		}
	default:
		log.Fatalf("the document holds a %T, not a rule set", read)
	}
}
