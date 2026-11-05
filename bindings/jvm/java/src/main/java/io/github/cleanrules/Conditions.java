package io.github.cleanrules;

import io.github.cleanrules.internal.Build;
import io.github.cleanrules.internal.Native;
import java.lang.foreign.MemorySegment;
import java.util.Arrays;
import java.util.List;
import java.util.Objects;

/**
 * Where conditions start: fields, constants and the connectives, meant to be
 * imported statically.
 *
 * <pre>{@code
 * import static io.github.cleanrules.Conditions.*;
 *
 * Expr vip = and(field("customer", "years").gt(5), field("order", "total").gt(1000));
 * Expr open = not(field("account", "closed"));
 * }</pre>
 *
 * <p>The core builds each node and applies the construction rules
 * ({@code spec/ast.md}), so a node that breaks one is an
 * {@link ExpressionException} with the core's message.
 */
public final class Conditions {
    private Conditions() {}

    /**
     * A field by its path: keys ({@link String}, never split at dots) and
     * indexes ({@link Integer} or {@link Long}, from 0), outermost first, such
     * as {@code field("customer", "age")} or {@code field("items", 0)}.
     *
     * @param path the segments
     * @return the field
     * @throws ExpressionException for an empty path, an empty key, a negative
     *     index or a segment of another kind
     */
    public static Field field(Object... path) {
        Objects.requireNonNull(path, "path");
        Native.ensureLoaded();
        MemorySegment segments = Build.data(Arrays.asList(path));
        try {
            return (Field) Expr.make((out, error, scope) -> Native.nodeField(segments, out, error));
        } finally {
            Native.valueFree(segments);
        }
    }

    /**
     * A constant, such as {@code constant(true)} where no node is involved;
     * next to a node, Java values become constants on their own.
     *
     * @param value {@code null}, a boolean, a number, a string, an enum
     *     constant, a date or a datetime
     * @return the constant
     * @throws ExpressionException for a NaN, an integer beyond 64 bits, a
     *     decimal beyond 38 digits, a datetime finer than a microsecond, or a
     *     value that is not a scalar
     */
    public static Const constant(Object value) {
        Native.ensureLoaded();
        MemorySegment converted = Build.constant(value);
        try {
            return (Const) Expr.make((out, error, scope) -> Native.nodeConstant(converted, out, error));
        } finally {
            Native.valueFree(converted);
        }
    }

    /**
     * The conjunction of two or more conditions, flattened.
     *
     * @param operands the conditions
     * @return an {@code and} node
     * @throws ExpressionException for fewer than two, or an operand that is a value
     */
    public static Expr and(Expr... operands) {
        return Expr.connective(true, List.of(operands));
    }

    /**
     * The disjunction of two or more conditions, flattened.
     *
     * @param operands the conditions
     * @return an {@code or} node
     * @throws ExpressionException for fewer than two, or an operand that is a value
     */
    public static Expr or(Expr... operands) {
        return Expr.connective(false, List.of(operands));
    }

    /**
     * The negation of a condition.
     *
     * @param operand the condition
     * @return a {@code not} node
     * @throws ExpressionException if the operand is a value
     */
    public static Expr not(Expr operand) {
        return Objects.requireNonNull(operand, "operand").not();
    }
}
