package io.github.cleanrules.internal;

import io.github.cleanrules.ActionException;
import io.github.cleanrules.ExpressionException;
import java.lang.foreign.MemorySegment;
import java.time.Instant;
import java.time.LocalDateTime;
import java.time.OffsetDateTime;
import java.time.ZonedDateTime;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Optional;

/**
 * Java values converted for the C ABI's constructors ({@code cr_node_*},
 * {@code cr_rule_*}): constants, lists of them, field paths, tags and
 * metadata, read whole with the rules of native data ({@code spec/data-model.md},
 * <i>Java and Scala</i>). The C ABI checks what a value can hold; this class
 * rejects first what a value cannot carry to it, a NaN or a datetime finer
 * than a microsecond, which would be read as a null or truncated
 * ({@code spec/data-model.md}, <i>Constants</i>).
 */
public final class Build {
    private Build() {}

    private static final PathTree WHOLE = PathTree.of(List.of(List.of()));

    /** An owned value of {@code value}, read whole as data. */
    public static MemorySegment data(Object value) {
        try (DataReader reader = new DataReader()) {
            return reader.read(value, WHOLE);
        }
    }

    /** An owned value of a constant, after the checks that only Java can make. */
    public static MemorySegment constant(Object value) {
        check(value, null);
        return data(value);
    }

    /** An owned list of constants, each checked. */
    public static MemorySegment constants(Collection<?> values) {
        return constants(values, null);
    }

    /** An owned list of constants, each checked; an error points at {@code pointer}, if any. */
    public static MemorySegment constants(Collection<?> values, String pointer) {
        List<Object> items = new ArrayList<>(values.size());
        for (Object value : values) {
            check(value, pointer);
            items.add(value);
        }
        return data(items);
    }

    /**
     * Check that {@code value} can be carried to the C ABI as a constant (no
     * NaN, nothing finer than a microsecond); an error points at
     * {@code pointer}, if any.
     */
    public static void check(Object value, String pointer) {
        while (value instanceof Optional<?> optional) {
            value = optional.orElse(null);
        }
        if (value instanceof Double number && number.isNaN() || value instanceof Float small && small.isNaN()) {
            throw expression(
                    "a constant cannot be NaN (" + value + ")", "test for missing values with isNull()", pointer);
        }
        int nanos = switch (value) {
            case LocalDateTime local -> local.getNano();
            case OffsetDateTime aware -> aware.getNano();
            case ZonedDateTime aware -> aware.getNano();
            case Instant instant -> instant.getNano();
            case java.util.Date date when !date.getClass().getName().startsWith("java.sql.D")
                    && !date.getClass().getName().equals("java.sql.Time") -> date.toInstant().getNano();
            case null, default -> 0;
        };
        if (nanos % 1000 != 0) {
            throw expression(
                    "the datetime " + value + " is finer than a microsecond",
                    "datetimes hold microseconds; round the constant first",
                    pointer);
        }
    }

    /** An {@code expression_error} that the binding finds, with its JSON form. */
    public static ExpressionException expression(String message, String hint) {
        return expression(message, hint, null);
    }

    /** An {@code expression_error} that the binding finds at {@code pointer} (or nowhere, for null). */
    public static ExpressionException expression(String message, String hint, String pointer) {
        return new ExpressionException(
                "expression_error", message, hint, json("expression_error", message, hint, pointer, null), null);
    }

    /**
     * The JSON form of an error that the binding finds ({@code spec/errors.md}):
     * its code, message and hint, and a pointer and a rule when not null.
     */
    public static String json(String code, String message, String hint, String pointer, String rule) {
        StringBuilder json = new StringBuilder("{\"code\":").append(quote(code))
                .append(",\"message\":").append(quote(message))
                .append(",\"hint\":").append(quote(hint));
        if (pointer != null) {
            json.append(",\"pointer\":").append(quote(pointer));
        }
        if (rule != null) {
            json.append(",\"rule\":").append(quote(rule));
        }
        return json.append('}').toString();
    }

    /**
     * The {@code action_error} of an action that threw during the idiomatic
     * {@code run}, made by the core ({@code cr_error_new_action}): its
     * message names the action when the rule names it.
     */
    public static ActionException action(
            String rule,
            String actionName,
            List<String> executed,
            Throwable thrown,
            io.github.cleanrules.Decision decision) {
        byte[] ruleBytes = Text.name(rule);
        byte[] nameBytes = actionName == null ? null : Text.name(actionName);
        byte[] detail = Text.name(DataReader.describe(thrown));
        MemorySegment names = data(List.copyOf(executed));
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(Native.errorNewAction(
                    scope.bytes(ruleBytes),
                    ruleBytes.length,
                    nameBytes == null ? MemorySegment.NULL : scope.bytes(nameBytes),
                    nameBytes == null ? 0 : nameBytes.length,
                    scope.bytes(detail),
                    detail.length,
                    names,
                    out,
                    scope.error()));
            MemorySegment error = out.get(java.lang.foreign.ValueLayout.ADDRESS, 0);
            try {
                return new ActionException(
                        "action_error",
                        Native.errorMessage(error),
                        Native.errorHint(error),
                        Native.errorJson(error),
                        thrown,
                        null,
                        decision);
            } finally {
                Native.errorFree(error);
            }
        } finally {
            Native.valueFree(names);
        }
    }

    /** {@code text} as a JSON string. */
    public static String quote(String text) {
        StringBuilder out = new StringBuilder(text.length() + 2).append('"');
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20 || Character.isSurrogate(c)) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        return out.append('"').toString();
    }
}
