package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.ActionException;
import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.EvaluationException;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.HitPolicyException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.RuleDefinitionException;
import io.github.cleanrules.RuleNotFoundException;
import io.github.cleanrules.RuleSyntaxException;
import io.github.cleanrules.TypeMismatchException;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.ffi.Node;
import io.github.cleanrules.ffi.RuleSet;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.ObjectInputStream;
import java.io.ObjectOutputStream;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalInt;
import org.junit.jupiter.api.Test;

/** One error of each code, with its context (spec/errors.md). */
class ErrorsTest {
    private static final String RULE = "{\"name\": \"a\", \"when\": {\"op\": \"const\", \"value\": true}}";

    @Test
    void formatErrorsPointIntoTheDocument() {
        FormatException shape = assertThrows(
                FormatException.class,
                () -> RuleSet.read("{\"version\": 1, \"ruleset\": {\"rules\": [{\"name\": 1}]}}", Format.JSON));
        assertEquals("format_error", shape.code());
        assertTrue(shape.pointer().orElseThrow().startsWith("/ruleset/rules/0"), shape.json());
        assertEquals(OptionalInt.empty(), shape.line());

        FormatException syntax =
                assertThrows(FormatException.class, () -> RuleSet.read("{\n  \"version\": 1,\n", Format.JSON));
        assertEquals(OptionalInt.of(3), syntax.line());
        assertTrue(syntax.column().isPresent());
        assertEquals(Optional.empty(), syntax.pointer());
        assertTrue(syntax.json().startsWith("{\"code\":\"format_error\""), syntax.json());
    }

    @Test
    void anUnknownVersionIsUnsupported() {
        FormatException error = assertThrows(
                FormatException.class,
                () -> RuleSet.read("{\"version\": 99, \"ruleset\": {\"rules\": [" + RULE + "]}}", Format.JSON));
        assertEquals("unsupported_version", error.code());
        assertEquals(Optional.of("/version"), error.pointer());
    }

    @Test
    void limitsAreFormatErrors() {
        Limits small = Limits.defaults().withMaxSize(8);
        FormatException error =
                assertThrows(FormatException.class, () -> RuleSet.read(Fixtures.DISCOUNTS, Format.TEXT, small));
        assertEquals("limit_exceeded", error.code());
    }

    @Test
    void expressionErrors() {
        ExpressionException error =
                assertThrows(ExpressionException.class, () -> Node.read("(a > 1) + 2", Format.TEXT));
        assertEquals("expression_error", error.code());
        assertEquals(OptionalInt.of(1), error.line());
        assertTrue(error.column().isPresent());
        assertTrue(error.hint().isPresent() || !error.getMessage().isEmpty());
    }

    @Test
    void ruleDefinitionErrors() {
        RuleDefinitionException error = assertThrows(
                RuleDefinitionException.class, () -> RuleSet.read("RULE a IF true\nRULE a IF false", Format.TEXT));
        assertEquals("rule_definition_error", error.code());
        assertEquals(Optional.of("a"), error.rule());
        assertEquals(OptionalInt.of(2), error.line());
    }

    @Test
    void ruleSyntaxErrors() {
        RuleSyntaxException error =
                assertThrows(RuleSyntaxException.class, () -> RuleSet.read("IF a >", Format.TEXT));
        assertEquals("rule_syntax_error", error.code());
        assertEquals(OptionalInt.of(1), error.line());
        assertEquals(OptionalInt.of(7), error.column());
    }

    @Test
    void registryErrorsNameWhatIsMissing() {
        RegistryException logic =
                assertThrows(RegistryException.class, () -> RuleSet.read("LOGIC nope\nIF true", Format.TEXT));
        assertEquals("registry_error", logic.code());
        assertEquals(Optional.of("nope"), logic.name());
        assertTrue(logic.line().isPresent());
        assertTrue(logic.available().contains("boolean"), logic.json());

        try (RuleSet rules = Fixtures.discounts()) {
            RegistryException action = assertThrows(RegistryException.class, () -> rules.checkActions(List.of("other")));
            assertEquals(Optional.of("vip"), action.rule());
            assertEquals(Optional.of("notify_sales"), action.name());
            assertEquals(List.of("other"), action.available());
            rules.checkActions(List.of("notify_sales", "other"));
        }
    }

    @Test
    void strictModeErrors() {
        try (RuleSet strict = RuleSet.read("STRICT\nRULE a IF x > 1", Format.TEXT)) {
            TypeMismatchException mismatch =
                    assertThrows(TypeMismatchException.class, () -> strict.evaluate(Map.of("x", "one")));
            assertEquals("type_mismatch", mismatch.code());
            assertEquals(Optional.of("a"), mismatch.rule());
            assertEquals(Optional.of(List.of("x")), mismatch.path());
            assertEquals(List.of("string", "number"), mismatch.types());
            assertTrue(mismatch.node().orElseThrow().startsWith("{\"op\":\"gt\""), mismatch.node().orElseThrow());
            assertEquals(OptionalInt.empty(), mismatch.row());
            assertNull(mismatch.getCause());

            MissingFieldException missing = assertThrows(MissingFieldException.class, () -> strict.evaluateJson("{}"));
            assertEquals(Optional.of(List.of("x")), missing.path());

            // An index in a path is an Integer.
            try (RuleSet indexed = RuleSet.read("STRICT\nRULE a IF items[1] > 1", Format.TEXT)) {
                MissingFieldException index = assertThrows(
                        MissingFieldException.class, () -> indexed.evaluate(Map.of("items", List.of(1))));
                assertEquals(Optional.of(List.of("items", 1)), index.path());
            }
        }
    }

    @Test
    void hitPolicyErrorsListTheRules() {
        try (RuleSet unique = RuleSet.read("HIT POLICY unique\nRULE a IF true\nRULE b IF true", Format.TEXT)) {
            HitPolicyException error = assertThrows(HitPolicyException.class, () -> unique.evaluateJson("{}"));
            assertEquals(List.of("a", "b"), error.rules());
        }
    }

    @Test
    void theHierarchyFollowsTheSpec() {
        MissingFieldException missing;
        try (RuleSet strict = RuleSet.read("STRICT\nRULE a IF x > 1", Format.TEXT)) {
            missing = assertThrows(MissingFieldException.class, () -> strict.evaluateJson("{}"));
        }
        assertInstanceOf(EvaluationException.class, missing);
        assertInstanceOf(CleanRulesException.class, missing);
        assertInstanceOf(RuntimeException.class, missing);
        assertTrue(EvaluationException.class.isAssignableFrom(TypeMismatchException.class));
        assertTrue(EvaluationException.class.isAssignableFrom(HitPolicyException.class));
        for (Class<?> type : List.of(
                FormatException.class,
                ExpressionException.class,
                RuleDefinitionException.class,
                RuleSyntaxException.class,
                RegistryException.class,
                RuleNotFoundException.class,
                EvaluationException.class,
                ActionException.class,
                UnsupportedException.class)) {
            assertEquals(CleanRulesException.class, type.getSuperclass(), type.getName());
        }
    }

    @Test
    void misusesAreIllegalArguments() {
        try (RuleSet rules = Fixtures.discounts()) {
            IllegalArgumentException empty =
                    assertThrows(IllegalArgumentException.class, () -> rules.checkActions(List.of("")));
            assertTrue(empty.getMessage().contains("cr_actions_add"), empty.getMessage());
            assertThrows(NullPointerException.class, () -> rules.evaluateJson((String) null));
        }
        assertThrows(IllegalArgumentException.class, () -> Limits.defaults().withMaxDepth(-1));
    }

    @Test
    void exceptionsSerializeWithTheirContext() throws Exception {
        RuleSyntaxException error =
                assertThrows(RuleSyntaxException.class, () -> RuleSet.read("IF a >", Format.TEXT));
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        try (ObjectOutputStream out = new ObjectOutputStream(bytes)) {
            out.writeObject(error);
        }
        try (ObjectInputStream in = new ObjectInputStream(new ByteArrayInputStream(bytes.toByteArray()))) {
            RuleSyntaxException read = (RuleSyntaxException) in.readObject();
            assertEquals(error.getMessage(), read.getMessage());
            assertEquals(error.code(), read.code());
            assertEquals(error.line(), read.line());
            assertEquals(error.json(), read.json());
        }
    }
}
