package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.constant;
import static io.github.cleanrules.Conditions.field;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Aggregate;
import io.github.cleanrules.Cause;
import io.github.cleanrules.CauseGroup;
import io.github.cleanrules.Counterfactual;
import io.github.cleanrules.Decision;
import io.github.cleanrules.Explanation;
import io.github.cleanrules.FieldUsage;
import io.github.cleanrules.GroupKind;
import io.github.cleanrules.NoteKind;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.HitPolicyException;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.Note;
import io.github.cleanrules.Requirement;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleExplanation;
import io.github.cleanrules.RuleNotFoundException;
import io.github.cleanrules.RuleSet;
import io.github.cleanrules.RuleSummary;
import io.github.cleanrules.Status;
import io.github.cleanrules.Summary;
import io.github.cleanrules.Truth;
import java.math.BigDecimal;
import java.time.Duration;
import java.time.Period;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalLong;
import java.util.SequencedMap;
import org.junit.jupiter.api.Test;

/** Decisions, explanations and summaries, compared with the core's own documents. */
class DecisionsTest {
    static RuleSet discounts() {
        Rule blocked = Rule.named("blocked")
                .when(field("customer", "blocked"))
                .then("discount", 0)
                .priority(99)
                .description("blocked customers never get a discount");
        Rule vip = Rule.named("vip")
                .when(field("customer", "years").gt(5).and(field("order", "total").gt(1000)))
                .then("discount", new BigDecimal("0.15"))
                .priority(10)
                .tags("loyalty")
                .action("notify_sales");
        Rule loyal = Rule.named("loyal")
                .when(field("customer", "years").gt(2).or(field("customer", "newsletter")))
                .then("discount", new BigDecimal("0.05"))
                .priority(5)
                .tags("loyalty");
        return RuleSet.builder().add(blocked, vip, loyal).hitPolicy(HitPolicy.PRIORITY).build();
    }

    static Map<String, Object> order(int years, String total) {
        return Map.of(
                "customer", Map.of("years", years, "newsletter", false),
                "order", Map.of("total", new BigDecimal(total)));
    }

    @Test
    void decisionsSayWhatFiredAndWasSelected() {
        RuleSet rules = discounts();
        Decision decision = rules.evaluate(order(7, "1500.00"));
        assertSame(rules, decision.ruleSet());
        assertEquals(HitPolicy.PRIORITY, decision.hitPolicy());
        assertEquals(List.of("vip", "loyal"), decision.fired());
        assertEquals(1, decision.selected().size());
        assertSame(rules.rule("vip"), decision.selected().getFirst().rule());
        assertEquals(Map.of("discount", new BigDecimal("0.15")), decision.output().orElseThrow());
        assertEquals(List.of(Map.of("discount", new BigDecimal("0.15"))), decision.outputs());
        assertEquals(Optional.empty(), decision.aggregated());
        assertEquals(3, decision.results().size());
        assertEquals(Truth.UNKNOWN, decision.results().get(0).truth());
        assertFalse(decision.results().get(0).fired());
        assertEquals(Truth.TRUE, decision.results().get(2).truth());
        assertTrue(decision.results().get(2).output().isEmpty());
        assertEquals("Decision(priority: selected [vip])", decision.toString());
        assertEquals(Optional.empty(), rules.evaluate(order(0, "1")).output());

        // Read back from its document.
        Decision again = rules.readDecision(decision.toJson());
        assertEquals(decision.fired(), again.fired());
        assertEquals(decision.toJson(), again.toJson());
        assertTrue(decision.write(Format.YAML).contains("hit_policy:"), decision.write(Format.YAML));
        assertThrows(FormatException.class, () -> RuleSet.of(rules.rule("loyal")).readDecision(decision.toJson()));

        // A batch.
        List<Decision> many = rules.evaluateMany(List.of(order(7, "1500"), order(3, "10"), order(0, "0")));
        assertEquals(List.of(List.of("vip"), List.of("loyal"), List.of()),
                many.stream().map(d -> d.selected().stream().map(r -> r.rule().name()).toList()).toList());
    }

    @Test
    void closingADecisionIsOptional() {
        Decision decision = discounts().evaluate(order(7, "1500.00"));
        String text = decision.explain().toString();
        decision.close();
        decision.close();
        // What was read stays; what needs the native decision is an error.
        assertEquals(List.of("vip", "loyal"), decision.fired());
        assertEquals(text, decision.explain().toString());
        assertThrows(IllegalStateException.class, decision::output);
        assertThrows(IllegalStateException.class, decision::toJson);
        try (Decision closing = discounts().evaluate(order(1, "1"))) {
            assertEquals(List.of(), closing.fired());
        }
    }

    @Test
    void multiHitPoliciesAndAggregates() {
        Rule a = Rule.named("a").when(constant(true)).then("n", 1);
        Rule b = Rule.named("b").when(constant(true)).then("n", 2);
        RuleSet all = RuleSet.builder().add(a, b).hitPolicy(HitPolicy.RULE_ORDER).build();
        Decision decision = all.evaluate(Map.of());
        assertEquals(List.of(Map.of("n", 1L), Map.of("n", 2L)), decision.outputs());
        HitPolicyException several = assertThrows(HitPolicyException.class, decision::output);
        assertEquals("hit_policy_error", several.code());
        RuleSet sum = RuleSet.builder().add(a, b).hitPolicy(HitPolicy.COLLECT).aggregate(Aggregate.SUM).build();
        assertEquals(Map.of("n", 3L), sum.evaluate(Map.of()).aggregated().orElseThrow());
        RuleSet strict = RuleSet.builder().add(Rule.named("adult").when(field("age").ge(18))).strict(true).build();
        MissingFieldException missing = assertThrows(MissingFieldException.class, () -> strict.evaluate(Map.of()));
        assertEquals(Optional.of("adult"), missing.rule());
    }

    @Test
    void everyValueKindComesBack() {
        Object opaque = new Object();
        ZonedDateTime madrid = ZonedDateTime.of(2026, 11, 5, 9, 30, 0, 0, ZoneId.of("Europe/Madrid"));
        Map<String, Object> data = new java.util.LinkedHashMap<>();
        data.put("bool", true);
        data.put("int", 7);
        data.put("float", 1.5);
        data.put("decimal", new BigDecimal("0.10"));
        data.put("string", "text");
        data.put("date", LocalDate.of(2026, 11, 5));
        data.put("naive", LocalDateTime.of(2026, 11, 5, 9, 30));
        data.put("aware", madrid);
        data.put("offset", ZonedDateTime.of(2026, 11, 5, 9, 30, 0, 0, ZoneOffset.ofHours(2)));
        data.put("list", List.of(1, "a"));
        data.put("map", Map.of("k", 1));
        data.put("opaque", opaque);
        data.put("missing", null);
        Rule echo = Rule.named("echo").when(constant(true));
        for (String key : data.keySet()) {
            echo = echo.then(key, field(key));
        }
        Map<String, Object> output = RuleSet.of(echo).evaluate(data).output().orElseThrow();
        assertEquals(true, output.get("bool"));
        assertEquals(7L, output.get("int"));
        assertEquals(1.5, output.get("float"));
        assertEquals(new BigDecimal("0.10"), output.get("decimal"));
        assertEquals("text", output.get("string"));
        assertEquals(LocalDate.of(2026, 11, 5), output.get("date"));
        assertEquals(LocalDateTime.of(2026, 11, 5, 9, 30), output.get("naive"));
        assertEquals(madrid, output.get("aware"));
        assertEquals(ZoneOffset.ofHours(2), ((ZonedDateTime) output.get("offset")).getZone());
        assertEquals(List.of(1L, "a"), output.get("list"));
        assertEquals(Map.of("k", 1L), output.get("map"));
        assertSame(opaque, output.get("opaque"));
        assertEquals(null, output.get("missing"));
        assertTrue(output.containsKey("missing"));
        assertInstanceOf(SequencedMap.class, output);
        assertThrows(UnsupportedOperationException.class, () -> output.put("x", 1));
    }

    @Test
    void explanationsHaveTheStructureOfTheCoreDocument() {
        RuleSet rules = discounts();
        for (Decision decision : List.of(
                rules.evaluate(order(7, "1500.00")),
                rules.evaluate(order(7, "900.00")),
                rules.evaluate(Map.of("customer", Map.of("years", "seven"))))) {
            Explanation explanation = decision.explain();
            assertSame(explanation, decision.explain());
            Map<?, ?> document = (Map<?, ?>) ((Map<?, ?>) TestJson.parse(explanation.toJson())).get("explanation");
            List<?> rulesJson = (List<?>) document.get("rules");
            assertEquals(rulesJson.size(), explanation.rules().size());
            List<String> selected = new ArrayList<>();
            for (Object name : (List<?>) document.get("selected")) {
                selected.add((String) name);
            }
            assertEquals(selected, explanation.selected());
            for (int i = 0; i < rulesJson.size(); i++) {
                Map<?, ?> json = (Map<?, ?>) rulesJson.get(i);
                RuleExplanation rule = explanation.rules().get(i);
                assertEquals(json.get("rule"), rule.rule());
                assertEquals(json.get("status"), rule.status().jsonName());
                assertEquals(Optional.ofNullable(json.get("achievable")), rule.achievable());
                assertEquals(json.containsKey("note"), rule.note().isPresent());
                sameRequirement(json.get("requirement"), rule.requirement());
                assertTrue(explanation.toString().contains(rule.toString()));
            }
        }
    }

    /** A requirement as the explanation document writes it, and as the API reads it. */
    private static void sameRequirement(Object json, Requirement requirement) {
        Map<?, ?> map = (Map<?, ?>) json;
        if (requirement instanceof CauseGroup group) {
            List<?> items = (List<?>) map.get(group.kind().jsonName());
            assertEquals(items.size(), group.items().size());
            for (int i = 0; i < items.size(); i++) {
                sameRequirement(items.get(i), group.items().get(i));
            }
            return;
        }
        Cause cause = (Cause) requirement;
        assertTrue(TestJson.same(map.get("condition"), TestJson.parse(cause.node().toJson())), cause.node().toJson());
        assertEquals(map.get("truth"), cause.truth() == Truth.UNKNOWN ? null : cause.truth() == Truth.TRUE);
        assertEquals(map.get("negated"), cause.negated());
        assertEquals(((List<?>) map.get("values")).size(), cause.values().size());
        for (int i = 0; i < cause.values().size(); i++) {
            Map<?, ?> read = (Map<?, ?>) ((List<?>) map.get("values")).get(i);
            assertEquals(read.get("path"), cause.values().get(i).path());
        }
        assertEquals(map.containsKey("reason"), cause.reason().isPresent());
        cause.reason().ifPresent(reason -> assertEquals(((Map<?, ?>) map.get("reason")).get("code"), reason.code()));
        assertEquals(map.containsKey("counterfactual"), cause.counterfactual().isPresent());
        cause.counterfactual().ifPresent(counterfactual -> {
            Map<?, ?> wanted = (Map<?, ?>) map.get("counterfactual");
            assertEquals(wanted.get("op"), counterfactual.op());
            assertEquals(wanted.get("path"), counterfactual.path());
        });
        boolean holds = cause.truth() == (cause.negated() ? Truth.FALSE : Truth.TRUE);
        assertEquals(holds, cause.holds());
    }

    @Test
    void explanationsInDetail() {
        RuleSet rules = discounts();
        Decision decision = rules.evaluate(order(7, "900.00"));
        assertEquals(decision.ruleSet().rules().get(1).name(), decision.whyNot("vip").rule());
        RuleExplanation vip = decision.whyNot("vip");
        assertEquals(Status.NOT_FIRED, vip.status());
        assertEquals(Optional.of(true), vip.achievable());
        Cause total = vip.causes().getFirst();
        assertEquals("order.total > 1000", total.node().toString());
        assertEquals(Truth.FALSE, total.truth());
        assertFalse(total.holds());
        assertEquals(List.of(new Cause.FieldValue(List.of("order", "total"), new BigDecimal("900.00"))),
                total.values());
        Counterfactual needed = total.counterfactual().orElseThrow();
        assertEquals(List.of("order", "total"), needed.path());
        assertEquals("gt", needed.op());
        assertEquals(1000L, needed.target());
        assertEquals(Optional.of(new BigDecimal("100.00")), needed.gap());
        assertEquals("order.total > 1000 is false (order.total = 900.00); short by 100.00", total.toString());
        RuleExplanation loyal = decision.whyNot("loyal");
        assertEquals(Status.SELECTED, loyal.status());
        assertEquals(Optional.empty(), loyal.note());
        RuleExplanation blocked = decision.whyNot("blocked");
        assertEquals(Status.UNKNOWN, blocked.status());
        assertEquals("missing_field", blocked.causes().getFirst().reason().orElseThrow().code());
        assertEquals(List.of(List.of("customer", "blocked")), blocked.causes().getFirst().reason().orElseThrow().paths());
        RuleNotFoundException missing = assertThrows(RuleNotFoundException.class, () -> decision.whyNot("nope"));
        assertEquals(List.of("blocked", "loyal", "vip"), missing.available());
        assertEquals(decision.explain().toString(), decisionText(rules, decision));

        // A note, and gaps between dates.
        Rule early = Rule.named("early").when(field("d").lt(LocalDate.of(2026, 1, 1))).priority(1);
        Rule any = Rule.named("any").when(constant(true)).priority(2);
        Rule low = Rule.named("low").when(constant(true));
        Decision dated = RuleSet.builder().add(early, any, low).hitPolicy(HitPolicy.PRIORITY).build()
                .evaluate(Map.of("d", LocalDate.of(2026, 1, 3)));
        assertEquals(Optional.of(Period.ofDays(2)),
                dated.whyNot("early").causes().getFirst().counterfactual().orElseThrow().gap());
        Note note = dated.whyNot("low").note().orElseThrow();
        assertEquals(NoteKind.LOWER_PRIORITY, note.kind());
        assertEquals("any", note.winner());
        assertEquals(OptionalLong.of(0), note.priority());
        assertEquals(OptionalLong.of(2), note.winnerPriority());
        assertEquals(note, dated.whyNot("low").note().orElseThrow());
        // Gaps between datetimes are durations, to the microsecond.
        Decision timed = RuleSet.of(Rule.named("t").when(field("at").lt(LocalDateTime.of(2026, 1, 1, 0, 0))))
                .evaluate(Map.of("at", LocalDateTime.of(2026, 1, 1, 0, 0, 1, 500_000_000)));
        assertEquals(Optional.of(Duration.ofMillis(1500)),
                timed.whyNot("t").causes().getFirst().counterfactual().orElseThrow().gap());
        assertEquals(Status.FIRED, dated.whyNot("low").status());
        Requirement group = rules.evaluate(order(1, "1.00")).whyNot("vip").requirement();
        assertEquals(GroupKind.ALL, ((CauseGroup) group).kind());
    }

    /** The explanation's text, as the native layer gives it for the same input. */
    private static String decisionText(RuleSet rules, Decision decision) {
        return rules.readDecision(decision.toJson()).explain().toString();
    }

    @Test
    void summariesHaveTheStructureOfTheCoreDocument() {
        RuleSet rules = discounts();
        Summary summary = rules.summary();
        assertEquals(HitPolicy.PRIORITY, summary.hitPolicy());
        assertEquals(Optional.empty(), summary.aggregate());
        assertEquals("boolean", summary.logic());
        assertFalse(summary.isStrict());
        assertEquals(List.of("blocked", "vip", "loyal"), summary.rules().stream().map(RuleSummary::name).toList());
        RuleSummary vip = summary.rules().get(1);
        assertEquals(10, vip.priority());
        assertEquals(List.of("discount"), vip.outputs());
        assertTrue(vip.action());
        assertEquals(Optional.of("notify_sales"), vip.actionName());
        assertFalse(summary.rules().get(0).action());
        assertEquals(Optional.empty(), summary.rules().get(0).actionName());
        assertEquals(vip, rules.summary().rules().get(1));
        FieldUsage years = summary.fields().get(1);
        assertEquals(List.of("customer", "years"), years.path());
        assertEquals(List.of("vip", "loyal"), years.conditions());
        assertEquals(List.of(), years.payloads());
        assertEquals(years, rules.summary().fields().get(1));
        assertEquals(List.of("discount"), summary.outputs());
        assertEquals(Map.of("loyalty", List.of("vip", "loyal")), summary.tags());
        Map<?, ?> document = (Map<?, ?>) ((Map<?, ?>) TestJson.parse(summary.toJson())).get("summary");
        assertEquals(((List<?>) document.get("fields")).size(), summary.fields().size());
        assertTrue(summary.toString().contains("vip"), summary.toString());

        // A rule whose action is code is marked as having one.
        RuleSet coded = RuleSet.of(rules.rule("loyal").action(context -> {}));
        assertTrue(coded.summary().rules().getFirst().action());
        assertTrue(coded.summary().toJson().contains("\"action\":true"));
        RuleSet payloads = RuleSet.of(Rule.named("p").when(constant(true)).then("t", field("order", "total").mul(2)));
        assertEquals(List.of(new FieldUsage.Payload("p", "t")), payloads.summary().fields().getFirst().payloads());
    }
}
