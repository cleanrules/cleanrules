package cleanrules

import (
	"bytes"
	"crypto/sha256"
	"errors"
	"fmt"
	"regexp"
	"strings"
	"testing"
)

var digestForm = regexp.MustCompile(`^sha256:[0-9a-f]{64}$`)

// TestCanonicalBytes checks each kind of document's canonical bytes: sorted
// members, no whitespace, the same bytes when read back from any format,
// and a digest that is the SHA-256 hash of the bytes.
func TestCanonicalBytes(t *testing.T) {
	formats := map[string][]Format{
		"condition": {FormatJSON, FormatYAML, FormatText},
		"rule":      {FormatJSON, FormatYAML, FormatText},
		"ruleset":   {FormatJSON, FormatYAML, FormatText},
		"table":     {FormatJSON, FormatYAML, FormatCSV},
	}
	for kind, doc := range documents(t) {
		t.Run(kind, func(t *testing.T) {
			canonical := must(CanonicalBytes(doc))(t)
			if bytes.ContainsAny(canonical, "\n\r\t") || !bytes.HasSuffix(canonical, []byte(`,"version":1}`)) {
				t.Errorf("not the canonical layout: %s", canonical)
			}
			digest := must(Digest(doc))(t)
			if !digestForm.MatchString(digest) {
				t.Errorf("digest %q", digest)
			}
			if want := fmt.Sprintf("sha256:%x", sha256.Sum256(canonical)); digest != want {
				t.Errorf("digest %s, the hash of the bytes %s", digest, want)
			}
			for _, format := range formats[kind] {
				back := must(Loads(must(Dumps(doc, WithFormat(format)))(t), WithFormat(format)))(t)
				if again := must(CanonicalBytes(back))(t); !bytes.Equal(again, canonical) {
					t.Errorf("%s: read back, %s", format, again)
				}
				if again := must(Digest(back))(t); again != digest {
					t.Errorf("%s: read back, the digest %s", format, again)
				}
			}
		})
	}
}

// TestDigestOfSpecExample checks the example of spec/json-format.md
// (Canonical bytes, Digests), built in code.
func TestDigestOfSpecExample(t *testing.T) {
	vip := rule(t, "vip", Field("years").Gt(5), Then("discount", MustDecimal("0.15")))
	rs := ruleSet(t, nil, vip)
	want := `{"ruleset":{"hit_policy":"first","logic":"boolean","rules":[{"name":"vip","then":{"discount":{"op":"const","value":{"$decimal":"0.15"}}},"when":{"left":{"op":"field","path":["years"]},"op":"gt","right":{"op":"const","value":5}}}]},"version":1}`
	if got := string(must(CanonicalBytes(rs))(t)); got != want {
		t.Errorf("canonical bytes\n%s\nwant\n%s", got, want)
	}
	if got := must(Digest(rs))(t); got != "sha256:5a0e590cb8f3bd57392ede84c0d0c4596cf9374fefcbcc6b28ed61421878f312" {
		t.Errorf("digest %s", got)
	}
	// The rule set digest of a rule is that of a rule set of it alone.
	if got := must(Digest(ruleSet(t, nil, vip)))(t); got != must(Digest(rs))(t) {
		t.Errorf("the rule set of the rule: %s", got)
	}
	if must(Digest(vip))(t) == must(Digest(rs))(t) {
		t.Error("a rule's own digest is that of its rule document, not of a rule set")
	}
}

// TestDigestsAreNotEquivalence checks that a digest tells one artifact,
// not a meaning.
func TestDigestsAreNotEquivalence(t *testing.T) {
	pairs := map[string][2]Document{
		"flipped":     {Field("x").Gt(5), Const(5).Lt(Field("x"))},
		"scale":       {Field("x").Eq(MustDecimal("1.0")), Field("x").Eq(MustDecimal("1.00"))},
		"renamed":     {rule(t, "a", Field("x").Gt(5)), rule(t, "b", Field("x").Gt(5))},
		"description": {rule(t, "a", Field("x").Gt(5)), rule(t, "a", Field("x").Gt(5), WithDescription("d"))},
	}
	for name, pair := range pairs {
		if must(Digest(pair[0]))(t) == must(Digest(pair[1]))(t) {
			t.Errorf("%s: the same digest", name)
		}
	}
	// The same rule set, built in code and read from JSON with another
	// layout and member order and its defaults left out, is one artifact.
	built := ruleSet(t, nil, rule(t, "r", Field("x").Gt(5)))
	read := must(Loads(`{"ruleset": {"rules": [{"when": {"right": {"value": 5, "op": "const"}, "op": "gt",
		"left": {"path": ["x"], "op": "field"}}, "name": "r"}]}, "version": 1}`))(t)
	if must(Digest(built))(t) != must(Digest(read))(t) {
		t.Error("the same rule set has two digests")
	}
}

// TestTableDigests checks a decision table's two digests.
func TestTableDigests(t *testing.T) {
	table := documents(t)["table"].(*DecisionTable)
	own := must(Digest(table))(t)
	compiled := must(Digest(must(table.RuleSet())(t)))(t)
	if own == compiled || !digestForm.MatchString(compiled) {
		t.Errorf("the table's digest %s, its rule set's %s", own, compiled)
	}
	if !strings.HasPrefix(string(must(CanonicalBytes(table))(t)), `{"decision_table":`) {
		t.Error("the table's canonical bytes are not its own document")
	}
	// Another table that compiles to the same rule set has the same rule set
	// digest, and its own digest.
	other := must(Loads(must(Dumps(table, WithFormat(FormatCSV)))(t), WithFormat(FormatCSV)))(t).(*DecisionTable)
	if must(Digest(must(other.RuleSet())(t)))(t) != compiled {
		t.Error("the same compiled rule set has two digests")
	}
}

// TestCanonicalErrors checks what has no canonical bytes: the errors of
// Dumps.
func TestCanonicalErrors(t *testing.T) {
	closed := ruleSet(t, nil, rule(t, "r", Field("x").Gt(1)))
	closed.Close()
	closedTable := documents(t)["table"].(*DecisionTable)
	closedTable.Close()
	cases := map[string]struct {
		doc  Document
		want error
	}{
		"value node":   {Field("x").Add(1), ErrExpression},
		"broken node":  {Field("x").Add("a"), ErrExpression},
		"nil":          {nil, ErrUnsupported},
		"nil expr":     {(*Expr)(nil), ErrExpression},
		"nil rule":     {(*Rule)(nil), ErrRuleDefinition},
		"nil rule set": {(*RuleSet)(nil), ErrClosed},
		"nil table":    {(*DecisionTable)(nil), ErrClosed},
		"closed":       {closed, ErrClosed},
		"closed table": {closedTable, ErrClosed},
	}
	for name, c := range cases {
		if b, err := CanonicalBytes(c.doc); !errors.Is(err, c.want) || b != nil {
			t.Errorf("CanonicalBytes: %s: %v, want %v", name, err, c.want)
		}
		if d, err := Digest(c.doc); !errors.Is(err, c.want) || d != "" {
			t.Errorf("Digest: %s: %v, want %v", name, err, c.want)
		}
	}
	var e *Error
	if _, err := Digest(Field("x").Add(1)); !errors.As(err, &e) || e.Code != CodeExpression {
		t.Errorf("a value node: %v", err)
	}
}

// TestReadDecisionWithDigest checks that a decision read with its rule set
// digest keeps it, and that one of other rules is a decision_mismatch.
func TestReadDecisionWithDigest(t *testing.T) {
	rs := documents(t)["ruleset"].(*RuleSet)
	digest := must(Digest(rs))(t)
	d := must(rs.Evaluate(map[string]any{"customer": map[string]any{"years": 7}, "country": "ES"}))(t)
	text := must(d.JSON())(t)
	if strings.Contains(text, "ruleset_digest") {
		t.Fatalf("a decision written without being asked carries a digest: %s", text)
	}
	stamped := strings.Replace(text, `"decision":{`, `"decision":{"ruleset_digest":"`+digest+`",`, 1)
	back := must(rs.ReadDecision(stamped))(t)
	if again := must(back.JSON())(t); !strings.Contains(again, `"ruleset_digest":"`+digest+`"`) {
		t.Errorf("written back without its digest: %s", again)
	}
	other := must(Digest(ruleSet(t, nil, rule(t, "r", Field("x").Gt(1)))))(t)
	wrong := strings.Replace(text, `"decision":{`, `"decision":{"ruleset_digest":"`+other+`",`, 1)
	var e *Error
	if _, err := rs.ReadDecision(wrong); !errors.Is(err, ErrDecisionMismatch) || !errors.As(err, &e) || e.Pointer == nil || *e.Pointer != "/decision/ruleset_digest" {
		t.Errorf("another rule set's digest: %v", err)
	}
	upper := strings.Replace(text, `"decision":{`, `"decision":{"ruleset_digest":"`+strings.ToUpper(digest)+`",`, 1)
	if _, err := rs.ReadDecision(upper); !errors.Is(err, ErrFormat) || errors.Is(err, ErrDecisionMismatch) {
		t.Errorf("an uppercase digest: %v", err)
	}
}
