package io.github.cleanrules.tests;

import static io.github.cleanrules.Conditions.field;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import io.github.cleanrules.Documents;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.function.Executable;

/**
 * The limit {@code max_path_segments} on the paths of fields
 * ({@code spec/json-format.md}, <i>Errors and limits</i>), and the depth of
 * the Java data read along them.
 */
class PathSegmentsTest {
    /** A path of {@code n} segments, keys and indexes in turns. */
    private static Object[] path(int n) {
        Object[] path = new Object[n];
        for (int i = 0; i < n; i++) {
            path[i] = i % 2 == 0 ? "k" : (Object) 0;
        }
        return path;
    }

    /** Data as deep as {@code path(n)}, with 1 at its end. */
    private static Object along(int n) {
        Object data = 1;
        for (int i = n - 1; i >= 0; i--) {
            data = i % 2 == 0 ? Map.of("k", data) : List.of(data);
        }
        return data;
    }

    private static void beyond(Executable call) {
        FormatException error = assertThrows(FormatException.class, call);
        assertEquals("limit_exceeded", error.code());
        assertEquals("max_path_segments", error.name().orElseThrow());
    }

    @Test
    void fieldsHaveAtMostTheDefault() {
        assertEquals(256, Limits.defaults().maxPathSegments());
        assertEquals(2, Limits.defaults().withMaxPathSegments(2).maxPathSegments());
        assertThrows(IllegalArgumentException.class, () -> Limits.defaults().withMaxPathSegments(-1));
        field(path(256));
        beyond(() -> field(path(257)));
    }

    @Test
    void readersTakeTheLimit() {
        String text = "IF a.b.c THEN y = 1";
        beyond(() -> Documents.ruleFromText(text, Limits.defaults().withMaxPathSegments(2)));
        Documents.ruleFromText(text, Limits.defaults().withMaxPathSegments(3));
    }

    @Test
    void aPathAtTheLimitOverDataAsDeepRunsOnASmallStack() throws InterruptedException {
        Object[] path = path(256);
        RuleSet rules = RuleSet.of(Rule.named("r").when(field(path).eq(1)).then("y", field(path)));
        Object data = along(256);
        List<Object> outputs = new ArrayList<>();
        MaxValuesTest.onSmallStack(() -> outputs.add(rules.evaluate(data).selected().size()));
        assertEquals(List.of(1), outputs);
    }
}
