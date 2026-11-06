/*
 * The rules of discounts.rules, from Java (task G.1a, the native layer): read
 * them, evaluate Java data (a record, a map and a BigDecimal), read the output
 * and which rules fired, explain the decision, run the rules' named action
 * (notify_sales) as a lambda, and catch a strict-mode error.
 *
 *     (cd bindings/jvm && ./gradlew jar)    # with this platform's library inside
 *     java --enable-native-access=io.github.cleanrules \
 *          --module-path bindings/jvm/java/build/libs --add-modules io.github.cleanrules \
 *          docs/examples/Discounts.java docs/examples/discounts.rules
 */

import io.github.cleanrules.ActionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.RuleSet;
import java.math.BigDecimal;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;

public class Discounts {
    /** A customer, read through its components. */
    record Customer(int years, boolean newsletter) {}

    public static void main(String[] args) throws Exception {
        String text = Files.readString(Path.of(args.length > 0 ? args[0] : "docs/examples/discounts.rules"));
        try (RuleSet rules = RuleSet.read(text, Format.TEXT)) {
            // Java data in: only the fields the rules read are read.
            Map<String, Object> order = Map.of(
                    "customer", new Customer(7, false),
                    "order", Map.of("total", new BigDecimal("1500.00")));
            try (Decision decision = rules.evaluate(order)) {
                // Java values out: the discount is a BigDecimal.
                Map<?, ?> output = (Map<?, ?>) decision.output();
                System.out.println("discount: " + output.get("discount"));
                for (int rule : decision.selected()) {
                    System.out.println("selected: " + decision.ruleName(rule));
                }
                System.out.println(decision.explain());
            }

            // A named action is data; the program gives the code.
            try (Decision run = rules.run(order, Map.of("notify_sales", call ->
                    System.out.println("notify_sales for " + call.ruleName() + ": " + call.output().get("discount"))))) {
                System.out.println("ran: " + run.selected().size() + " rule");
            }

            // An action that throws stops the run.
            try {
                rules.run(order, Map.of("notify_sales", call -> {
                    throw new IllegalStateException("the sales service is down");
                }));
            } catch (ActionException error) {
                System.out.println(error.code() + " in " + error.rule().orElseThrow() + ": " + error.getCause().getMessage());
                error.nativeDecision().ifPresent(Decision::close);
            }
        }

        // Strict mode throws instead of giving unknown.
        try (RuleSet strict = RuleSet.read("STRICT\nRULE adult IF customer.age >= 18", Format.TEXT)) {
            strict.evaluate(Map.of("customer", Map.of()));
        } catch (MissingFieldException error) {
            System.out.println(error.code() + ": " + error.getMessage());
        }
    }
}
