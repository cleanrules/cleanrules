package io.github.cleanrules;

import io.github.cleanrules.internal.Json;
import java.io.Serial;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalInt;

/**
 * An error of cleanrules: the root of the hierarchy of {@code spec/errors.md},
 * whose classes are named with {@code Exception} instead of {@code Error}.
 *
 * <p>Every error has a stable {@linkplain #code() code}, the same in every
 * language, a message and an optional hint written by the core, and the
 * context fields that apply to its code ({@link #pointer()}, {@link #line()},
 * {@link #rule()}, {@link #path()}...), read from the error's JSON form
 * ({@link #json()}). An error caused by another one (a getter that threw while
 * strict mode read the data, an action that threw) keeps it as its
 * {@linkplain #getCause() cause}.
 *
 * <p>The exceptions are unchecked and immutable.
 */
public class CleanRulesException extends RuntimeException {
    @Serial
    private static final long serialVersionUID = 1L;

    /** The error's code. */
    private final String code;
    /** The core's hint, or null. */
    private final String hint;
    /** The error's JSON form, which holds the context. */
    private final String json;
    /** The JSON form, read when the context is first asked for. */
    private transient Json.Members members;

    /**
     * An error as the core reports it. The binding makes these from the
     * library's errors; the arguments are what the core wrote.
     *
     * @param code the error's code, such as {@code missing_field}
     * @param message the message
     * @param hint the hint, or null
     * @param json the error's JSON form, which holds the context
     * @param cause the error that caused it, or null
     */
    public CleanRulesException(String code, String message, String hint, String json, Throwable cause) {
        super(message, cause);
        this.code = code;
        this.hint = hint;
        this.json = json;
    }

    /**
     * The error's code, such as {@code missing_field} ({@code spec/errors.md},
     * <i>Codes</i>).
     *
     * @return the code
     */
    public String code() {
        return code;
    }

    /**
     * How to fix the error, when the core gives a hint.
     *
     * @return the hint, or empty
     */
    public Optional<String> hint() {
        return Optional.ofNullable(hint);
    }

    /**
     * The error's JSON form ({@code spec/errors.md}, <i>JSON form</i>): its
     * code, message, hint, context and cause, as the core writes it.
     *
     * @return the JSON text
     */
    public String json() {
        return json;
    }

    private Map<String, Object> context() {
        Json.Members read = members;
        if (read == null) {
            read = Json.object(json);
            members = read;
        }
        return read.values();
    }

    private Optional<String> text(String member) {
        return context().get(member) instanceof String value ? Optional.of(value) : Optional.empty();
    }

    private OptionalInt number(String member) {
        return context().get(member) instanceof Long value && value == value.intValue()
                ? OptionalInt.of(value.intValue())
                : OptionalInt.empty();
    }

    private List<String> names(String member) {
        if (!(context().get(member) instanceof List<?> items)) {
            return List.of();
        }
        List<String> names = new ArrayList<>(items.size());
        for (Object item : items) {
            names.add(String.valueOf(item));
        }
        return Collections.unmodifiableList(names);
    }

    /**
     * Where in a document the error is: a JSON Pointer into the document as
     * written.
     *
     * @return the pointer, or empty
     */
    public Optional<String> pointer() {
        return text("pointer");
    }

    /**
     * The line of a syntax error, from 1.
     *
     * @return the line, or empty
     */
    public OptionalInt line() {
        return number("line");
    }

    /**
     * The column of a syntax error, from 1, in Unicode code points.
     *
     * @return the column, or empty
     */
    public OptionalInt column() {
        return number("column");
    }

    /**
     * The name of the rule the error is about.
     *
     * @return the rule's name, or empty
     */
    public Optional<String> rule() {
        return text("rule");
    }

    /**
     * The rules involved, in rule order (a {@code hit_policy_error}).
     *
     * @return the rules' names, empty when none apply
     */
    public List<String> rules() {
        return names("rules");
    }

    /**
     * The full path of the field being read: {@link String} keys and
     * {@link Integer} indexes ({@link Long} for an index beyond them).
     *
     * @return the path, or empty when the error is not about a field
     */
    public Optional<List<Object>> path() {
        if (!(context().get("path") instanceof List<?> segments)) {
            return Optional.empty();
        }
        List<Object> path = new ArrayList<>(segments.size());
        for (Object segment : segments) {
            // Indexes that fit as Integer, the others (beyond any Java list) as Long.
            path.add(segment instanceof Long index && index == index.intValue() ? (Object) index.intValue() : segment);
        }
        return Optional.of(Collections.unmodifiableList(path));
    }

    /**
     * The type groups involved (a {@code type_mismatch}).
     *
     * @return the groups, empty when none apply
     */
    public List<String> types() {
        return names("types");
    }

    /**
     * The position of the input in a batch, from 0.
     *
     * @return the row, or empty outside a batch
     */
    public OptionalInt row() {
        return number("row");
    }

    /**
     * The name looked up (a {@code registry_error} or {@code rule_not_found}).
     *
     * @return the name, or empty
     */
    public Optional<String> name() {
        return text("name");
    }

    /**
     * The names that exist, sorted, where the error lists them.
     *
     * @return the names, empty when none apply
     */
    public List<String> available() {
        return names("available");
    }

    /**
     * The rules whose actions had run before an action failed, in rule order.
     *
     * @return the rules' names, empty when none apply
     */
    public List<String> executed() {
        return names("executed");
    }

    /**
     * The failing node, as a condition node in JSON form.
     *
     * @return the node's JSON text, or empty
     */
    public Optional<String> node() {
        context();
        return Optional.ofNullable(members.texts().get("node"));
    }

    /**
     * What was not supported (an {@code unsupported} error).
     *
     * @return the operation, or empty
     */
    public Optional<String> operation() {
        return text("operation");
    }

    /**
     * Why the operation is not supported.
     *
     * @return the reason, or empty
     */
    public Optional<String> reason() {
        return text("reason");
    }
}
