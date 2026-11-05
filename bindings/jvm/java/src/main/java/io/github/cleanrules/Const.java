package io.github.cleanrules;

import java.lang.foreign.MemorySegment;

/**
 * A constant ({@code spec/ast.md}, {@code const}): null, a boolean, a number,
 * a string, a date or a datetime, made by {@link Conditions#constant(Object)}
 * or from a Java value next to a node.
 */
public final class Const extends Expr {
    Const(MemorySegment pointer) {
        super(pointer);
    }

    /**
     * The constant as a Java value ({@code spec/data-model.md}, <i>Values given
     * back</i>): {@code null}, {@link Boolean}, {@link Long}, {@link Double},
     * {@link java.math.BigDecimal}, {@link String}, {@link java.time.LocalDate},
     * {@link java.time.LocalDateTime} or {@link java.time.ZonedDateTime}.
     *
     * @return the value
     */
    public Object value() {
        return detail();
    }
}
