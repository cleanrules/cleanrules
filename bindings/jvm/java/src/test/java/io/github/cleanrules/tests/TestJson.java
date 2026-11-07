package io.github.cleanrules.tests;

import java.math.BigInteger;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * A lossless JSON reader, writer and comparer for the conformance tests
 * (conformance/README.md, Comparing): objects keep their members in order
 * ({@link LinkedHashMap}; a repeated key is an error), numbers keep their text
 * ({@link Num}), so nothing that the vectors compare is lost. The reader
 * checks the syntax of RFC 8259 and refuses text after the value. Nothing
 * here uses the library.
 */
final class TestJson {
    /** A number, as written. */
    record Num(String text) {
        static Num of(long value) {
            return new Num(Long.toString(value));
        }

        static Num of(BigInteger value) {
            return new Num(value.toString());
        }

        /** Whether it is an int literal: no fraction and no exponent (spec/json-format.md, Values). */
        boolean integral() {
            return !(text.contains(".") || text.contains("e") || text.contains("E"));
        }
    }

    /** How the actual side of a comparison was made. */
    enum Mode {
        /** Written by the core: compared exactly. */
        EXACT,
        /**
         * Encoded from the Java values given back (spec/data-model.md, Java
         * and Scala, Values given back), which allows two differences: a
         * decimal infinity comes back as a {@code Double} infinity, and an
         * aware datetime in a zone that Java's zone database does not know
         * comes back at a {@code ZoneOffset}.
         */
        GIVEN,
        /** As {@link #GIVEN}, from a decision read from a document: an opaque value comes back as {@code null}. */
        RECORDED
    }

    private static final Pattern NUMBER = Pattern.compile("-?(?:0|[1-9][0-9]*)(?:\\.[0-9]+)?(?:[eE][+-]?[0-9]+)?");

    private final String text;
    private int at;

    private TestJson(String text) {
        this.text = text;
    }

    static Object parse(String text) {
        TestJson json = new TestJson(text);
        json.space();
        Object value = json.value();
        json.space();
        if (json.at != text.length()) {
            throw json.error("text after the value");
        }
        return value;
    }

    private IllegalArgumentException error(String what) {
        String shown = text.length() > 200 ? text.substring(0, 200) + "..." : text;
        return new IllegalArgumentException(what + " at " + at + " of " + shown);
    }

    private char peek() {
        if (at >= text.length()) {
            throw error("unexpected end");
        }
        return text.charAt(at);
    }

    private Object value() {
        char c = peek();
        switch (c) {
            case '{' -> {
                at++;
                Map<String, Object> members = new LinkedHashMap<>();
                space();
                if (peek() == '}') {
                    at++;
                    return members;
                }
                while (true) {
                    space();
                    int start = at;
                    String key = string();
                    if (members.containsKey(key)) {
                        at = start;
                        throw error("repeated key " + key);
                    }
                    space();
                    expect(':');
                    space();
                    members.put(key, value());
                    space();
                    if (peek() == ',') {
                        at++;
                        continue;
                    }
                    expect('}');
                    return members;
                }
            }
            case '[' -> {
                at++;
                List<Object> items = new ArrayList<>();
                space();
                if (peek() == ']') {
                    at++;
                    return items;
                }
                while (true) {
                    space();
                    items.add(value());
                    space();
                    if (peek() == ',') {
                        at++;
                        continue;
                    }
                    expect(']');
                    return items;
                }
            }
            case '"' -> {
                return string();
            }
            default -> {
                for (Object literal : new Object[] {Boolean.TRUE, Boolean.FALSE, null}) {
                    String word = String.valueOf(literal);
                    if (text.startsWith(word, at)) {
                        at += word.length();
                        return literal;
                    }
                }
                Matcher number = NUMBER.matcher(text).region(at, text.length());
                if (!number.lookingAt()) {
                    throw error("unexpected character");
                }
                at = number.end();
                return new Num(number.group());
            }
        }
    }

    private String string() {
        expect('"');
        StringBuilder out = new StringBuilder();
        while (true) {
            char c = peek();
            if (c < 0x20) {
                throw error("control character in a string");
            }
            at++;
            if (c == '"') {
                return out.toString();
            }
            if (c != '\\') {
                out.append(c);
                continue;
            }
            char escape = peek();
            switch (escape) {
                case '"', '\\', '/' -> out.append(escape);
                case 'b' -> out.append('\b');
                case 'f' -> out.append('\f');
                case 'n' -> out.append('\n');
                case 'r' -> out.append('\r');
                case 't' -> out.append('\t');
                case 'u' -> {
                    String hex = text.substring(at + 1, Math.min(at + 5, text.length()));
                    if (!hex.matches("[0-9a-fA-F]{4}")) {
                        throw error("bad \\u escape");
                    }
                    // A UTF-16 code unit: a surrogate pair is two escapes.
                    out.append((char) Integer.parseInt(hex, 16));
                    at += 4;
                }
                default -> throw error("bad escape");
            }
            at++;
        }
    }

    private void space() {
        while (at < text.length() && " \t\r\n".indexOf(text.charAt(at)) >= 0) {
            at++;
        }
    }

    private void expect(char c) {
        if (peek() != c) {
            throw error("'" + c + "' expected");
        }
        at++;
    }

    /** Compact JSON, numbers as written. */
    static String write(Object value) {
        StringBuilder out = new StringBuilder();
        write(value, out);
        return out.toString();
    }

    private static void write(Object value, StringBuilder out) {
        switch (value) {
            case null -> out.append("null");
            case Boolean bool -> out.append(bool);
            case Num number -> out.append(number.text());
            case String string -> {
                out.append('"');
                for (int i = 0; i < string.length(); i++) {
                    char c = string.charAt(i);
                    switch (c) {
                        case '"' -> out.append("\\\"");
                        case '\\' -> out.append("\\\\");
                        case '\n' -> out.append("\\n");
                        case '\r' -> out.append("\\r");
                        case '\t' -> out.append("\\t");
                        default -> {
                            if (c < 0x20) {
                                out.append(String.format("\\u%04x", (int) c));
                            } else {
                                out.append(c);
                            }
                        }
                    }
                }
                out.append('"');
            }
            case List<?> items -> {
                out.append('[');
                for (int i = 0; i < items.size(); i++) {
                    if (i > 0) {
                        out.append(',');
                    }
                    write(items.get(i), out);
                }
                out.append(']');
            }
            case Map<?, ?> members -> {
                out.append('{');
                boolean first = true;
                for (Map.Entry<?, ?> member : members.entrySet()) {
                    if (!first) {
                        out.append(',');
                    }
                    first = false;
                    write(member.getKey(), out);
                    out.append(':');
                    write(member.getValue(), out);
                }
                out.append('}');
            }
            default -> throw new IllegalArgumentException("not JSON: " + value.getClass().getName() + " " + value);
        }
    }

    /** Whether two trees are the same, as the core writes them ({@link Mode#EXACT}). */
    static boolean same(Object actual, Object expected) {
        return same(actual, expected, Mode.EXACT, false);
    }

    /**
     * Whether {@code actual} matches {@code expected} (conformance/README.md,
     * Comparing): object members in order, ints exact at any size, floats by
     * exact double and sign of zero, an int never equal to a float, any two
     * opaque values equal, and {@code detail} in reasons ignored; with the
     * differences that {@code mode} allows.
     */
    static boolean same(Object actual, Object expected, Mode mode) {
        return same(actual, expected, mode, false);
    }

    private static boolean same(Object actual, Object expected, Mode mode, boolean inReason) {
        if (expected instanceof Num want) {
            return actual instanceof Num got && sameNumber(got, want);
        }
        if (expected instanceof List<?> want) {
            if (!(actual instanceof List<?> got) || got.size() != want.size()) {
                return false;
            }
            for (int i = 0; i < want.size(); i++) {
                if (!same(got.get(i), want.get(i), mode, false)) {
                    return false;
                }
            }
            return true;
        }
        if (expected instanceof Map<?, ?> want) {
            if (want.size() == 1 && want.containsKey("$opaque")) {
                // Any two opaque values are equal.
                if (mode == Mode.RECORDED && actual == null) {
                    return true;
                }
                return actual instanceof Map<?, ?> got && got.size() == 1 && got.containsKey("$opaque");
            }
            if (mode != Mode.EXACT && want.size() == 1) {
                Object allowed = givenForm(want);
                if (allowed != null) {
                    return same(actual, allowed, Mode.EXACT, false);
                }
            }
            if (!(actual instanceof Map<?, ?> got)) {
                return false;
            }
            List<Map.Entry<?, ?>> a = members(got, inReason);
            List<Map.Entry<?, ?>> e = members(want, inReason);
            if (a.size() != e.size()) {
                return false;
            }
            for (int i = 0; i < e.size(); i++) {
                Object key = e.get(i).getKey();
                if (!key.equals(a.get(i).getKey())
                        || !same(a.get(i).getValue(), e.get(i).getValue(), mode, "reason".equals(key))) {
                    return false;
                }
            }
            return true;
        }
        return expected == null ? actual == null : expected.equals(actual);
    }

    /**
     * What Java gives back for a typed value that Java values cannot hold
     * (spec/data-model.md, Java and Scala, Values given back), or null when
     * Java holds it: a decimal infinity as a float infinity, and a datetime in
     * a zone unknown to Java's zone database at its offset.
     */
    private static Object givenForm(Map<?, ?> typed) {
        if (typed.get("$decimal") instanceof String text && (text.equals("inf") || text.equals("-inf"))) {
            return Map.of("$float", text);
        }
        if (typed.get("$datetime") instanceof String text && text.endsWith("]")) {
            String zone = text.substring(text.indexOf('[') + 1, text.length() - 1);
            if (!JavaValues.knownZone(zone)) {
                String fixed = text.substring(0, text.indexOf('['));
                return Map.of("$datetime", fixed.endsWith("+00:00") ? fixed.substring(0, fixed.length() - 6) + "Z" : fixed);
            }
        }
        return null;
    }

    private static List<Map.Entry<?, ?>> members(Map<?, ?> map, boolean inReason) {
        List<Map.Entry<?, ?>> list = new ArrayList<>();
        for (Map.Entry<?, ?> member : map.entrySet()) {
            if (!(inReason && "detail".equals(member.getKey()))) {
                list.add(member);
            }
        }
        return list;
    }

    private static boolean sameNumber(Num actual, Num expected) {
        if (actual.integral() != expected.integral()) {
            return false;
        }
        if (actual.integral()) {
            return new BigInteger(actual.text()).equals(new BigInteger(expected.text()));
        }
        // The same double, and the same sign of zero.
        return Double.doubleToRawLongBits(Double.parseDouble(actual.text()))
                == Double.doubleToRawLongBits(Double.parseDouble(expected.text()));
    }
}
