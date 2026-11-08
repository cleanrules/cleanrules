package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.field;
import static io.github.cleanrules.Documents.dumps;
import static io.github.cleanrules.Documents.loads;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.Aggregate;
import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.Conditions;
import io.github.cleanrules.Decision;
import io.github.cleanrules.DecisionTable;
import io.github.cleanrules.Expr;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.HitPolicyException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.RuleDefinitionException;
import io.github.cleanrules.RuleSyntaxException;
import io.github.cleanrules.TableOutput;
import io.github.cleanrules.TablePolicy;
import io.github.cleanrules.TableRow;
import io.github.cleanrules.UnsupportedException;
import java.math.BigDecimal;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalLong;
import org.junit.jupiter.api.Test;

/** Decision tables from a grid over the C ABI: cells, header items, errors with pointers, policies. */
class TablesTest {
    private static Map<String, Object> customer(int years, int total) {
        return Map.of("customer", Map.of("years", years), "order", Map.of("total", total));
    }

    @Test
    void textCellsAreReadAsTheTablesCsv() {
        DecisionTable table = DecisionTable.of(
                List.of("RULE", "customer.years", "order.total", "THEN discount", "THEN label"),
                List.of(
                        List.of("vip", "> 5", "> 1000", new BigDecimal("0.15"), "'VIP'"),
                        Arrays.asList("loyal", "> 2", "-", new BigDecimal("0.05"), null)));
        assertEquals(TablePolicy.FIRST, table.hitPolicy());
        assertEquals(Optional.empty(), table.aggregate());
        assertEquals("boolean", table.logic());
        assertEquals(false, table.isStrict());
        assertEquals(List.of("customer.years", "order.total"), table.inputs().stream().map(Expr::toString).toList());
        assertEquals(List.of(new TableOutput("discount"), new TableOutput("label")), table.outputs());
        assertEquals("DecisionTable(first: 2 rows, 4 columns)", table.toString());
        assertEquals(
                "HIT POLICY,first\nRULE,customer.years,order.total,THEN discount,THEN label\n"
                        + "vip,> 5,> 1000,0.15,'VIP'\nloyal,> 2,-,0.05,\n",
                dumps(table, Format.CSV));
        Decision decision = table.evaluate(customer(9, 1500));
        assertEquals(
                Optional.of(Map.of("discount", new BigDecimal("0.15"), "label", "VIP")), decision.output());
        assertEquals(List.of("vip", "loyal"), decision.fired());
        assertEquals(2, table.evaluateMany(List.of(customer(9, 1500), customer(3, 0))).size());
        assertEquals(List.of("vip", "loyal"), table.ruleSet().rules().stream().map(r -> r.name()).toList());
        assertEquals(HitPolicy.FIRST, table.ruleSet().hitPolicy());
        assertEquals(2, table.summary().rules().size());
        assertEquals(dumps(table), table.toJson());
    }

    @Test
    void rowsAreTheCoresWithTheirCells() {
        DecisionTable table = DecisionTable.builder()
                .header("RULE", "PRIORITY", "DESCRIPTION", "age", "THEN band")
                .row("minor", 3, "under age", "< 18", "'minor'")
                .row(null, null, null, "-", "'adult'")
                .build();
        List<TableRow> rows = table.rows();
        assertEquals(2, rows.size());
        TableRow minor = rows.get(0);
        assertEquals(Optional.of("minor"), minor.name());
        assertEquals(OptionalLong.of(3), minor.priority());
        assertEquals(Optional.of("under age"), minor.description());
        assertEquals(List.of(Optional.of(field("age").lt(18))), minor.inputs());
        assertEquals("\"minor\"", minor.outputs().getFirst().orElseThrow().toString());
        TableRow adult = rows.get(1);
        assertEquals(Optional.empty(), adult.name());
        assertEquals(OptionalLong.empty(), adult.priority());
        assertEquals(List.of(Optional.empty()), adult.inputs());
        assertEquals("row_2", table.ruleSet().rules().get(1).name());
        assertEquals(rows, table.rows());
    }

    private enum Risk {
        HIGH,
        LOW {
            @Override
            public String toString() {
                return "low risk";
            }
        }
    }

    @Test
    void enumCellsAreConstantsNotText() {
        DecisionTable table = DecisionTable.builder()
                .header("RULE", "x", "THEN t")
                .row("a", Risk.HIGH, Risk.HIGH)
                .row("b", Optional.of(Risk.LOW), Optional.of(Risk.LOW))
                .build();
        // The constant's name as a string, never the field it would name as text
        // (and its name, not its toString).
        assertEquals(List.of(Optional.of(field("x").eq("HIGH"))), table.rows().get(0).inputs());
        assertEquals(List.of(Optional.of(Conditions.constant("LOW"))), table.rows().get(1).outputs());
        assertEquals("HIT POLICY,first\nRULE,x,THEN t\na,'HIGH','HIGH'\nb,'LOW','LOW'\n", dumps(table, Format.CSV));
        assertEquals(Optional.of(Map.of("t", "HIGH")), table.evaluate(Map.of("x", "HIGH")).output());
        assertEquals(Optional.of(Map.of("t", "LOW")), table.evaluate(Map.of("x", Risk.LOW)).output());
        // As an output column's values.
        DecisionTable listed = DecisionTable.builder()
                .header("x", TableOutput.of("t", Risk.HIGH, Risk.LOW))
                .row("-", Risk.LOW)
                .build();
        assertEquals(List.of(TableOutput.of("t", "HIGH", "LOW")), listed.outputs());
        // Under RULE and DESCRIPTION, an enum constant is the name or
        // description (its name, unquoted); under PRIORITY, an error: the core
        // decides by the column (cr_grid_push_row_cells).
        DecisionTable named = DecisionTable.builder()
                .header("RULE", "x", "THEN t", "DESCRIPTION")
                .row(Risk.HIGH, "> 1", 1, Risk.LOW)
                .build();
        assertEquals(Optional.of("HIGH"), named.rows().getFirst().name());
        assertEquals(Optional.of("LOW"), named.rows().getFirst().description());
        assertEquals("HIT POLICY,first\nRULE,x,THEN t,DESCRIPTION\nHIGH,> 1,1,LOW\n", dumps(named, Format.CSV));
        FormatException priority = assertThrows(FormatException.class, () -> DecisionTable.builder()
                .header("x", "PRIORITY", "THEN t")
                .row("> 1", Risk.HIGH, 1)
                .build());
        assertEquals(Optional.of("/rows/0/1"), priority.pointer());
        // Any CharSequence that is not a String is a value too.
        DecisionTable chars = DecisionTable.builder()
                .header("x", "THEN t")
                .row(new StringBuilder("VIP"), "1")
                .build();
        assertEquals(List.of(Optional.of(field("x").eq("VIP"))), chars.rows().getFirst().inputs());
    }

    @Test
    void headerItemsAndCellsMayBeNodesAndConstants() {
        DecisionTable table = DecisionTable.builder()
                .hitPolicy(TablePolicy.PRIORITY)
                .header(List.of("RULE", field("age"), TableOutput.of("risk", "high", "low")))
                .row("minor", field("age").lt(18), "'high'")
                .row("adult", Optional.empty(), "'low'")
                .row("exact", 40, "'low'")
                .build();
        assertEquals(List.of(TableOutput.of("risk", "high", "low")), table.outputs());
        assertEquals(
                "HIT POLICY,priority\nRULE,age,\"THEN risk IN ('high', 'low')\"\nminor,< 18,'high'\nadult,-,'low'\n"
                        + "exact,40,'low'\n",
                dumps(table, Format.CSV));
        assertEquals(Optional.of(Map.of("risk", "high")), table.evaluate(Map.of("age", 10)).output());
        assertEquals(Optional.of(Map.of("risk", "low")), table.evaluate(Map.of("age", 40)).output());
        // An output's values come back as Java values.
        DecisionTable numbers = DecisionTable.builder()
                .header("x", TableOutput.of("n", 1, 2.5))
                .row("> 1", 1)
                .build();
        assertEquals(List.of(1L, 2.5), numbers.outputs().getFirst().values());
    }

    @Test
    void policiesAggregatesAndLogic() {
        List<String> header = List.of("RULE", "x", "THEN n");
        List<List<Object>> rows = List.of(List.of("a", "> 0", 1), List.of("b", "> 1", 2));
        DecisionTable sum = DecisionTable.builder()
                .header(header)
                .rows(rows)
                .hitPolicy(TablePolicy.COLLECT)
                .aggregate(Aggregate.SUM)
                .build();
        assertEquals(Optional.of(Aggregate.SUM), sum.aggregate());
        assertEquals(Optional.of(Map.of("n", 3L)), sum.evaluate(Map.of("x", 5)).output());
        DecisionTable unique = DecisionTable.builder().header(header).rows(rows).hitPolicy(TablePolicy.UNIQUE).build();
        assertThrows(HitPolicyException.class, () -> unique.evaluate(Map.of("x", 5)));
        DecisionTable order = DecisionTable.builder()
                .header("RULE", "x", TableOutput.of("n", 2, 1))
                .rows(rows)
                .hitPolicy(TablePolicy.OUTPUT_ORDER)
                .strict(true)
                .build();
        assertEquals(TablePolicy.OUTPUT_ORDER, order.hitPolicy());
        assertTrue(order.isStrict());
        assertEquals(List.of("b", "a"), order.evaluate(Map.of("x", 5)).selected().stream()
                .map(r -> r.rule().name())
                .toList());
        assertThrows(CleanRulesException.class, () -> order.evaluate(Map.of()));
        assertThrows(RegistryException.class, () -> DecisionTable.builder()
                .header(header)
                .logic("nope")
                .build());
        RuleDefinitionException noCollect = assertThrows(RuleDefinitionException.class, () -> DecisionTable.builder()
                .header(header)
                .rows(rows)
                .aggregate(Aggregate.SUM)
                .build());
        assertEquals(Optional.of("/aggregate"), noCollect.pointer());
        for (TablePolicy policy : TablePolicy.values()) {
            assertEquals(policy, TablePolicy.fromJsonName(policy.jsonName()));
        }
        assertEquals("output_order", TablePolicy.OUTPUT_ORDER.jsonName());
    }

    @Test
    void errorsPointIntoTheGrid() {
        RuleSyntaxException header = assertThrows(
                RuleSyntaxException.class, () -> DecisionTable.of(List.of("RULE", "age >"), List.of()));
        assertEquals(Optional.of("/header/1"), header.pointer());
        RuleSyntaxException cell = assertThrows(
                RuleSyntaxException.class,
                () -> DecisionTable.of(List.of("RULE", "age"), List.of(List.of("a", "< 1"), List.of("b", "<"))));
        assertEquals(Optional.of("/rows/1/1"), cell.pointer());
        FormatException tooMany = assertThrows(
                FormatException.class, () -> DecisionTable.of(List.of("age"), List.of(List.of("1", "2"))));
        assertTrue(tooMany.pointer().orElseThrow().startsWith("/rows/0"), tooMany.pointer().toString());
        ExpressionException nan = assertThrows(
                ExpressionException.class, () -> DecisionTable.of(List.of("RULE", "age"), List.of(List.of("a", Double.NaN))));
        assertEquals(Optional.of("/rows/0/1"), nan.pointer());
        assertTrue(nan.json().contains("\"pointer\":\"/rows/0/1\""));
        ExpressionException nanOutput = assertThrows(
                ExpressionException.class,
                () -> DecisionTable.of(List.of("age", TableOutput.of("n", Double.NaN)), List.of()));
        assertEquals(Optional.of("/header/1"), nanOutput.pointer());
        UnsupportedException other = assertThrows(
                UnsupportedException.class,
                () -> DecisionTable.of(List.of("age"), List.of(List.of(field("x").gt(1)))));
        assertEquals(Optional.of("/rows/0/0"), other.pointer());
        ExpressionException condition = assertThrows(
                ExpressionException.class, () -> DecisionTable.of(List.of(field("x").gt(1)), List.of()));
        assertEquals(Optional.of("/header/0"), condition.pointer());
        RuleDefinitionException notListed = assertThrows(
                RuleDefinitionException.class,
                () -> DecisionTable.of(List.of("age", TableOutput.of("r", "a")), List.of(List.of("1", "'b'"))));
        assertTrue(notListed.pointer().orElseThrow().startsWith("/rows/0"));
        assertThrows(IllegalArgumentException.class, () -> DecisionTable.builder().header(List.of(1)));
        FormatException big = assertThrows(FormatException.class, () -> DecisionTable.builder()
                .header("age")
                .row("< 1")
                .row("< 2")
                .limits(Limits.defaults().withMaxTableSize(3))
                .build());
        assertEquals("limit_exceeded", big.code());
    }

    @Test
    void tablesAreReadAndWrittenInEachFormat() {
        String csv = "HIT POLICY,C+\nRULE,age,THEN n\na,> 1,1\nb,> 2,2\n";
        DecisionTable table = (DecisionTable) loads(csv, Format.CSV);
        assertEquals(TablePolicy.COLLECT, table.hitPolicy());
        assertEquals(Optional.of(Aggregate.SUM), table.aggregate());
        for (Format format : List.of(Format.CSV, Format.JSON, Format.YAML)) {
            DecisionTable again = (DecisionTable) loads(dumps(table, format), format);
            assertEquals(dumps(table), dumps(again));
            assertEquals(table.rows(), again.rows());
        }
        assertThrows(UnsupportedException.class, () -> dumps(table, Format.TEXT));
        assertEquals(Optional.of(Map.of("n", 3L)), table.evaluate(Map.of("age", 5)).output());
    }
}
