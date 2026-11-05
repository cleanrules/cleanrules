/**
 * The native layer of cleanrules for Java, on the C ABI through the Foreign
 * Function &amp; Memory API: documents, rule sets, decisions and single nodes,
 * read and written in every format, evaluated on Java data or on JSON, with
 * actions as lambdas. The idiomatic API (conditions and rules built in code,
 * explanations as objects) comes on top of it.
 *
 * <p>{@link io.github.cleanrules.ffi.Document},
 * {@link io.github.cleanrules.ffi.RuleSet},
 * {@link io.github.cleanrules.ffi.Decision} and
 * {@link io.github.cleanrules.ffi.Node} own native memory: they are
 * {@link AutoCloseable}, with a cleaner behind them, and may be used from
 * several threads at once.
 */
package io.github.cleanrules.ffi;
