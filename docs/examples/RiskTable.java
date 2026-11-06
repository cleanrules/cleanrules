/*
 * The decision table of risk.csv with the idiomatic Java API (task G.1d), as
 * risk.mjs does in JavaScript: read from its file, built again from rows in
 * code, evaluated and explained, written as YAML; then one rule read from
 * text, a result tree, and a rule set written as a document and read back
 * with its action bound by name.
 *
 *     (cd bindings/jvm && ./gradlew jar)    # with this platform's library inside
 *     java --enable-native-access=io.github.cleanrules \
 *          --module-path bindings/jvm/java/build/libs --add-modules io.github.cleanrules \
 *          docs/examples/RiskTable.java docs/examples/risk.csv
 */

import static io.github.cleanrules.Conditions.field;
import static io.github.cleanrules.Documents.dumps;
import static io.github.cleanrules.Documents.load;
import static io.github.cleanrules.Documents.loads;
import static io.github.cleanrules.Documents.ruleFromText;

import io.github.cleanrules.Decision;
import io.github.cleanrules.DecisionTable;
import io.github.cleanrules.Format;
import io.github.cleanrules.ResultTree;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import io.github.cleanrules.TableOutput;
import io.github.cleanrules.TablePolicy;
import java.math.BigDecimal;
import java.nio.file.Path;
import java.util.Map;
import java.util.stream.Collectors;

public class RiskTable {
    public static void main(String[] args) {
        // A decision table from its CSV file (the format comes from the extension).
        if (!(load(Path.of(args[0])) instanceof DecisionTable table)) {
            throw new IllegalArgumentException(args[0] + " does not hold a decision table");
        }
        var order = Map.of("customer", Map.of("years", 9), "order", Map.of("total", 6000));
        Decision decision = table.evaluate(order);
        Map<String, Object> output = decision.output().orElseThrow();
        System.out.println("risk: " + output.get("risk") + ", discount: " + output.get("discount"));
        System.out.println(decision.explain().toString().lines().findFirst().orElseThrow());

        // The same table built from rows: strings are cells as the CSV writes
        // them, other values are constants, and a TableOutput lists an output's values.
        DecisionTable built = DecisionTable.builder()
                .hitPolicy(TablePolicy.PRIORITY)
                .header("RULE", field("customer", "years"), "order.total", "THEN discount",
                        TableOutput.of("risk", "high", "medium", "low"), "DESCRIPTION")
                .row("new", field("customer", "years").lt(1), "> 1000", 0, "'high'",
                        "New customers with large orders are checked first")
                .row("vip", "> 5", "-", new BigDecimal("0.15"), "'low'", "Long-standing customers")
                .row("big", null, "> 5000", new BigDecimal("0.05"), "'medium'")
                .row("default", null, null, 0, "'low'")
                .build();
        System.out.println("same table: " + dumps(built, Format.CSV).equals(dumps(table, Format.CSV)));
        System.out.println("rows: " + built.rows().stream()
                .map(row -> row.name().orElse("?"))
                .collect(Collectors.joining(", ")));
        dumps(built, Format.YAML).lines().limit(3).forEach(System.out::println);

        // One rule from text (fuzzytool's verb), and why its condition is unknown.
        Rule rule = ruleFromText("RULE vip IF customer.years > 5 AND order.total > 1000 THEN discount = 0.15");
        ResultTree tree = rule.when().evaluateTree(Map.of("customer", Map.of("years", 7)));
        ResultTree total = tree.children().get(1);
        System.out.println(rule.name() + ": " + tree.truth().orElseThrow().jsonName() + "; " + total.node() + " is "
                + total.truth().orElseThrow().jsonName() + " (" + total.reason().orElseThrow().code() + ")");

        // A rule set written as a document and read back, its action bound by name.
        RuleSet rules = RuleSet.of(ruleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15 DO notify"));
        String yaml = dumps(rules, Format.YAML);
        var read = loads(yaml, Format.YAML, Map.of("notify", context -> {
            ResultTree why = context.tree();
            System.out.println("notify for " + context.rule().name() + ": " + why.node() + " is "
                    + why.truth().orElseThrow().jsonName());
        }));
        switch (read) {
            case RuleSet back -> back.run(order);
            default -> throw new IllegalStateException("the document holds no rule set");
        }
    }
}
