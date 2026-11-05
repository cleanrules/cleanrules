package io.github.cleanrules;

import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * A value looked up in the input by its path ({@code spec/ast.md},
 * {@code field}): {@code Conditions.field("customer", "age")} or
 * {@code field("items", 0, "price")}. Keys are strings, never split at dots
 * (a key may hold one), and indexes are ints from 0.
 */
public final class Field extends Expr {
    Field(MemorySegment pointer) {
        super(pointer);
    }

    /**
     * The path, outermost first.
     *
     * @return the keys ({@link String}) and indexes ({@link Integer}), an unmodifiable list
     */
    public List<Object> path() {
        List<Object> path = new ArrayList<>();
        for (Object segment : (List<?>) detail()) {
            path.add(segment instanceof Long index && index == index.intValue() ? (Object) index.intValue() : segment);
        }
        return Collections.unmodifiableList(path);
    }

    /**
     * The field further down this one's path: {@code field("items").at(0, "price")}.
     *
     * @param segments keys ({@link String}) and indexes ({@link Integer})
     * @return the field
     * @throws ExpressionException for an empty key or a segment of another kind
     */
    public Field at(Object... segments) {
        List<Object> path = new ArrayList<>(path());
        path.addAll(List.of(segments));
        return Conditions.field(path.toArray());
    }
}
