package io.github.cleanrules;

/**
 * What a rule needed ({@code spec/explain.md}, <i>Requirements</i>): a
 * {@link Cause}, or a {@link CauseGroup} of requirements.
 */
public sealed interface Requirement permits Cause, CauseGroup {}
