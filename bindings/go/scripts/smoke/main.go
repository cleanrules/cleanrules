// Command smoke uses the Go binding as a program outside this repository
// does: CI (.github/workflows/go-libs.yml) copies it into a clean module
// whose go.mod replaces the binding's module with the assembled one, with
// its prebuilt libraries (bindings/go/scripts/assemble.py), and runs it on
// every platform it can: no cargo, no target directory. It reads a rule set
// from YAML with an action bound, builds a decision table, reads one rule
// from text, evaluates a result tree and explains a decision, and exits
// with status 1 at the first result that is not the expected one.
package main

import (
	"fmt"
	"os"
	"runtime"
	"strings"

	cr "github.com/cleanrules/cleanrules/bindings/go"
)

const rules = `version: 1
ruleset:
  hit_policy: priority
  logic: boolean
  rules:
    - name: vip
      priority: 10
      when: {op: gt, left: {op: field, path: [customer, years]}, right: {op: const, value: 5}}
      then: {discount: {op: const, value: {$decimal: "0.15"}}}
      do: notify
    - name: loyal
      when: {op: gt, left: {op: field, path: [customer, years]}, right: {op: const, value: 2}}
      then: {discount: {op: const, value: {$decimal: "0.05"}}}
`

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "smoke:", err)
		os.Exit(1)
	}
	fmt.Printf("smoke: cleanrules %s on %s/%s: ok\n", cr.Version(), runtime.GOOS, runtime.GOARCH)
}

func check(what string, got, want any) error {
	if fmt.Sprint(got) != fmt.Sprint(want) {
		return fmt.Errorf("%s: got %v, want %v", what, got, want)
	}
	return nil
}

func run() error {
	order := map[string]any{"customer": map[string]any{"years": 7}, "order": map[string]any{"total": 6000}}

	// A rule set from YAML, its action bound by name.
	notified := ""
	doc, err := cr.Loads(rules, cr.WithFormat(cr.FormatYAML), cr.WithActions(cr.Actions{
		"notify": func(ctx *cr.ActionContext) error {
			tree, err := ctx.Tree()
			if err != nil {
				return err
			}
			notified = ctx.Rule().Name() + ": " + tree.Node().String()
			return nil
		},
	}))
	if err != nil {
		return err
	}
	ruleSet, ok := doc.(*cr.RuleSet)
	if !ok {
		return fmt.Errorf("the document holds a %T", doc)
	}
	decision, err := ruleSet.Run(order)
	if err != nil {
		return err
	}
	output, err := decision.Output()
	if err != nil {
		return err
	}
	if err := check("discount", output["discount"], cr.MustDecimal("0.15")); err != nil {
		return err
	}
	if err := check("action", notified, "vip: customer.years > 5"); err != nil {
		return err
	}
	explanation, err := decision.Explain()
	if err != nil {
		return err
	}
	if err := check("explanation", strings.SplitN(explanation.String(), "\n", 2)[0], "HIT POLICY priority: selected vip"); err != nil {
		return err
	}

	// A decision table built in code, evaluated and written as CSV.
	table, err := cr.NewDecisionTable(
		[]any{"RULE", "customer.years", cr.Field("order", "total"), "THEN risk"},
		[][]any{
			{"new", "< 1", "> 1000", "'high'"},
			{"big", nil, cr.Field("order", "total").Gt(5000), "'medium'"},
			{"default", nil, nil, "'low'"},
		})
	if err != nil {
		return err
	}
	risk, err := table.Evaluate(order)
	if err != nil {
		return err
	}
	if err := check("risk", risk.Fired(), []string{"big", "default"}); err != nil {
		return err
	}
	csv, err := cr.Dumps(table, cr.WithFormat(cr.FormatCSV))
	if err != nil {
		return err
	}
	if err := check("csv", strings.Count(csv, "\n"), 5); err != nil {
		return err
	}

	// One rule from text, and a result tree.
	rule, err := cr.RuleFromText("RULE vip IF customer.years > 5 AND order.total > 1000 THEN discount = 0.15")
	if err != nil {
		return err
	}
	tree, err := rule.When().EvaluateTree(map[string]any{"customer": map[string]any{"years": 7}})
	if err != nil {
		return err
	}
	return check("tree", fmt.Sprint(tree.Truth, " ", tree.Children[1].Reason.Code), "unknown missing_field")
}
