/*
 * The rules of discounts.rules, built in code with the idiomatic Java API
 * (task G.1b): conditions with static imports and methods, rules and a rule
 * set, Java values back (a BigDecimal discount), the explanation as objects
 * and as text, why a rule did not fire, the summary, a named action bound to
 * a lambda, and a strict-mode error.
 *
 *     (cd bindings/jvm && ./gradlew jar)    # with this platform's library inside
 *     java --enable-native-access=io.github.cleanrules \
 *          --module-path bindings/jvm/java/build/libs --add-modules io.github.cleanrules \
 *          docs/examples/DiscountRules.java
 */

import static io.github.cleanrules.Conditions.field;

import io.github.cleanrules.Cause;
import io.github.cleanrules.Decision;
import io.github.cleanrules.HitPolicy;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import java.math.BigDecimal;
import java.util.Map;

public class DiscountRules {
    /** A customer, read through its components. */
    record Customer(int years, boolean newsletter) {}

    public static void main(String[] args) {
        RuleSet rules = RuleSet.builder()
                .add(
                        Rule.named("blocked")
                                .when(field("customer", "blocked"))
                                .then("discount", BigDecimal.ZERO)
                                .priority(99)
                                .description("blocked customers never get a discount"),
                        Rule.named("vip")
                                .when(field("customer", "years").gt(5).and(field("order", "total").gt(1000)))
                                .then("discount", new BigDecimal("0.15"))
                                .priority(10)
                                .tags("loyalty")
                                .action("notify_sales"),
                        Rule.named("loyal")
                                .when(field("customer", "years").gt(2).or(field("customer", "newsletter")))
                                .then("discount", new BigDecimal("0.05"))
                                .priority(5)
                                .tags("loyalty"))
                .hitPolicy(HitPolicy.PRIORITY)
                .build();

        // Java data in, Java values out: the discount is a BigDecimal.
        Map<String, Object> order = Map.of(
                "customer", new Customer(7, false),
                "order", Map.of("total", new BigDecimal("1500.00")));
        Decision decision = rules.evaluate(order);
        System.out.println("discount: " + decision.output().orElseThrow().get("discount"));
        System.out.println("fired: " + String.join(", ", decision.fired()));
        System.out.println();
        System.out.println(decision.explain());
        System.out.println();

        // Why a rule did not fire, as objects.
        Decision small = rules.evaluate(Map.of(
                "customer", new Customer(7, false),
                "order", Map.of("total", new BigDecimal("900.00"))));
        for (Cause cause : small.whyNot("vip").causes()) {
            String needed = cause.counterfactual()
                    .map(c -> "; needs " + c.op() + ", short by " + c.gap().orElse("?"))
                    .orElse("");
            System.out.println("vip: " + cause.node() + " is " + cause.truth().jsonName() + needed);
        }
        System.out.println();
        System.out.println(rules.summary());
        System.out.println();

        // A named action is data; the program binds it to a lambda.
        RuleSet bound = rules.bind(Map.of("notify_sales", context ->
                System.out.println("notify_sales for " + context.rule().name() + ": "
                        + context.output().get("discount"))));
        bound.run(order);

        // Strict mode throws instead of giving unknown.
        RuleSet strict = RuleSet.builder()
                .add(Rule.named("adult").when(field("customer", "age").ge(18)))
                .strict(true)
                .build();
        try {
            strict.evaluate(Map.of("customer", Map.of()));
        } catch (MissingFieldException error) {
            System.out.println();
            System.out.println(error.code() + " in rule " + error.rule().orElseThrow() + ": " + error.path().orElseThrow());
        }
    }
}
