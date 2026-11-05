package io.github.cleanrules.internal;

import io.github.cleanrules.ActionException;
import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.EvaluationException;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.HitPolicyException;
import io.github.cleanrules.MissingFieldException;
import io.github.cleanrules.RegistryException;
import io.github.cleanrules.RuleDefinitionException;
import io.github.cleanrules.RuleNotFoundException;
import io.github.cleanrules.RuleSyntaxException;
import io.github.cleanrules.TypeMismatchException;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.ffi.Decision;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.function.Function;

/**
 * Errors of the C ABI as Java exceptions: the hierarchy of
 * {@code spec/errors.md} for error codes, {@link IllegalArgumentException}
 * for {@code CR_INVALID_ARGUMENT} (a misuse of the ABI, which the binding
 * prevents, or a value it cannot build) and {@link IllegalStateException} for
 * {@code CR_INTERNAL_ERROR} (a bug in cleanrules).
 */
public final class Errors {
    private Errors() {}

    /**
     * A value whose reading threw.
     *
     * @param row the input's position in a batch, -1 alone
     * @param path where the value is
     * @param thrown what its reading threw
     */
    public record ReadError(int row, List<Object> path, Throwable thrown) {}

    /**
     * What a call knows about the causes of its errors: the values whose
     * reading threw, for strict mode's {@code evaluation_error}, and what an
     * action threw, for {@code action_error}; the objects of the opaque values
     * read; and how to keep the decision an {@link ActionException} gives.
     */
    public static final class Causes {
        /** The reads that threw. */
        public final List<ReadError> reads = new ArrayList<>();
        /** The objects of the opaque values read, by token. */
        public final List<Object> opaques;
        /** What an action threw, or null. */
        public Throwable action;
        /** An owned copy of a borrowed decision, for an {@link ActionException}; or null. */
        public Function<MemorySegment, Decision> decisions;

        /** Causes for a read whose opaque objects are {@code opaques}. */
        public Causes(List<Object> opaques) {
            this.opaques = opaques;
        }

        /** The cause of an error with {@code code} and {@code context}, or null. */
        Throwable find(String code, Map<String, Object> context) {
            if ("action_error".equals(code)) {
                return action;
            }
            // Strict mode chains what a getter threw (spec/ast.md, *Missing
            // values*): the read whose path is the error's path or a prefix.
            if (!"evaluation_error".equals(code) || !(context.get("path") instanceof List<?> path)) {
                return null;
            }
            long row = context.get("row") instanceof Long given ? given : -1;
            for (ReadError read : reads) {
                if (read.row() == row && prefix(read.path(), path)) {
                    return read.thrown();
                }
            }
            return null;
        }

        private static boolean prefix(List<Object> prefix, List<?> path) {
            if (prefix.size() > path.size()) {
                return false;
            }
            for (int i = 0; i < prefix.size(); i++) {
                Object mine = prefix.get(i);
                Object theirs = path.get(i);
                boolean same = mine instanceof Integer index
                        ? theirs instanceof Long other && other == index.longValue()
                        : mine.equals(theirs);
                if (!same) {
                    return false;
                }
            }
            return true;
        }
    }

    /** The exception for a call that returned {@code status} with {@code error}, which this frees. */
    public static RuntimeException of(int status, MemorySegment error, Causes causes) {
        if (error.equals(MemorySegment.NULL)) {
            return new IllegalStateException(
                    "a cleanrules call failed with status " + status + " and gave no error; please report it");
        }
        try {
            int code = Native.errorCode(error);
            String message = Native.errorMessage(error);
            String hint = Native.errorHint(error);
            if (code == Native.CR_INVALID_ARGUMENT) {
                return new IllegalArgumentException(hint == null ? message : message + " (" + hint + ")");
            }
            if (code == Native.CR_INTERNAL_ERROR) {
                return new IllegalStateException("cleanrules failed: " + message + "; please report it");
            }
            String json = Native.errorJson(error);
            Map<String, Object> context = Json.object(json).values();
            String name = String.valueOf(context.get("code"));
            Throwable cause = causes == null ? null : causes.find(name, context);
            if (code == Native.CR_ACTION_ERROR) {
                MemorySegment borrowed = Native.errorDecision(error);
                Decision decision = borrowed.equals(MemorySegment.NULL) || causes == null || causes.decisions == null
                        ? null
                        : causes.decisions.apply(borrowed);
                return new ActionException(name, message, hint, json, cause, decision);
            }
            return of(name, message, hint, json, cause);
        } finally {
            Native.errorFree(error);
        }
    }

    private static CleanRulesException of(String code, String message, String hint, String json, Throwable cause) {
        return switch (code) {
            case "format_error", "unsupported_version", "limit_exceeded", "decision_mismatch" ->
                new FormatException(code, message, hint, json, cause);
            case "expression_error" -> new ExpressionException(code, message, hint, json, cause);
            case "rule_definition_error" -> new RuleDefinitionException(code, message, hint, json, cause);
            case "rule_syntax_error" -> new RuleSyntaxException(code, message, hint, json, cause);
            case "registry_error" -> new RegistryException(code, message, hint, json, cause);
            case "rule_not_found" -> new RuleNotFoundException(code, message, hint, json, cause);
            case "evaluation_error" -> new EvaluationException(code, message, hint, json, cause);
            case "missing_field" -> new MissingFieldException(code, message, hint, json, cause);
            case "type_mismatch" -> new TypeMismatchException(code, message, hint, json, cause);
            case "hit_policy_error" -> new HitPolicyException(code, message, hint, json, cause);
            case "action_error" -> new ActionException(code, message, hint, json, cause, null);
            case "unsupported" -> new UnsupportedException(code, message, hint, json, cause);
            // A code newer than this binding: still an error of the hierarchy.
            default -> new CleanRulesException(code, message, hint, json, cause);
        };
    }
}
