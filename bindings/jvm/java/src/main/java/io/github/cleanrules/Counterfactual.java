package io.github.cleanrules;

import java.util.List;
import java.util.Objects;
import java.util.Optional;

/**
 * What a comparison needed: {@code path op target}, and how far the value was
 * ({@code spec/explain.md}, <i>Counterfactuals</i>). The engine makes these.
 */
public final class Counterfactual {
    private final List<Object> path;
    private final String op;
    private final Object target;
    private final Object gap;

    Counterfactual(List<Object> path, String op, Object target, Object gap) {
        this.path = List.copyOf(path);
        this.op = op;
        this.target = target;
        this.gap = gap;
    }

    /**
     * The field compared.
     *
     * @return the path: {@link String} keys and {@link Integer} indexes
     */
    public List<Object> path() {
        return path;
    }

    /**
     * The comparison it needed, such as {@code gt}.
     *
     * @return the comparison's tag
     */
    public String op() {
        return op;
    }

    /**
     * The value it was compared with, as values come back ({@code spec/data-model.md}).
     *
     * @return the target
     */
    public Object target() {
        return target;
    }

    /**
     * How far the value was: a number ({@link Long}, {@link Double} or
     * {@link java.math.BigDecimal}); a {@link java.time.Period} of days
     * between dates; a {@link java.time.Duration} between datetimes, to the
     * microsecond.
     *
     * @return the gap, or empty when there is no exact gap
     */
    public Optional<Object> gap() {
        return Optional.ofNullable(gap);
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof Counterfactual counterfactual
                && path.equals(counterfactual.path)
                && op.equals(counterfactual.op)
                && Objects.equals(target, counterfactual.target)
                && Objects.equals(gap, counterfactual.gap);
    }

    @Override
    public int hashCode() {
        return Objects.hash(path, op, target, gap);
    }

    @Override
    public String toString() {
        return "Counterfactual[" + path + " " + op + " " + target + ", gap=" + gap + "]";
    }
}
