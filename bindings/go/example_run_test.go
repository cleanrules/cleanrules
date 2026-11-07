package cleanrules

import (
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
)

// TestDiscountsExample builds docs/examples/discounts.go, runs it and checks
// its output. It builds with go build rather than go run, whose executables
// Go caches without knowing the Rust library they link.
func TestDiscountsExample(t *testing.T) {
	if testing.Short() {
		t.Skip("builds and runs a program")
	}
	goTool := filepath.Join(runtime.GOROOT(), "bin", "go")
	program := filepath.Join(t.TempDir(), "discounts")
	if runtime.GOOS == "windows" {
		program += ".exe"
	}
	build := exec.Command(goTool, "build", "-o", program, filepath.Join("..", "..", "docs", "examples", "discounts.go"))
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("%v\n%s", err, out)
	}
	out, err := exec.Command(program).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, out)
	}
	if string(out) != discountsOutput {
		t.Errorf("got:\n%s\nwant:\n%s", out, discountsOutput)
	}
}

const discountsOutput = `discount: 0.15
fired: [vip loyal]

HIT POLICY priority: selected vip
[ ] blocked (not fired): customer.blocked
    blocked customers never get a discount
    - customer.blocked is false (customer.blocked = FALSE)
[x] vip (selected): customer.years > 5 AND order.total > 1000
    because all of these hold:
    - customer.years > 5 is true (customer.years = 7)
    - order.total > 1000 is true (order.total = 1500.00)
[x] loyal (fired, not selected): customer.years > 2 OR customer.newsletter
    fired, but vip has a higher priority (10 > 5)
    - customer.years > 2 is true (customer.years = 7)

vip: order.total > 1000 is false; needs gt, short by 100.00

Rule set: 3 rules, hit policy priority, logic boolean
  priority  name     condition                                  outputs
        99  blocked  customer.blocked                           discount
        10  vip      customer.years > 5 AND order.total > 1000  discount DO notify_sales
         5  loyal    customer.years > 2 OR customer.newsletter  discount
Fields read (4):
  customer.blocked     conditions of blocked
  customer.years       conditions of vip, loyal
  order.total          conditions of vip
  customer.newsletter  conditions of loyal
Outputs: discount
Tags: loyalty (vip, loyal)

notify_sales for vip: 0.15

missing_field in rule adult: [customer age]
`
