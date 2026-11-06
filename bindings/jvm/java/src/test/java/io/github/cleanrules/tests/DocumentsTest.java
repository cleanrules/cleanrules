package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.field;
import static io.github.cleanrules.Documents.dump;
import static io.github.cleanrules.Documents.dumps;
import static io.github.cleanrules.Documents.load;
import static io.github.cleanrules.Documents.loads;
import static io.github.cleanrules.Documents.ruleFromText;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Action;
import io.github.cleanrules.DecisionTable;
import io.github.cleanrules.Decision;
import io.github.cleanrules.DocumentContent;
import io.github.cleanrules.Expr;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.Limits;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import io.github.cleanrules.RuleSyntaxException;
import io.github.cleanrules.UnsupportedException;
import java.io.IOException;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalInt;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/** Documents in every format, both ways, for each kind; actions; files; one rule from text. */
class DocumentsTest {
    private static final String CONDITION = "customer.years > 5 AND order.total > 1000";
    private static final String RULE = "RULE vip PRIORITY 10\nIF customer.years > 5\nTHEN discount = 0.15\nDO notify_sales";

    /** Each kind in each format: the core's canonical text, read back as the same thing. */
    @Test
    void eachKindGoesBothWaysInEachFormat() {
        List<DocumentContent> values = List.of(
                loads(CONDITION, Format.TEXT),
                loads(RULE, Format.TEXT),
                loads(Fixtures.DISCOUNTS, Format.TEXT),
                loads("HIT POLICY,first\nRULE,age,THEN band\nminor,< 18,'minor'\nadult,-,'adult'\n", Format.CSV));
        List<Class<?>> kinds = List.of(Expr.class, Rule.class, RuleSet.class, DecisionTable.class);
        for (int i = 0; i < values.size(); i++) {
            DocumentContent value = values.get(i);
            assertInstanceOf(kinds.get(i), value);
            List<Format> formats = value instanceof DecisionTable
                    ? List.of(Format.CSV, Format.JSON, Format.YAML)
                    : List.of(Format.JSON, Format.YAML, Format.TEXT);
            String json = dumps(value);
            assertEquals(json, dumps(value, Format.JSON));
            for (Format format : formats) {
                String text = dumps(value, format);
                DocumentContent again = loads(text, format);
                assertInstanceOf(kinds.get(i), again);
                assertEquals(text, dumps(again, format), format + " of " + value);
                assertEquals(json, dumps(again), format + " of " + value);
            }
        }
        assertTrue(dumps(values.get(0)).startsWith("{\"version\":1,\"condition\":"));
        assertEquals(CONDITION, dumps(values.get(0), Format.TEXT));
    }

    @Test
    void whatIsReadIsIdiomatic() {
        Rule rule = (Rule) loads(RULE, Format.TEXT);
        assertEquals("vip", rule.name());
        assertEquals("customer.years > 5", rule.when().toString());
        assertEquals(field("customer", "years").gt(5), rule.when());
        assertEquals(new BigDecimal("0.15"), ((io.github.cleanrules.Const) rule.then().get("discount")).value());
        assertEquals(10, rule.priority());
        assertEquals(Optional.of("notify_sales"), rule.actionName());
        assertTrue(rule.action().isEmpty());

        RuleSet rules = (RuleSet) loads(Fixtures.DISCOUNTS, Format.TEXT);
        assertEquals(HitPolicy.PRIORITY, rules.hitPolicy());
        assertEquals(List.of("blocked", "vip", "loyal"), rules.rules().stream().map(Rule::name).toList());
        assertEquals(Optional.of("blocked customers never get a discount"), rules.rule("blocked").description());
        assertEquals(List.of("loyalty"), rules.rule("vip").tags());
        Decision decision = rules.evaluate(Map.of(
                "customer", Map.of("years", 7, "blocked", false, "newsletter", false),
                "order", Map.of("total", 1500)));
        assertEquals(List.of("vip", "loyal"), decision.fired());
        // A rule read from a document is written back exactly, alone or in a new rule set.
        Rule vip = rules.rule("vip");
        assertEquals(dumps(vip, Format.TEXT), dumps(loads(dumps(vip)), Format.TEXT));
        assertTrue(dumps(RuleSet.of(vip), Format.TEXT).contains("TAGS loyalty"));
    }

    @Test
    void actionsAreBoundOnLoad() {
        List<String> called = new ArrayList<>();
        Action notify = context -> called.add(context.rule().name());
        Map<String, Action> registry = Map.of("notify_sales", notify);
        RuleSet rules = (RuleSet) loads(Fixtures.DISCOUNTS, Format.TEXT, registry);
        assertTrue(rules.rule("vip").action().isPresent());
        rules.run(Map.of("customer", Map.of("years", 9), "order", Map.of("total", 2000)));
        assertEquals(List.of("vip"), called);
        Rule rule = (Rule) loads(RULE, Format.TEXT, registry);
        assertTrue(rule.action().isPresent());
        // Without the registry they stay unbound; a registry that lacks one is the core's error.
        RuleSet unbound = (RuleSet) loads(Fixtures.DISCOUNTS, Format.TEXT);
        assertTrue(unbound.rule("vip").action().isEmpty());
        assertThrows(RegistryException.class, () -> unbound.run(Map.of()));
        RegistryException missing =
                assertThrows(RegistryException.class, () -> loads(Fixtures.DISCOUNTS, Format.TEXT, Map.of()));
        assertEquals(Optional.of("vip"), missing.rule());
        // A condition or a table has no actions to bind.
        assertInstanceOf(Expr.class, loads(CONDITION, Format.TEXT, registry));
    }

    @Test
    void codeWithoutANameIsNotWrittenUnlessOmitted() {
        Rule coded = Rule.named("coded").when(field("x").gt(1)).action(context -> {});
        UnsupportedException refused = assertThrows(UnsupportedException.class, () -> dumps(coded));
        assertEquals("unsupported", refused.code());
        assertEquals(Optional.of("/rule/do"), refused.pointer());
        assertEquals(Optional.of("coded"), refused.rule());
        assertTrue(refused.getMessage().contains("code without a name"), refused.getMessage());
        assertTrue(refused.hint().orElseThrow().contains("omitActions"));
        assertTrue(refused.json().contains("\"pointer\":\"/rule/do\""));
        String written = dumps(coded, Format.TEXT, true);
        assertEquals("RULE coded\nIF x > 1", written);
        RuleSet rules = RuleSet.of(Rule.named("a").when(field("x").gt(1)), coded);
        UnsupportedException inSet = assertThrows(UnsupportedException.class, () -> dumps(rules, Format.YAML));
        assertEquals(Optional.of("/ruleset/rules/1/do"), inSet.pointer());
        assertFalse(dumps(rules, Format.JSON, true).contains("\"do\""));
        // A named action is data: written, bound or not.
        Rule named = Rule.named("n").when(field("x").gt(1)).action("notify").action(context -> {});
        assertTrue(dumps(named).contains("\"do\":\"notify\""));
    }

    @Test
    void onlyAConditionIsADocument() {
        ExpressionException value = assertThrows(ExpressionException.class, () -> dumps(field("price").mul(2)));
        assertEquals("only a condition can be a document, not the value price * 2", value.getMessage());
        assertThrows(UnsupportedException.class, () -> dumps(field("x").gt(1), Format.CSV));
        assertThrows(UnsupportedException.class, () -> dumps(RuleSet.of(RulesTest.vip()), Format.CSV));
    }

    @Test
    void errorsOfReadingKeepTheirPlace() {
        FormatException json = assertThrows(FormatException.class, () -> loads("{\"version\": 1, \"rule\": 3}"));
        assertTrue(json.pointer().isPresent());
        RuleSyntaxException text = assertThrows(RuleSyntaxException.class, () -> loads("IF x >", Format.TEXT));
        assertEquals(OptionalInt.of(1), text.line());
        FormatException bytes = assertThrows(
                FormatException.class, () -> loads(new byte[] {'{', (byte) 0xff}, Format.JSON, null, null));
        assertTrue(bytes.getMessage().contains("UTF-8"), bytes.getMessage());
        FormatException tooBig = assertThrows(
                FormatException.class, () -> loads(RULE, Format.TEXT, Limits.defaults().withMaxSize(10), null));
        assertEquals("limit_exceeded", tooBig.code());
    }

    @Test
    void oneRuleIsReadFromText() {
        Rule vip = ruleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15");
        assertEquals("vip", vip.name());
        assertEquals("rule_1", ruleFromText("IF x > 1\nTHEN y = 2").name());
        assertThrows(RuleSyntaxException.class, () -> ruleFromText("x > 1"));
        assertThrows(RuleSyntaxException.class, () -> ruleFromText(Fixtures.DISCOUNTS));
        assertThrows(FormatException.class, () -> ruleFromText(RULE, Limits.defaults().withMaxSize(5)));
        Action notify = context -> {};
        Rule coded = ruleFromText("RULE c IF x THEN y = 1", notify);
        assertTrue(coded.action().isPresent());
        assertTrue(coded.actionName().isEmpty());
        // Text that names an action binds the code to that name.
        Rule named = ruleFromText(RULE, notify);
        assertEquals(Optional.of("notify_sales"), named.actionName());
        assertTrue(named.action().isPresent());
        assertTrue(ruleFromText(RULE, Map.of("notify_sales", notify)).action().isPresent());
        assertThrows(RegistryException.class, () -> ruleFromText(RULE, Map.of("other", notify)));
        // Limits and a registry together; either may be null.
        assertTrue(ruleFromText(RULE, null, Map.of("notify_sales", notify)).action().isPresent());
        assertTrue(ruleFromText(RULE, null, null).action().isEmpty());
        assertThrows(FormatException.class, () -> ruleFromText(RULE, Limits.defaults().withMaxSize(5), null));
    }

    @Test
    void filesTakeTheirFormatFromTheExtension(@TempDir Path directory) throws IOException {
        RuleSet rules = (RuleSet) loads(Fixtures.DISCOUNTS, Format.TEXT);
        for (String name : List.of("rules.json", "rules.yaml", "rules.YML", "rules.rules")) {
            Path file = directory.resolve(name);
            dump(rules, file);
            RuleSet again = (RuleSet) load(file);
            assertEquals(dumps(rules), dumps(again), name);
        }
        assertTrue(Files.readString(directory.resolve("rules.rules")).endsWith("\n"));
        assertTrue(Files.readString(directory.resolve("rules.yaml")).startsWith("version: 1"));
        Path table = directory.resolve("table.csv");
        Files.writeString(table, "RULE,age,THEN band\r\nminor,< 18,'minor'\r\n", StandardCharsets.UTF_8);
        DecisionTable read = (DecisionTable) load(table);
        assertEquals(1, read.rows().size());
        dump(read, directory.resolve("copy.csv"));
        assertEquals(dumps(read, Format.CSV), Files.readString(directory.resolve("copy.csv")));
        // Actions on load, and a format given apart from the extension.
        Path odd = directory.resolve("rules.txt");
        dump(rules, odd, Format.TEXT, false);
        assertThrows(IllegalArgumentException.class, () -> load(odd));
        RuleSet bound = (RuleSet) load(odd, Format.TEXT, null, Map.of("notify_sales", context -> {}));
        assertTrue(bound.rule("vip").action().isPresent());
        assertTrue(((RuleSet) load(directory.resolve("rules.json"), Map.of("notify_sales", context -> {})))
                .rule("vip").action().isPresent());
        // A file beyond the size limit is not read.
        FormatException big = assertThrows(
                FormatException.class,
                () -> load(directory.resolve("rules.json"), null, Limits.defaults().withMaxSize(20), null));
        assertEquals("limit_exceeded", big.code());
        assertTrue(big.getMessage().startsWith("rules.json has "), big.getMessage());
        assertTrue(big.hint().orElseThrow().contains("Limits.withMaxSize "), big.hint().toString());
        FormatException bigYaml = assertThrows(
                FormatException.class,
                () -> load(directory.resolve("rules.yaml"), null, Limits.defaults().withMaxYamlSize(20), null));
        assertTrue(bigYaml.hint().orElseThrow().contains("Limits.withMaxYamlSize"), bigYaml.hint().toString());
        FormatException bigText = assertThrows(
                FormatException.class,
                () -> load(directory.resolve("rules.rules"), null, Limits.defaults().withMaxTextSize(20), null));
        assertTrue(bigText.hint().orElseThrow().contains("Limits.withMaxTextSize"), bigText.hint().toString());
        assertThrows(java.io.UncheckedIOException.class, () -> load(directory.resolve("missing.json")));
    }
}
