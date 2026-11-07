package io.github.cleanrules.tests;

import io.github.cleanrules.tests.TestJson.Num;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.time.DateTimeException;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.OffsetDateTime;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.SequencedMap;
import java.util.function.Consumer;

/**
 * Values of the JSON format (spec/json-format.md, Values) as Java data, and
 * Java values given back as values of the JSON format, by the tables of
 * spec/data-model.md (Java and Scala) read both ways. Written for the
 * conformance runner from the spec alone: nothing here uses the library.
 */
final class JavaValues {
    private JavaValues() {}

    /** The largest int written as a bare number (spec/json-format.md, Values). */
    private static final BigInteger MAX_SAFE = BigInteger.TWO.pow(53).subtract(BigInteger.ONE);

    /**
     * The next of {@code count} equivalent Java forms of a value: values that
     * several Java classes hold equally (an int, a datetime in {@code Z}) take
     * each form in turn, so that every conversion of the binding is used. The
     * order of the vectors is fixed, so a run is deterministic.
     */
    private static final java.util.concurrent.atomic.AtomicLong TURN = new java.util.concurrent.atomic.AtomicLong();

    private static int turn(int count) {
        return (int) (TURN.getAndIncrement() % count);
    }

    /** A value no Java data holds: the runner skips the vector's native route, and says why. */
    static final class NoJavaForm extends RuntimeException {
        private static final long serialVersionUID = 1L;

        final String reason;

        NoJavaForm(String reason, String what) {
            super(what);
            this.reason = reason;
        }
    }

    /** A value given back that breaks the promises of Values given back. */
    static final class NotGiven extends RuntimeException {
        private static final long serialVersionUID = 1L;

        NotGiven(String message) {
            super(message);
        }
    }

    /** Whether Java's zone database knows a zone, by its exact name. */
    static boolean knownZone(String name) {
        try {
            ZoneId zone = ZoneId.of(name);
            return !(zone instanceof ZoneOffset) && zone.getId().equals(name);
        } catch (DateTimeException unknown) {
            return false;
        }
    }

    // ---- From the format to Java data.

    /**
     * A data value of the format as Java data: {@link Long}, {@link Integer},
     * {@link Short} or {@link BigInteger}, {@link Double}, {@link BigDecimal},
     * {@link String}, {@link Boolean}, {@link LocalDate},
     * {@link LocalDateTime}, {@link Instant}, {@link OffsetDateTime} and
     * {@link ZonedDateTime} (at an offset or in a named zone),
     * {@link ArrayList} and {@link LinkedHashMap}. {@code form} is told the
     * class of each value made.
     *
     * @throws NoJavaForm for a decimal infinity, which no Java value holds
     */
    static Object toJava(Object json, Consumer<String> form) {
        Object value = convert(json, form);
        if (value != null) {
            form.accept(value.getClass().getSimpleName());
        }
        return value;
    }

    private static Object convert(Object json, Consumer<String> form) {
        switch (json) {
            case null -> {
                return null;
            }
            case Num number -> {
                return number.integral() ? integer(number.text()) : (Object) Double.parseDouble(number.text());
            }
            case List<?> items -> {
                List<Object> list = new ArrayList<>(items.size());
                for (Object item : items) {
                    list.add(toJava(item, form));
                }
                return list;
            }
            case Map<?, ?> members -> {
                if (members.size() == 1) {
                    Map.Entry<?, ?> only = members.entrySet().iterator().next();
                    String key = (String) only.getKey();
                    if (key.startsWith("$")) {
                        return typed(key, only.getValue(), form);
                    }
                }
                return map(members, form);
            }
            default -> {
                return json;
            }
        }
    }

    private static Object typed(String key, Object inner, Consumer<String> form) {
        String text = inner instanceof Num n ? n.text() : inner instanceof String s ? s : null;
        return switch (key) {
            case "$int" -> integer(text);
            case "$float" -> switch (text) {
                case "inf" -> Double.POSITIVE_INFINITY;
                case "-inf" -> Double.NEGATIVE_INFINITY;
                default -> Double.parseDouble(text);
            };
            case "$decimal" -> {
                if (text.equals("inf") || text.equals("-inf")) {
                    throw new NoJavaForm("decimal_infinity", "the decimal " + text);
                }
                yield new BigDecimal(text);
            }
            case "$date" -> LocalDate.parse(text);
            case "$datetime" -> datetime(text);
            case "$map" -> map((Map<?, ?>) inner, form);
            default -> throw new IllegalArgumentException("no Java form for " + key);
        };
    }

    private static Map<String, Object> map(Map<?, ?> members, Consumer<String> form) {
        Map<String, Object> map = new LinkedHashMap<>();
        for (Map.Entry<?, ?> member : members.entrySet()) {
            map.put((String) member.getKey(), toJava(member.getValue(), form));
        }
        return map;
    }

    /**
     * An int: {@link Long}, or in turn {@link Integer}, {@link Short} or a
     * {@link BigInteger} where it fits; beyond 64 bits a {@link BigInteger}
     * (null with {@code int_overflow}).
     */
    private static Object integer(String text) {
        BigInteger value = new BigInteger(text);
        if (value.bitLength() >= 64) {
            return value;
        }
        long n = value.longValueExact();
        return switch (turn(4)) {
            case 1 -> n == (int) n ? (Object) (int) n : value;
            case 2 -> n == (short) n ? (Object) (short) n : value;
            case 3 -> value;
            default -> n;
        };
    }

    /**
     * A {@code $datetime}: naive as {@link LocalDateTime}; in {@code Z} as
     * {@link Instant}, or in turn {@link OffsetDateTime} or
     * {@link ZonedDateTime} at {@link ZoneOffset#UTC}; at an offset as
     * {@link OffsetDateTime}; in a zone as the
     * {@link ZonedDateTime} at the written instant (the binding sends the
     * instant and the zone's name, spec/data-model.md, Datetimes).
     */
    static Object datetime(String text) {
        int bracket = text.indexOf('[');
        if (bracket >= 0) {
            String zone = text.substring(bracket + 1, text.length() - 1);
            if (!knownZone(zone)) {
                throw new NoJavaForm("zone_unknown_to_java", "the zone " + zone);
            }
            return OffsetDateTime.parse(text.substring(0, bracket)).atZoneSameInstant(ZoneId.of(zone));
        }
        if (text.endsWith("Z")) {
            // Each is the fixed offset Z (spec/data-model.md, Datetimes).
            Instant instant = Instant.parse(text);
            return switch (turn(3)) {
                case 1 -> instant.atOffset(ZoneOffset.UTC);
                case 2 -> instant.atZone(ZoneOffset.UTC);
                default -> instant;
            };
        }
        if (text.matches(".*[+-]\\d\\d:\\d\\d(:\\d\\d)?$")) {
            return OffsetDateTime.parse(text);
        }
        return LocalDateTime.parse(text);
    }

    // ---- From Java values given back to the format.

    /**
     * A value given back to Java (spec/data-model.md, Java and Scala, Values
     * given back) as a value of the format, by the table read backwards: each
     * class is checked, so an {@link Integer} or a modifiable list is a
     * {@link NotGiven}; an object of any other class is opaque.
     */
    static Object given(Object value) {
        switch (value) {
            case null -> {
                return null;
            }
            case Boolean bool -> {
                return bool;
            }
            case String text -> {
                return text;
            }
            case Long number -> {
                BigInteger big = BigInteger.valueOf(number);
                return big.abs().compareTo(MAX_SAFE) <= 0 ? Num.of(number) : Map.of("$int", big.toString());
            }
            case Double number -> {
                if (number.isNaN()) {
                    throw new NotGiven("NaN given back");
                }
                if (number.isInfinite()) {
                    return Map.of("$float", number > 0 ? "inf" : "-inf");
                }
                return new Num(floatText(number));
            }
            case BigDecimal decimal -> {
                if (decimal.scale() < 0) {
                    throw new NotGiven("the decimal " + decimal + " has a negative scale");
                }
                return Map.of("$decimal", decimal.toPlainString());
            }
            case LocalDate date -> {
                return Map.of("$date", dateText(date));
            }
            case LocalDateTime local -> {
                return Map.of("$datetime", datetimeText(local, null, null));
            }
            case ZonedDateTime zoned -> {
                ZoneId zone = zoned.getZone();
                if (!(zone instanceof ZoneOffset) && !knownZone(zone.getId())) {
                    throw new NotGiven("the ZonedDateTime " + zoned + " is in a zone Java does not know");
                }
                String name = zone instanceof ZoneOffset ? null : zone.getId();
                return Map.of("$datetime", datetimeText(zoned.toLocalDateTime(), zoned.getOffset(), name));
            }
            case List<?> items -> {
                unmodifiable(() -> items.add(null), "list");
                List<Object> list = new ArrayList<>(items.size());
                for (Object item : items) {
                    list.add(given(item));
                }
                return list;
            }
            case Map<?, ?> members -> {
                if (!(members instanceof SequencedMap<?, ?>)) {
                    throw new NotGiven("a map given back as a " + members.getClass().getName() + ", not a SequencedMap");
                }
                unmodifiable(() -> members.clear(), "map");
                Map<String, Object> map = new LinkedHashMap<>();
                boolean dollar = false;
                for (Map.Entry<?, ?> member : members.entrySet()) {
                    String key = (String) member.getKey();
                    dollar |= key.startsWith("$");
                    map.put(key, given(member.getValue()));
                }
                return dollar ? Map.of("$map", map) : map;
            }
            default -> {
                if (value instanceof Number || value instanceof Character) {
                    throw new NotGiven("a " + value.getClass().getName() + " given back");
                }
                return Map.of("$opaque", value.getClass().getName());
            }
        }
    }

    private static void unmodifiable(Runnable change, String what) {
        try {
            change.run();
        } catch (UnsupportedOperationException expected) {
            return;
        }
        throw new NotGiven("a modifiable " + what + " given back");
    }

    /** A path given back ({@link String} keys, {@link Integer} indexes) in the format. */
    static List<Object> path(List<?> path) {
        List<Object> out = new ArrayList<>(path.size());
        for (Object segment : path) {
            out.add(switch (segment) {
                case String key -> key;
                case Integer index -> Num.of(index);
                case Long index -> Num.of(index);
                default -> throw new NotGiven("a path segment " + segment);
            });
        }
        return out;
    }

    /** The text of a finite float, with a fraction or an exponent. */
    static String floatText(double value) {
        // Double.toString always writes a '.' or an 'E': the shortest digits that read back.
        return Double.toString(value);
    }

    static String dateText(LocalDate date) {
        return String.format("%04d-%02d-%02d", date.getYear(), date.getMonthValue(), date.getDayOfMonth());
    }

    /** A datetime's text as the format writes it (spec/json-format.md, Dates and datetimes). */
    static String datetimeText(LocalDateTime local, ZoneOffset offset, String zone) {
        if (local.getNano() % 1000 != 0) {
            throw new NotGiven("a datetime finer than a microsecond: " + local);
        }
        StringBuilder text = new StringBuilder(dateText(local.toLocalDate()));
        text.append(String.format("T%02d:%02d:%02d", local.getHour(), local.getMinute(), local.getSecond()));
        if (local.getNano() != 0) {
            text.append(String.format(".%06d", local.getNano() / 1000));
        }
        if (offset == null) {
            return text.toString();
        }
        int seconds = offset.getTotalSeconds();
        if (seconds == 0 && zone == null) {
            return text.append('Z').toString();
        }
        int size = Math.abs(seconds);
        text.append(seconds < 0 ? '-' : '+').append(String.format("%02d:%02d", size / 3600, size % 3600 / 60));
        if (size % 60 != 0) {
            text.append(String.format(":%02d", size % 60));
        }
        return zone == null ? text.toString() : text.append('[').append(zone).append(']').toString();
    }
}
