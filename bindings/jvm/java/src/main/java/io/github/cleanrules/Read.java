package io.github.cleanrules;

import java.math.BigInteger;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

/** Reading the maps and lists the core gives (explanations and summaries) into the API's objects. */
final class Read {
    private Read() {}

    @SuppressWarnings("unchecked") // The core gives maps with string keys.
    static Map<String, Object> map(Object value) {
        return (Map<String, Object>) value;
    }

    static List<?> list(Object value) {
        return (List<?>) value;
    }

    static List<String> strings(Object value) {
        List<String> strings = new ArrayList<>();
        for (Object item : list(value)) {
            strings.add((String) item);
        }
        return Collections.unmodifiableList(strings);
    }

    /** A path: String keys, and indexes as Integer (or Long beyond them). */
    static List<Object> path(Object value) {
        List<Object> path = new ArrayList<>();
        for (Object segment : list(value)) {
            path.add(segment instanceof Long index && index == index.intValue() ? (Object) index.intValue() : segment);
        }
        return Collections.unmodifiableList(path);
    }

    static List<List<Object>> paths(Object value) {
        List<List<Object>> paths = new ArrayList<>();
        for (Object path : list(value)) {
            paths.add(path(path));
        }
        return Collections.unmodifiableList(paths);
    }

    /** An int of the JSON format: a number, or {@code {"$int": "..."}} beyond 2^53. */
    static long integer(Object value) {
        return switch (value) {
            case Long number -> number;
            case BigInteger number -> number.longValueExact();
            case Map<?, ?> typed -> Long.parseLong(String.valueOf(typed.get("$int")));
            default -> throw new IllegalStateException("cleanrules gave " + value + " for an int; please report it");
        };
    }
}
