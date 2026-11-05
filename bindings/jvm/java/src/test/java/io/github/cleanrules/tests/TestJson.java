package io.github.cleanrules.tests;

import java.math.BigInteger;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A lossless JSON reader and writer for the conformance tests: objects keep
 * their members in order ({@link LinkedHashMap}), numbers keep their text
 * ({@link Num}), so nothing that the vectors compare is lost
 * (conformance/README.md, Comparing).
 */
final class TestJson {
    /** A number, as written. */
    record Num(String text) {
        boolean integral() {
            return !(text.contains(".") || text.contains("e") || text.contains("E"));
        }
    }

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
            throw new IllegalArgumentException("text after the value at " + json.at);
        }
        return value;
    }

    private Object value() {
        char c = text.charAt(at);
        switch (c) {
            case '{' -> {
                at++;
                Map<String, Object> members = new LinkedHashMap<>();
                space();
                if (text.charAt(at) == '}') {
                    at++;
                    return members;
                }
                while (true) {
                    space();
                    String key = string();
                    space();
                    expect(':');
                    space();
                    members.put(key, value());
                    space();
                    if (text.charAt(at) == ',') {
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
                if (text.charAt(at) == ']') {
                    at++;
                    return items;
                }
                while (true) {
                    space();
                    items.add(value());
                    space();
                    if (text.charAt(at) == ',') {
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
            case 't' -> {
                at += 4;
                return Boolean.TRUE;
            }
            case 'f' -> {
                at += 5;
                return Boolean.FALSE;
            }
            case 'n' -> {
                at += 4;
                return null;
            }
            default -> {
                int start = at;
                while (at < text.length() && "+-0123456789.eE".indexOf(text.charAt(at)) >= 0) {
                    at++;
                }
                if (start == at) {
                    throw new IllegalArgumentException("unexpected " + c + " at " + at);
                }
                return new Num(text.substring(start, at));
            }
        }
    }

    private String string() {
        expect('"');
        StringBuilder out = new StringBuilder();
        while (true) {
            char c = text.charAt(at++);
            if (c == '"') {
                return out.toString();
            }
            if (c != '\\') {
                out.append(c);
                continue;
            }
            char escape = text.charAt(at++);
            switch (escape) {
                case 'b' -> out.append('\b');
                case 'f' -> out.append('\f');
                case 'n' -> out.append('\n');
                case 'r' -> out.append('\r');
                case 't' -> out.append('\t');
                case 'u' -> {
                    out.append((char) Integer.parseInt(text.substring(at, at + 4), 16));
                    at += 4;
                }
                default -> out.append(escape);
            }
        }
    }

    private void space() {
        while (at < text.length() && " \t\r\n".indexOf(text.charAt(at)) >= 0) {
            at++;
        }
    }

    private void expect(char c) {
        if (text.charAt(at) != c) {
            throw new IllegalArgumentException("'" + c + "' expected at " + at);
        }
        at++;
    }

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
            default -> throw new IllegalArgumentException("not JSON: " + value);
        }
    }

    /**
     * Whether two parsed values are the same (conformance/README.md,
     * Comparing): members in order, numbers by lexical class and exact value
     * with the sign of a float zero, any two opaque values equal.
     */
    static boolean same(Object a, Object b) {
        if (a instanceof Num x && b instanceof Num y) {
            if (x.integral() != y.integral()) {
                return false;
            }
            if (x.integral()) {
                return new BigInteger(x.text()).equals(new BigInteger(y.text()));
            }
            return Double.doubleToRawLongBits(Double.parseDouble(x.text()))
                    == Double.doubleToRawLongBits(Double.parseDouble(y.text()));
        }
        if (a instanceof Map<?, ?> x && b instanceof Map<?, ?> y) {
            if (x.containsKey("$opaque") && y.containsKey("$opaque") && x.size() == 1 && y.size() == 1) {
                return true;
            }
            if (x.size() != y.size()) {
                return false;
            }
            var i = x.entrySet().iterator();
            var j = y.entrySet().iterator();
            while (i.hasNext()) {
                var p = i.next();
                var q = j.next();
                if (!p.getKey().equals(q.getKey()) || !same(p.getValue(), q.getValue())) {
                    return false;
                }
            }
            return true;
        }
        if (a instanceof List<?> x && b instanceof List<?> y) {
            if (x.size() != y.size()) {
                return false;
            }
            for (int k = 0; k < x.size(); k++) {
                if (!same(x.get(k), y.get(k))) {
                    return false;
                }
            }
            return true;
        }
        return a == null ? b == null : a.equals(b);
    }

    /** {@code value} without the members named {@code detail}, at any depth. */
    static Object withoutDetail(Object value) {
        if (value instanceof Map<?, ?> members) {
            Map<String, Object> copy = new LinkedHashMap<>();
            members.forEach((key, item) -> {
                if (!"detail".equals(key)) {
                    copy.put((String) key, withoutDetail(item));
                }
            });
            return copy;
        }
        if (value instanceof List<?> items) {
            List<Object> copy = new ArrayList<>();
            items.forEach(item -> copy.add(withoutDetail(item)));
            return copy;
        }
        return value;
    }
}
