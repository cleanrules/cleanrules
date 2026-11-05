package io.github.cleanrules.internal;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BOOLEAN;
import static java.lang.foreign.ValueLayout.JAVA_DOUBLE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.MemorySegment;
import java.math.BigDecimal;
import java.time.DateTimeException;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.SequencedMap;

/**
 * Values given back to Java ({@code spec/data-model.md}, <i>Java and Scala</i>,
 * <i>Values given back</i>): outputs, payloads and aggregates, from the
 * {@code cr_value}s the library gives.
 *
 * <table>
 * <caption>Values given back</caption>
 * <tr><th>Value</th><th>Java</th></tr>
 * <tr><td>null, whatever its reason</td><td>{@code null}</td></tr>
 * <tr><td>boolean</td><td>{@link Boolean}</td></tr>
 * <tr><td>int</td><td>{@link Long}</td></tr>
 * <tr><td>float</td><td>{@link Double}</td></tr>
 * <tr><td>decimal</td><td>{@link BigDecimal}; the infinities as {@link Double}'s</td></tr>
 * <tr><td>string</td><td>{@link String}</td></tr>
 * <tr><td>date</td><td>{@link LocalDate}</td></tr>
 * <tr><td>naive datetime</td><td>{@link LocalDateTime}</td></tr>
 * <tr><td>aware datetime</td><td>{@link ZonedDateTime} in its zone, or at a {@link ZoneOffset}</td></tr>
 * <tr><td>list</td><td>an unmodifiable {@link List}</td></tr>
 * <tr><td>map</td><td>an unmodifiable {@link SequencedMap}, in the value's order</td></tr>
 * <tr><td>opaque</td><td>the object it was read from, or {@code null}</td></tr>
 * </table>
 */
public final class Values {
    private final List<Object> opaques;
    private final Scope scope = new Scope();
    private final MemorySegment pointer = scope.pointer();
    private final MemorySegment size = scope.size();
    private final MemorySegment[] ints = {scope.int32(), scope.int32(), scope.int32(), scope.int32(),
        scope.int32(), scope.int32(), scope.int32()};
    private final MemorySegment wide = scope.size();
    private final MemorySegment bool = scope.bool();

    private Values(List<Object> opaques) {
        this.opaques = opaques;
    }

    /** The Java value of {@code value}, borrowed; opaque tokens index {@code opaques}. */
    public static Object toJava(MemorySegment value, List<Object> opaques) {
        Values values = new Values(opaques);
        try {
            return values.convert(value);
        } finally {
            values.scope.close();
        }
    }

    /** The Java value of an owned {@code value}, which this frees. */
    public static Object take(MemorySegment value, List<Object> opaques) {
        try {
            return toJava(value, opaques);
        } finally {
            Native.valueFree(value);
        }
    }

    private Object convert(MemorySegment value) {
        int kind = Native.valueKind(value);
        return switch (kind) {
            case Native.CR_VALUE_NULL -> null;
            case Native.CR_VALUE_BOOL -> {
                scope.check(Native.valueGetBool(value, bool, scope.error()));
                yield bool.get(JAVA_BOOLEAN, 0);
            }
            case Native.CR_VALUE_INT -> {
                scope.check(Native.valueGetInt(value, wide, scope.error()));
                yield wide.get(JAVA_LONG, 0);
            }
            case Native.CR_VALUE_FLOAT -> {
                scope.check(Native.valueGetFloat(value, wide, scope.error()));
                yield wide.get(JAVA_DOUBLE, 0);
            }
            case Native.CR_VALUE_DECIMAL -> {
                scope.check(Native.valueGetDecimal(value, pointer, size, scope.error()));
                yield decimal(Scope.takeText(pointer, size));
            }
            case Native.CR_VALUE_STRING -> {
                scope.check(Native.valueGetString(value, pointer, size, scope.error()));
                yield Scope.borrowedText(pointer, size);
            }
            case Native.CR_VALUE_DATE -> {
                scope.check(Native.valueGetDate(value, ints[0], ints[1], ints[2], scope.error()));
                yield LocalDate.of(get(0), get(1), get(2));
            }
            case Native.CR_VALUE_NAIVE_DATETIME -> {
                scope.check(Native.valueGetNaiveDatetime(
                        value, ints[0], ints[1], ints[2], ints[3], ints[4], ints[5], ints[6], scope.error()));
                yield LocalDateTime.of(get(0), get(1), get(2), get(3), get(4), get(5), get(6) * 1000);
            }
            case Native.CR_VALUE_AWARE_DATETIME -> {
                scope.check(Native.valueGetAwareDatetime(
                        value, wide, ints[0], ints[1], pointer, size, scope.error()));
                Instant instant = Instant.ofEpochSecond(wide.get(JAVA_LONG, 0), get(0) * 1000L);
                String zone = pointer.get(ADDRESS, 0).equals(MemorySegment.NULL)
                        ? null
                        : Scope.borrowedText(pointer, size);
                yield aware(instant, get(1), zone);
            }
            case Native.CR_VALUE_LIST -> {
                long length = Native.valueLen(value);
                List<Object> items = new ArrayList<>((int) Math.min(length, Integer.MAX_VALUE));
                for (long i = 0; i < length; i++) {
                    items.add(convert(Native.valueListGet(value, i)));
                }
                yield Collections.unmodifiableList(items);
            }
            case Native.CR_VALUE_MAP -> {
                long length = Native.valueLen(value);
                SequencedMap<String, Object> entries = new LinkedHashMap<>();
                for (long i = 0; i < length; i++) {
                    scope.check(Native.valueMapEntry(value, i, pointer, size, wide, scope.error()));
                    String key = Scope.borrowedText(pointer, size);
                    entries.put(key, convert(wide.get(ADDRESS, 0)));
                }
                yield Collections.unmodifiableSequencedMap(entries);
            }
            case Native.CR_VALUE_OPAQUE -> {
                scope.check(Native.valueGetOpaqueToken(value, wide, scope.error()));
                long token = wide.get(JAVA_LONG, 0);
                yield token >= 0 && token < opaques.size() ? opaques.get((int) token) : null;
            }
            default -> throw new IllegalStateException("cleanrules gave a value of unknown kind " + kind);
        };
    }

    private int get(int index) {
        return ints[index].get(JAVA_INT, 0);
    }

    /** A decimal's canonical text as a BigDecimal; the infinities as Double's. */
    private static Object decimal(String text) {
        return switch (text) {
            case "inf" -> Double.POSITIVE_INFINITY;
            case "-inf" -> Double.NEGATIVE_INFINITY;
            default -> new BigDecimal(text);
        };
    }

    /**
     * An aware datetime: in its IANA zone when Java knows the name, or else
     * at its offset. In a zone, the local fields follow Java's zone database.
     */
    private static ZonedDateTime aware(Instant instant, int offsetSeconds, String zone) {
        if (zone != null) {
            try {
                return ZonedDateTime.ofInstant(instant, ZoneId.of(zone));
            } catch (DateTimeException unknown) {
                // A zone Java's database lacks: the offset keeps the instant.
            }
        }
        return ZonedDateTime.ofInstant(instant, ZoneOffset.ofTotalSeconds(offsetSeconds));
    }

    /** Field paths given as a list of lists: String keys and Integer (or Long) indexes. */
    public static List<List<Object>> paths(MemorySegment value) {
        Object read = take(value, List.of());
        List<List<Object>> paths = new ArrayList<>();
        if (read instanceof List<?> all) {
            for (Object one : all) {
                List<Object> path = new ArrayList<>();
                for (Object segment : (List<?>) one) {
                    // An index as an Integer, or as a Long beyond them.
                    path.add(segment instanceof Long index && index == index.intValue()
                            ? (Object) index.intValue()
                            : segment);
                }
                paths.add(Collections.unmodifiableList(path));
            }
        }
        return Collections.unmodifiableList(paths);
    }
}
