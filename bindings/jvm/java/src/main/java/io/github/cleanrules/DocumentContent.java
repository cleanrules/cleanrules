package io.github.cleanrules;

/**
 * What a document holds ({@code spec/json-format.md}, <i>Documents</i>): a
 * condition ({@link Expr}), a {@link Rule}, a {@link RuleSet} or a
 * {@link DecisionTable}. {@link Documents#loads(String)} gives one, to take
 * apart with a {@code switch}:
 *
 * <pre>{@code
 * switch (Documents.load(Path.of("pricing.yaml"))) {
 *     case RuleSet rules -> rules.evaluate(order);
 *     case DecisionTable table -> table.evaluate(order);
 *     case Rule rule -> RuleSet.of(rule).evaluate(order);
 *     case Expr condition -> condition.evaluateTree(order);
 * }
 * }</pre>
 *
 * and {@link Documents#dumps(DocumentContent)} writes one.
 */
public sealed interface DocumentContent permits Expr, Rule, RuleSet, DecisionTable {}
