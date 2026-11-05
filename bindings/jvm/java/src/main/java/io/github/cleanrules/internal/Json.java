package io.github.cleanrules.internal;

import java.math.BigInteger;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A small JSON reader for the texts the library writes, such as the JSON form
 * of an error ({@code spec/errors.md}, <i>JSON form</i>): objects become
 * {@link LinkedHashMap}s in their order, arrays {@link List}s, integers
 * {@link Long} (or {@link BigInteger} beyond 64 bits), other numbers
 * {@link Double}. It also keeps the text of each member of the top object,
 * for members given back as JSON (an error's {@code node}).
 */
public final class Json {
    private final String text;
    private int at;

    private Json(String text) {
        this.text = text;
    }

    /**
     * An object read from JSON: its members, and the text of each.
     *
     * @param values the members' values
     * @param texts the members' JSON texts
     */
    public record Members(Map<String, Object> values, Map<String, String> texts) {}

    /** Read a JSON object. */
    public static Members object(String text) {
        Json json = new Json(text);
        json.space();
        json.expect('{');
        Map<String, Object> values = new LinkedHashMap<>();
        Map<String, String> texts = new LinkedHashMap<>();
        json.space();
        if (!json.take('}')) {
            do {
                json.space();
                String key = json.string();
                json.space();
                json.expect(':');
                json.space();
                int start = json.at;
                values.put(key, json.value());
                texts.put(key, text.substring(start, json.at));
                json.space();
            } while (json.take(','));
            json.expect('}');
        }
        json.space();
        if (json.at != text.length()) {
            throw json.problem("text after the object");
        }
        return new Members(Collections.unmodifiableMap(values), Collections.unmodifiableMap(texts));
    }

    private Object value() {
        if (at >= text.length()) {
            throw problem("a value is missing");
        }
        char c = text.charAt(at);
        switch (c) {
            case '{' -> {
                at++;
                Map<String, Object> members = new LinkedHashMap<>();
                space();
                if (!take('}')) {
                    do {
                        space();
                        String key = string();
                        space();
                        expect(':');
                        space();
                        members.put(key, value());
                        space();
                    } while (take(','));
                    expect('}');
                }
                return members;
            }
            case '[' -> {
                at++;
                List<Object> items = new ArrayList<>();
                space();
                if (!take(']')) {
                    do {
                        space();
                        items.add(value());
                        space();
                    } while (take(','));
                    expect(']');
                }
                return items;
            }
            case '"' -> {
                return string();
            }
            case 't' -> {
                word("true");
                return Boolean.TRUE;
            }
            case 'f' -> {
                word("false");
                return Boolean.FALSE;
            }
            case 'n' -> {
                word("null");
                return null;
            }
            default -> {
                return number();
            }
        }
    }

    private Object number() {
        int start = at;
        boolean integral = true;
        while (at < text.length()) {
            char c = text.charAt(at);
            if (c == '.' || c == 'e' || c == 'E') {
                integral = false;
            } else if (!(c == '-' || c == '+' || (c >= '0' && c <= '9'))) {
                break;
            }
            at++;
        }
        String literal = text.substring(start, at);
        try {
            if (integral) {
                BigInteger big = new BigInteger(literal);
                return big.bitLength() < 64 ? (Object) big.longValueExact() : big;
            }
            return Double.valueOf(literal);
        } catch (NumberFormatException | ArithmeticException bad) {
            throw problem("an invalid number");
        }
    }

    private String string() {
        expect('"');
        StringBuilder out = new StringBuilder();
        while (true) {
            if (at >= text.length()) {
                throw problem("an unterminated string");
            }
            char c = text.charAt(at++);
            if (c == '"') {
                return out.toString();
            }
            if (c != '\\') {
                out.append(c);
                continue;
            }
            if (at >= text.length()) {
                throw problem("an unterminated escape");
            }
            char escape = text.charAt(at++);
            switch (escape) {
                case '"', '\\', '/' -> out.append(escape);
                case 'b' -> out.append('\b');
                case 'f' -> out.append('\f');
                case 'n' -> out.append('\n');
                case 'r' -> out.append('\r');
                case 't' -> out.append('\t');
                case 'u' -> {
                    if (at + 4 > text.length()) {
                        throw problem("a short \\u escape");
                    }
                    try {
                        out.append((char) Integer.parseInt(text.substring(at, at + 4), 16));
                    } catch (NumberFormatException bad) {
                        throw problem("an invalid \\u escape");
                    }
                    at += 4;
                }
                default -> throw problem("an invalid escape");
            }
        }
    }

    private void word(String word) {
        if (!text.startsWith(word, at)) {
            throw problem("an invalid literal");
        }
        at += word.length();
    }

    private void space() {
        while (at < text.length()) {
            char c = text.charAt(at);
            if (c != ' ' && c != '\t' && c != '\n' && c != '\r') {
                return;
            }
            at++;
        }
    }

    private boolean take(char c) {
        if (at < text.length() && text.charAt(at) == c) {
            at++;
            return true;
        }
        return false;
    }

    private void expect(char c) {
        if (!take(c)) {
            throw problem("'" + c + "' expected");
        }
    }

    private IllegalStateException problem(String what) {
        return new IllegalStateException(
                "cleanrules gave invalid JSON (" + what + " at offset " + at + "); please report it");
    }
}
