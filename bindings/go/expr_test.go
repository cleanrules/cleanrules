package cleanrules

import (
	"errors"
	"math"
	"reflect"
	"strings"
	"testing"
	"time"
)

// TestOperators builds every node and checks its tag, its kind and the text
// the core writes for it.
func TestOperators(t *testing.T) {
	x := Field("x")
	madrid := must(time.LoadLocation("Europe/Madrid"))(t)
	tests := []struct {
		expr *Expr
		tag  string
		kind Kind
		text string
	}{
		{x.Eq(1), "eq", KindCondition, "x = 1"},
		{x.Ne("a"), "ne", KindCondition, `x != "a"`},
		{x.Lt(1.5), "lt", KindCondition, "x < 1.5e0"},
		{x.Le(MustDecimal("0.10")), "le", KindCondition, "x <= 0.10"},
		{x.Gt(Date{2026, 1, 2}), "gt", KindCondition, `x > DATE "2026-01-02"`},
		{x.Ge(DateTime{2026, 1, 2, 3, 4, 5, 6}), "ge", KindCondition, `x >= DATETIME "2026-01-02T03:04:05.000006"`},
		{x.Eq(time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)), "eq", KindCondition, `x = DATETIME "2026-01-02T03:04:05Z"`},
		{x.Eq(time.Date(2026, 1, 2, 3, 4, 5, 0, time.FixedZone("", 5400))), "eq", KindCondition, `x = DATETIME "2026-01-02T03:04:05+01:30"`},
		{x.Eq(time.Date(2026, 7, 2, 3, 4, 5, 0, madrid)), "eq", KindCondition, `x = DATETIME "2026-07-02T03:04:05+02:00[Europe/Madrid]"`},
		{x.Eq(true), "eq", KindCondition, "x = TRUE"},
		{x.Add(1), "add", KindValue, "x + 1"},
		{x.Sub(Field("y")), "sub", KindValue, "x - y"},
		{x.Mul(2), "mul", KindValue, "x * 2"},
		{x.Div(3), "div", KindValue, "x / 3"},
		{x.Neg(), "neg", KindValue, "-x"},
		{x.Abs(), "abs", KindValue, "ABS(x)"},
		{x.Gt(1).And(Field("y").Lt(2)), "and", KindCondition, "x > 1 AND y < 2"},
		{And(x.Gt(1), Field("y").Lt(2), Field("z")), "and", KindCondition, "x > 1 AND y < 2 AND z"},
		{x.Gt(1).Or(Field("y")), "or", KindCondition, "x > 1 OR y"},
		{Or(x.Gt(1), Field("y")), "or", KindCondition, "x > 1 OR y"},
		{Not(x.Gt(1)), "not", KindCondition, "NOT x > 1"},
		{x.Gt(1).Not(), "not", KindCondition, "NOT x > 1"},
		{x.IsIn("PT", "ES", "ES"), "in", KindCondition, `x IN ("ES", "PT")`},
		{x.IsIn([]int{3, 1}), "in", KindCondition, "x IN (1, 3)"},
		{x.Between(1, 9), "between", KindCondition, "x BETWEEN 1 AND 9"},
		{x.BetweenClosed(1, 9, ClosedLeft), "between", KindCondition, "x BETWEEN 1 AND 9 CLOSED LEFT"},
		{x.BetweenClosed(1, 9, ClosedRight), "between", KindCondition, "x BETWEEN 1 AND 9 CLOSED RIGHT"},
		{x.BetweenClosed(1, 9, ClosedNone), "between", KindCondition, "x BETWEEN 1 AND 9 CLOSED NONE"},
		{x.IsNull(), "is_null", KindCondition, "x IS NULL"},
		{x.Matches("^a.*"), "matches", KindCondition, `x MATCHES "^a.*"`},
		{x.StartsWith("a"), "starts_with", KindCondition, `x STARTS WITH "a"`},
		{x.EndsWith("b"), "ends_with", KindCondition, `x ENDS WITH "b"`},
		{x.Contains("c'd"), "contains", KindCondition, `x CONTAINS "c'd"`},
		{Field("items", 0, "price"), "field", KindAny, "items[0].price"},
		{Field("a.b"), "field", KindAny, "`a.b`"},
		{Const(true), "const", KindAny, "TRUE"},
		{Const(nil), "const", KindValue, "NULL"},
		{Const(nil).IsNull(), "is_null", KindCondition, "NULL IS NULL"},
		{Const(math.Inf(1)), "const", KindValue, `FLOAT "inf"`},
		{Const(int64(math.MinInt64)), "const", KindValue, "-9223372036854775808"},
	}
	for _, tt := range tests {
		t.Run(tt.text, func(t *testing.T) {
			if err := tt.expr.Err(); err != nil {
				t.Fatal(err)
			}
			if got := tt.expr.String(); got != tt.text {
				t.Errorf("text %q, want %q", got, tt.text)
			}
			if got := tt.expr.Tag(); got != tt.tag {
				t.Errorf("tag %q, want %q", got, tt.tag)
			}
			if got := tt.expr.Kind(); got != tt.kind {
				t.Errorf("kind %q, want %q", got, tt.kind)
			}
		})
	}
}

// TestConstantsBack checks that constants of each kind come back as the
// same Go values from a payload.
func TestConstantsBack(t *testing.T) {
	madrid := must(time.LoadLocation("Europe/Madrid"))(t)
	for _, v := range []any{
		true, int64(-3), 2.5, math.Copysign(0, -1), MustDecimal("1.50"), MustDecimal("inf"), "olé",
		Date{2026, 10, 31}, DateTime{2026, 10, 31, 1, 2, 3, 4},
		time.Date(2026, 10, 31, 1, 2, 3, 4000, time.UTC), time.Date(2026, 7, 1, 1, 2, 3, 0, madrid),
		time.Date(2026, 7, 1, 1, 2, 3, 0, time.FixedZone("", -3600)),
	} {
		rs := ruleSet(t, nil, rule(t, "r", Const(true), Then("v", v)))
		got := must(rs.Evaluate(nil))(t).Results()[0].Output["v"]
		if !sameValue(got, v) {
			t.Errorf("got %#v, want %#v", got, v)
		}
	}
	// Other integer kinds come back as int64.
	rs := ruleSet(t, nil, rule(t, "r", Const(true), Then("a", uint8(7)), Then("b", int32(-7))))
	output := must(rs.Evaluate(nil))(t).Results()[0].Output
	if !reflect.DeepEqual(output, map[string]any{"a": int64(7), "b": int64(-7)}) {
		t.Errorf("got %#v", output)
	}
}

// TestConstructionErrors checks that nodes that break a construction rule
// carry an error with its code, and that every node built on them does too.
func TestConstructionErrors(t *testing.T) {
	x := Field("x")
	tests := []struct {
		name string
		expr *Expr
		code Code
		text string // a part of the message
	}{
		{"empty path", Field(), CodeExpression, "path"},
		{"empty key", Field(""), CodeExpression, "empty string"},
		{"negative index", Field(-1), CodeExpression, "negative index"},
		{"boolean segment", Field(true), CodeExpression, "boolean"},
		{"NaN", Const(math.NaN()), CodeExpression, "NaN"},
		{"finer than a microsecond", Const(time.Date(2026, 1, 2, 3, 4, 5, 1, time.UTC)), CodeExpression, "finer than a microsecond"},
		{"list", Const([]int{1}), CodeExpression, "scalar"},
		{"map", Const(map[string]int{}), CodeExpression, "scalar"},
		{"uint64 beyond int64", Const(uint64(math.MaxUint64)), CodeExpression, "64 bits"},
		{"decimal beyond 38 digits", Const(MustDecimal("1e40")), CodeExpression, "38 digits"},
		{"opaque", Const(complex(1, 1)), CodeExpression, "complex128"},
		{"bytes", Const([]byte("a")), CodeExpression, "[]uint8"},
		{"impossible date", Const(Date{2026, 2, 30}), CodeExpression, "Date"},
		{"null operand", x.Eq(nil), CodeExpression, "null constant"},
		{"condition operand", x.Gt(1).Eq(2), CodeExpression, "condition"},
		{"value in and", x.And(Field("y").Add(1)), CodeExpression, "value"},
		{"arithmetic on a string", x.Add("a"), CodeExpression, "numbers"},
		{"no values", x.IsIn(), CodeExpression, "in"},
		{"mixed values", x.IsIn(1, "a"), CodeExpression, "type groups"},
		{"null value", x.IsIn(nil), CodeExpression, "null"},
		{"expression value", x.IsIn(Field("y")), CodeExpression, "cannot be a constant"},
		{"boolean bounds", x.Between(true, false), CodeExpression, "orderable"},
		{"bad pattern", x.Matches("("), CodeExpression, "not valid"},
		{"one operand", And(x.Gt(1)), CodeExpression, "two operands"},
		{"nil operand", x.Gt((*Expr)(nil)), CodeExpression, "nil or zero *Expr"},
		{"nil Not", Not(nil), CodeExpression, "nil or zero *Expr"},
		{"zero Expr", Not(&Expr{}), CodeExpression, "nil or zero *Expr"},
		{"unknown closed", x.BetweenClosed(1, 2, "x"), CodeExpression, "closed must be"},
		{"pattern not UTF-8", x.Matches(string([]byte{0xff})), CodeFormat, "pattern is not valid UTF-8"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := tt.expr.Err()
			var e *Error
			if !errors.As(err, &e) || e.Code != tt.code {
				t.Fatalf("got %v, want %s", err, tt.code)
			}
			if !strings.Contains(e.Message, tt.text) {
				t.Errorf("message %q lacks %q", e.Message, tt.text)
			}
			if strings.HasPrefix(e.Message, "cr_") || strings.Contains(e.JSON, `"message":"cr_`) {
				t.Errorf("a C function in the message: %s", e.JSON)
			}
			if !strings.Contains(e.JSON, `"code":"`+string(tt.code)+`"`) {
				t.Errorf("JSON form %s", e.JSON)
			}
			// The error goes on through every node built on it, and stops
			// NewRule.
			built := Not(tt.expr.Eq(1).And(Field("y")))
			if built.Err() != err {
				t.Errorf("the chain lost the error: %v", built.Err())
			}
			if _, err := NewRule("r", built); err != tt.expr.Err() {
				t.Errorf("NewRule gave %v", err)
			}
			if !strings.HasPrefix(built.String(), "!(") || built.Tag() != "" || built.Kind() != "" {
				t.Errorf("an invalid expression reads as %q", built.String())
			}
		})
	}
	var nilExpr *Expr
	if codeOf(nilExpr.Err()) != CodeExpression || nilExpr.Tag() != "" {
		t.Error("a nil *Expr")
	}
}

// TestIntrospection checks Children, FieldPaths, Equal and Hash.
func TestIntrospection(t *testing.T) {
	cond := Field("a").Gt(1).And(Field("b", 0).Between(Field("lo"), 9))
	children := cond.Children()
	if len(children) != 2 || children[0].String() != "a > 1" || children[1].Tag() != "between" {
		t.Fatalf("children %v", children)
	}
	if grand := children[1].Children(); len(grand) != 3 || grand[1].String() != "lo" {
		t.Errorf("between's children %v", grand)
	}
	paths := must(cond.FieldPaths())(t)
	if !reflect.DeepEqual(paths, [][]any{{"a"}, {"b", 0}, {"lo"}}) {
		t.Errorf("paths %v", paths)
	}
	same := Field("a").Gt(1).And(Field("b", 0).Between(Field("lo"), 9))
	if !cond.Equal(same) || cond.Hash() != same.Hash() {
		t.Error("structurally identical nodes differ")
	}
	if Const(1).Equal(Const(1.0)) || !Const(0.0).Equal(Const(math.Copysign(0, -1))) {
		t.Error("constants compare by type and value")
	}
	if !decimalConst("1.0").Equal(decimalConst("1.00")) {
		t.Error("decimals 1.0 and 1.00 are the same node")
	}
	if cond.Equal(nil) || cond.Equal(Field()) {
		t.Error("an invalid expression equals nothing")
	}
	if _, err := Field().FieldPaths(); codeOf(err) != CodeExpression {
		t.Errorf("FieldPaths of an invalid expression: %v", err)
	}
}

func decimalConst(s string) *Expr { return Const(MustDecimal(s)) }
