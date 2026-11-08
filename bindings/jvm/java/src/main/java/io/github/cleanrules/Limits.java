package io.github.cleanrules;

import io.github.cleanrules.internal.Native;
import java.util.Arrays;

/**
 * Limits for reading documents and inputs ({@code spec/json-format.md},
 * <i>Errors and limits</i>): an immutable value, which starts from the core's
 * defaults ({@link #defaults()}) and changes one limit at a time
 * ({@link #withMaxSize(long)}...). A document or input beyond a limit is a
 * {@link FormatException} with the code {@code limit_exceeded}, also Java
 * data beyond {@link #maxValues()}, in every mode.
 *
 * <p>Limits hold no native memory and are safe to share between threads.
 */
public final class Limits {
    private static final String[] NAMES = {
        "maxSize", "maxNesting", "maxDepth", "maxTableSize", "maxYamlSize", "maxTextSize", "maxValues"
    };
    private static final int SIZE = 0;
    private static final int NESTING = 1;
    private static final int DEPTH = 2;
    private static final int TABLE_SIZE = 3;
    private static final int YAML_SIZE = 4;
    private static final int TEXT_SIZE = 5;
    private static final int VALUES = 6;

    private static final class Defaults {
        static final Limits LIMITS = new Limits(Native.defaultLimits());
    }

    private final long[] values;

    private Limits(long[] values) {
        this.values = values;
    }

    /**
     * The core's default limits.
     *
     * @return the defaults
     * @throws UnsatisfiedLinkError if the native library cannot be loaded
     */
    public static Limits defaults() {
        // Loaded first, so that a failure never leaves the holder uninitialisable.
        Native.ensureLoaded();
        return Defaults.LIMITS;
    }

    private Limits with(long value, String what, int... limits) {
        if (value < 0) {
            throw new IllegalArgumentException(what + " must be 0 or more, not " + value);
        }
        long[] changed = values.clone();
        for (int limit : limits) {
            changed[limit] = value;
        }
        return new Limits(changed);
    }

    /**
     * The largest JSON text, in bytes.
     *
     * @return the limit
     */
    public long maxSize() {
        return values[SIZE];
    }

    /**
     * The deepest nesting of JSON arrays and objects, also of the lists and
     * maps of an input.
     *
     * @return the limit
     */
    public long maxNesting() {
        return values[NESTING];
    }

    /**
     * The deepest condition or payload value, in nodes.
     *
     * @return the limit
     */
    public long maxDepth() {
        return values[DEPTH];
    }

    /**
     * The most cells and nodes a decision table may build.
     *
     * @return the limit
     */
    public long maxTableSize() {
        return values[TABLE_SIZE];
    }

    /**
     * The largest YAML text, in bytes.
     *
     * @return the limit
     */
    public long maxYamlSize() {
        return values[YAML_SIZE];
    }

    /**
     * The largest text in the rule syntax or CSV, in bytes.
     *
     * @return the limit
     */
    public long maxTextSize() {
        return values[TEXT_SIZE];
    }

    /**
     * The most values read from the Java (or Scala) data of one input
     * ({@code spec/data-model.md}, <i>Reading native data</i>, <i>Limits</i>):
     * 1,000,000 by default. Each value placed where a path ends or stops
     * counts, with everything inside it, an object met through several
     * references each time, and a list along the paths counts its size.
     * An input beyond it is refused, not evaluated: a {@link FormatException}
     * with the code {@code limit_exceeded}, in every mode. Constants and
     * metadata are converted under the default.
     *
     * @return the limit
     */
    public long maxValues() {
        return values[VALUES];
    }

    /**
     * These limits with another limit on the values read from native data.
     *
     * @param count the most values, 0 or more
     * @return the new limits
     * @throws IllegalArgumentException if {@code count} is negative
     */
    public Limits withMaxValues(long count) {
        return with(count, "maxValues", VALUES);
    }

    /**
     * These limits with the largest text of every format, in bytes: the JSON,
     * YAML and text limits at once.
     *
     * @param bytes the largest size, 0 or more
     * @return the new limits
     * @throws IllegalArgumentException if {@code bytes} is negative
     */
    public Limits withMaxSize(long bytes) {
        return with(bytes, "maxSize", SIZE, YAML_SIZE, TEXT_SIZE);
    }

    /**
     * These limits with another JSON nesting limit.
     *
     * @param depth the deepest nesting, 0 or more
     * @return the new limits
     * @throws IllegalArgumentException if {@code depth} is negative
     */
    public Limits withMaxNesting(long depth) {
        return with(depth, "maxNesting", NESTING);
    }

    /**
     * These limits with another node depth limit.
     *
     * @param depth the deepest condition or value, 0 or more
     * @return the new limits
     * @throws IllegalArgumentException if {@code depth} is negative
     */
    public Limits withMaxDepth(long depth) {
        return with(depth, "maxDepth", DEPTH);
    }

    /**
     * These limits with another decision table limit.
     *
     * @param size the most cells and nodes, 0 or more
     * @return the new limits
     * @throws IllegalArgumentException if {@code size} is negative
     */
    public Limits withMaxTableSize(long size) {
        return with(size, "maxTableSize", TABLE_SIZE);
    }

    /**
     * These limits with another YAML size limit.
     *
     * @param bytes the largest YAML text, 0 or more
     * @return the new limits
     * @throws IllegalArgumentException if {@code bytes} is negative
     */
    public Limits withMaxYamlSize(long bytes) {
        return with(bytes, "maxYamlSize", YAML_SIZE);
    }

    /**
     * These limits with another size limit for the rule syntax and CSV.
     *
     * @param bytes the largest text, 0 or more
     * @return the new limits
     * @throws IllegalArgumentException if {@code bytes} is negative
     */
    public Limits withMaxTextSize(long bytes) {
        return with(bytes, "maxTextSize", TEXT_SIZE);
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof Limits limits && Arrays.equals(values, limits.values);
    }

    @Override
    public int hashCode() {
        return Arrays.hashCode(values);
    }

    @Override
    public String toString() {
        StringBuilder text = new StringBuilder("Limits[");
        for (int i = 0; i < NAMES.length; i++) {
            text.append(i == 0 ? "" : ", ").append(NAMES[i]).append('=').append(values[i]);
        }
        return text.append(']').toString();
    }
}
