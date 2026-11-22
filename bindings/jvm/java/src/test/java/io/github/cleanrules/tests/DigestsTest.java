package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.constant;
import static io.github.cleanrules.Conditions.field;
import static io.github.cleanrules.Documents.canonicalBytes;
import static io.github.cleanrules.Documents.digest;
import static io.github.cleanrules.Documents.dumps;
import static io.github.cleanrules.Documents.loads;
import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.DecisionTable;
import io.github.cleanrules.DocumentContent;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.Format;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.ffi.Document;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Function;
import org.junit.jupiter.api.Test;

/** Canonical bytes and digests (spec/json-format.md, Canonical bytes, Digests, Rule set digests). */
class DigestsTest {
    private static final String TEXT = """
            HIT POLICY priority

            RULE vip PRIORITY 10
            IF customer.years > 5
            THEN discount = 0.15
            """;

    /** The same rule set as JSON with other whitespace, member order, escapes and number layout. */
    private static final String JSON = """
            {
              "ruleset" : {
                "rules" : [ {
                  "then" : { "discount" : { "value" : { "$decimal" : "1.5e-1" }, "op" : "const" } },
                  "when" : {
                    "right" : { "value" : 5, "op" : "const" },
                    "op" : "gt",
                    "left" : { "path" : [ "customer", "years" ], "op" : "field" }
                  },
                  "priority" : 10,
                  "name" : "\\u0076ip"
                } ],
                "logic" : "boolean",
                "hit_policy" : "priority"
              },
              "version" : 1
            }
            """;

    private static RuleSet built() {
        return RuleSet.builder()
                .hitPolicy(HitPolicy.PRIORITY)
                .add(Rule.named("vip")
                        .when(field("customer", "years").gt(5))
                        .then("discount", new BigDecimal("0.15"))
                        .priority(10))
                .build();
    }

    private static String sha256(byte[] bytes) throws Exception {
        return "sha256:" + HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
    }

    @Test
    void theSameRuleSetHasTheSameBytesAndDigestHoweverItIsWritten() throws Exception {
        RuleSet fromText = (RuleSet) loads(TEXT, Format.TEXT);
        byte[] bytes = canonicalBytes(fromText);
        String digest = digest(fromText);
        String canonical = new String(bytes, StandardCharsets.UTF_8);
        assertEquals(
                "{\"ruleset\":{\"hit_policy\":\"priority\",\"logic\":\"boolean\",\"rules\":[{\"name\":\"vip\","
                        + "\"priority\":10,\"then\":{\"discount\":{\"op\":\"const\",\"value\":{\"$decimal\":\"0.15\"}}},"
                        + "\"when\":{\"left\":{\"op\":\"field\",\"path\":[\"customer\",\"years\"]},\"op\":\"gt\","
                        + "\"right\":{\"op\":\"const\",\"value\":5}}}]},\"version\":1}",
                canonical);
        assertTrue(digest.matches("sha256:[0-9a-f]{64}"), digest);
        assertEquals(sha256(bytes), digest);
        List<DocumentContent> same = List.of(
                loads(JSON),
                loads(dumps(fromText, Format.YAML), Format.YAML),
                loads(dumps(fromText)),
                built(),
                fromText.bind(Map.of()));
        for (DocumentContent other : same) {
            assertArrayEquals(bytes, canonicalBytes(other), other.toString());
            assertEquals(digest, digest(other), other.toString());
        }
        // The native layer gives the same.
        try (Document document = Document.read(JSON, Format.JSON);
                io.github.cleanrules.ffi.RuleSet rules = io.github.cleanrules.ffi.RuleSet.read(TEXT, Format.TEXT)) {
            assertArrayEquals(bytes, document.canonicalBytes());
            assertEquals(digest, document.digest());
            assertArrayEquals(bytes, rules.canonicalBytes());
            assertEquals(digest, rules.digest());
        }
    }

    @Test
    void aDigestIdentifiesAnArtifactNotItsMeaning() {
        // x > 5 and 5 < x decide alike, but are other artifacts.
        assertNotEquals(digest(field("x").gt(5)), digest(constant(5).lt(field("x"))));
        assertEquals(digest(field("x").gt(5)), digest(loads("x > 5", Format.TEXT)));
        // The decimals 1.0 and 1.00 are equal, but of another scale.
        Rule one = Rule.named("r").when(constant(true)).then("price", new BigDecimal("1.0"));
        Rule two = Rule.named("r").when(constant(true)).then("price", new BigDecimal("1.00"));
        assertNotEquals(digest(RuleSet.of(one)), digest(RuleSet.of(two)));
        assertTrue(new String(canonicalBytes(one), StandardCharsets.UTF_8).contains("{\"$decimal\":\"1.0\"}"));
        assertTrue(new String(canonicalBytes(two), StandardCharsets.UTF_8).contains("{\"$decimal\":\"1.00\"}"));
        // A description is part of the artifact.
        assertNotEquals(digest(one), digest(one.description("the price")));
    }

    @Test
    void aRuleAndARuleSetOfItAreOtherDocuments() {
        Rule vip = built().rules().getFirst();
        String rule = new String(canonicalBytes(vip), StandardCharsets.UTF_8);
        assertTrue(rule.startsWith("{\"rule\":{\"name\":\"vip\""), rule);
        assertTrue(rule.endsWith(",\"version\":1}"), rule);
        assertNotEquals(digest(vip), digest(RuleSet.of(vip)));
        // A rule set read from a rule document is a rule set of that rule with the defaults.
        try (io.github.cleanrules.ffi.RuleSet read = io.github.cleanrules.ffi.RuleSet.read(dumps(vip), Format.JSON)) {
            assertEquals(digest(RuleSet.of(vip)), read.digest());
        }
    }

    @Test
    void codeWithoutANameIsRefusedUnlessOmitted() {
        Rule plain = Rule.named("coded").when(field("x").gt(1));
        Rule coded = plain.action(context -> {});
        List<Function<DocumentContent, Object>> calls = List.of(value -> canonicalBytes(value), value -> digest(value));
        for (Function<DocumentContent, Object> call : calls) {
            UnsupportedException refused = assertThrows(UnsupportedException.class, () -> call.apply(coded));
            assertEquals(Optional.of("/rule/do"), refused.pointer());
            assertEquals(Optional.of("coded"), refused.rule());
            RuleSet rules = RuleSet.of(Rule.named("a").when(field("x").gt(1)), coded);
            UnsupportedException inSet = assertThrows(UnsupportedException.class, () -> call.apply(rules));
            assertEquals(Optional.of("/ruleset/rules/1/do"), inSet.pointer());
        }
        // Omitted, the rule is written without it.
        assertEquals(digest(plain), digest(coded, true));
        assertArrayEquals(canonicalBytes(plain), canonicalBytes(coded, true));
        assertEquals(digest(RuleSet.of(plain)), digest(RuleSet.of(coded), true));
        // A named action is data, part of the artifact, bound or not.
        Rule named = plain.action("notify");
        assertNotEquals(digest(plain), digest(named));
        assertEquals(digest(named), digest(named.action(context -> {})));
        assertFalse(new String(canonicalBytes(coded, true), StandardCharsets.UTF_8).contains("\"do\""));
    }

    @Test
    void onlyAConditionIsADocument() {
        ExpressionException value = assertThrows(ExpressionException.class, () -> digest(field("price").mul(2)));
        assertEquals("only a condition can be a document, not the value price * 2", value.getMessage());
        assertThrows(ExpressionException.class, () -> canonicalBytes(field("price").mul(2)));
        assertThrows(NullPointerException.class, () -> digest(null));
    }

    @Test
    void aTableHasItsOwnDigestAndItsRuleSetAnother() throws Exception {
        String csv = "RULE,customer.years,THEN discount\nvip,> 5,0.15\nloyal,> 2,0.05\n";
        DecisionTable table = (DecisionTable) loads(csv, Format.CSV);
        String tableDigest = digest(table);
        String rulesetDigest = digest(table.ruleSet());
        assertNotEquals(tableDigest, rulesetDigest);
        assertTrue(new String(canonicalBytes(table), StandardCharsets.UTF_8).startsWith("{\"decision_table\":"));
        assertEquals(sha256(canonicalBytes(table)), tableDigest);
        // The same table, read again from its JSON document.
        assertEquals(tableDigest, digest(loads(dumps(table))));
        try (Document document = Document.read(csv, Format.CSV);
                io.github.cleanrules.ffi.RuleSet compiled = io.github.cleanrules.ffi.RuleSet.fromDocument(document);
                io.github.cleanrules.ffi.RuleSet read = io.github.cleanrules.ffi.RuleSet.read(csv, Format.CSV)) {
            assertEquals(tableDigest, document.digest());
            assertEquals(rulesetDigest, compiled.digest());
            assertEquals(rulesetDigest, read.digest());
        }
        // The compiled rule set, written as a rule set document, keeps that digest.
        assertEquals(rulesetDigest, digest(loads(dumps(table.ruleSet()))));
    }

    /**
     * A bundle (spec/bundles.md) has no idiomatic Java type yet: {@code loads} refuses it, and the native layer
     * reads, writes and hashes it.
     */
    @Test
    void bundlesAreReadThroughTheNativeLayerOnly() {
        RuleSet rules = RuleSet.of();
        String limits = "\"limits\": {\"max_size\": 1, \"max_nesting\": 1, \"max_depth\": 1,"
                + " \"max_path_segments\": 1, \"max_table_size\": 1, \"max_values\": 1}";
        String bundle = "{\"version\": 1, \"bundle\": {\"id\": \"a\", \"revision\": \"1\","
                + " \"ruleset\": {\"rules\": []}, \"ruleset_digest\": \"" + digest(rules) + "\","
                + " \"engine\": {\"name\": \"cleanrules\", \"version\": \"1.0.0-other\", " + limits + "},"
                + " \"contract\": {\"input\": {}, \"output\": {\"keys\": []}}}}";
        assertThrows(UnsupportedException.class, () -> loads(bundle));
        try (Document document = Document.read(bundle, Format.JSON)) {
            assertEquals(io.github.cleanrules.DocumentKind.BUNDLE, document.kind());
            assertTrue(document.digest().startsWith("sha256:"));
        }
    }

    /**
     * A case file (spec/tests.md) has no idiomatic Java type yet: {@code loads} refuses it, after the core's
     * errors in it, and the native layer reads, writes and hashes it.
     */
    @Test
    void caseFilesAreReadThroughTheNativeLayerOnly() {
        String cases = "{\"version\":1,\"cases\":{\"items\":[{\"id\":\"a\",\"input\":{\"x\":1}}]}}";
        UnsupportedException refused = assertThrows(UnsupportedException.class, () -> loads(cases));
        assertEquals("loads", refused.operation().orElseThrow());
        FormatException repeated = assertThrows(FormatException.class,
                () -> loads("{\"version\": 1, \"cases\": {\"items\": [{\"id\": \"a\", \"input\": {}},"
                        + " {\"id\": \"a\", \"input\": {}}]}}"));
        assertEquals("/cases/items/1/id", repeated.pointer().orElseThrow());
        try (Document document = Document.read(cases, Format.JSON)) {
            assertEquals(io.github.cleanrules.DocumentKind.CASES, document.kind());
            assertEquals("cases", document.kind().jsonName());
            assertEquals(cases, document.write(Format.JSON));
            assertTrue(document.digest().startsWith("sha256:"));
        }
    }
}
