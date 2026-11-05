/**
 * cleanrules for Java: conditions and rules built in code, rule sets,
 * decisions, explanations and summaries as objects, and what every layer of
 * the binding shares (errors, enums, limits, versions).
 *
 * <pre>{@code
 * import static io.github.cleanrules.Conditions.*;
 *
 * Rule vip = Rule.named("vip")
 *         .when(field("customer", "years").gt(5).and(field("order", "total").gt(1000)))
 *         .then("discount", new BigDecimal("0.15"));
 * Decision decision = RuleSet.of(vip).evaluate(order);
 * decision.output();        // Optional[{discount=0.15}]
 * decision.whyNot("vip");   // the rule's explanation, as objects
 * }</pre>
 *
 * <p>The core builds, checks, evaluates and explains; this package converts
 * Java values and runs the actions. The native layer, on the C ABI, with
 * documents in every format, is {@link io.github.cleanrules.ffi}.
 */
package io.github.cleanrules;
