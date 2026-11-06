package io.github.cleanrules;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.internal.Bridge;
import io.github.cleanrules.internal.Build;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;

/**
 * Loading and dumping conditions, rules, rule sets and decision tables, and
 * reading one rule from text:
 *
 * <pre>{@code
 * import static io.github.cleanrules.Documents.*;
 *
 * if (load(Path.of("pricing.yaml"), Map.of("notify_sales", notify)) instanceof RuleSet rules) {
 *     String yaml = dumps(rules, Format.YAML);
 * }
 * Rule vip = ruleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15");
 * }</pre>
 *
 * <p>The documents are the JSON format of {@code spec/json-format.md}, the
 * contract between languages: a rule set written here is read the same way in
 * Rust, Python or Go. YAML files hold the same documents, the rule syntax
 * ({@code spec/text.md}) the same model, and CSV holds decision tables
 * ({@code spec/tables.md}). The core reads and writes every document, strictly
 * and within limits: unknown or duplicate keys, malformed values and syntax
 * errors are errors with a JSON Pointer or a line and column, and documents
 * larger or deeper than the limits are refused. A file's format is its
 * extension's ({@code .json}, {@code .yaml}, {@code .yml}, {@code .rules},
 * {@code .csv}) unless given.
 *
 * <p>Rules read from a document keep the core's own rules, so they write back
 * exactly. A rule's action is the name the document gives it, never code:
 * pass {@code actions}, a registry, to bind those names ({@link RuleSet#bind(Map)}).
 */
public final class Documents {
    private Documents() {}

    private static final Map<String, Format> EXTENSIONS = Map.of(
            ".json", Format.JSON, ".yaml", Format.YAML, ".yml", Format.YAML, ".rules", Format.TEXT, ".csv", Format.CSV);

    // ---- Reading.

    /**
     * Read a JSON document.
     *
     * @param text the document
     * @return an {@link Expr} for a condition document, a {@link Rule}, a
     *     {@link RuleSet} or a {@link DecisionTable}
     * @throws FormatException for invalid JSON, an unknown or duplicate key, a
     *     wrong type or an unknown version, with a JSON Pointer; or a document
     *     beyond the limits
     * @throws ExpressionException if a condition breaks a construction rule
     * @throws RuleDefinitionException if a rule or rule set breaks a construction rule
     * @throws RegistryException for an unknown logic
     */
    public static DocumentContent loads(String text) {
        return loads(text, Format.JSON, null, null);
    }

    /**
     * Read a document in JSON, YAML, the rule syntax or CSV (a decision table).
     *
     * @param text the document
     * @param format its format
     * @return what the document holds
     * @throws CleanRulesException as {@link #loads(String)}, and a
     *     {@link RuleSyntaxException} for a text that does not follow the
     *     syntax, with the line and column of the first token that cannot be read
     */
    public static DocumentContent loads(String text, Format format) {
        return loads(text, format, null, null);
    }

    /**
     * Read a document, and bind the rules it holds to an action registry.
     *
     * @param text the document
     * @param format its format
     * @param actions each action's name and its code; null to leave named actions unbound
     * @return what the document holds
     * @throws RegistryException for a named action that {@code actions} lacks
     * @throws CleanRulesException as {@link #loads(String, Format)}
     */
    public static DocumentContent loads(String text, Format format, Map<String, ? extends Action> actions) {
        return loads(text, format, null, actions);
    }

    /**
     * Read a document within limits, and bind its rules to an action registry.
     *
     * @param text the document
     * @param format its format
     * @param limits the limits, or null for the defaults
     * @param actions each action's name and its code, or null
     * @return what the document holds
     * @throws CleanRulesException as {@link #loads(String, Format, Map)}
     */
    public static DocumentContent loads(
            String text, Format format, Limits limits, Map<String, ? extends Action> actions) {
        return loads(Text.document(Objects.requireNonNull(text, "text")), format, limits, actions);
    }

    /**
     * Read a document given as UTF-8 bytes within limits, and bind its rules
     * to an action registry. Bytes that are not UTF-8 are a
     * {@link FormatException} at the first invalid byte.
     *
     * @param text the document's bytes
     * @param format its format
     * @param limits the limits, or null for the defaults
     * @param actions each action's name and its code, or null
     * @return what the document holds
     * @throws CleanRulesException as {@link #loads(String, Format, Map)}
     */
    public static DocumentContent loads(
            byte[] text, Format format, Limits limits, Map<String, ? extends Action> actions) {
        Objects.requireNonNull(text, "text");
        Objects.requireNonNull(format, "format");
        Native.ensureLoaded();
        MemorySegment document;
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(Native.documentRead(
                    scope.bytes(text), text.length, Native.format(format), scope.limits(limits), out, scope.error()));
            document = out.get(ADDRESS, 0);
        }
        int kind = Native.documentKind(document);
        if (kind == Native.CR_DOCUMENT_DECISION_TABLE) {
            // The table owns the document from here, and its cleaner frees it.
            DecisionTable table = DecisionTable.fromDocument(document);
            return table;
        }
        DocumentContent content;
        try {
            content = switch (kind) {
                case Native.CR_DOCUMENT_CONDITION -> Expr.make((out, error, scope) -> Native.documentCondition(document, out, error));
                case Native.CR_DOCUMENT_RULE -> Rule.fromCore(Scope.value((out, error) -> Native.documentRule(document, out, error)));
                case Native.CR_DOCUMENT_RULESET -> RuleSet.fromNative(Bridge.ffi().ruleSet(
                        Scope.value((out, error) -> Native.rulesetFromDocument(document, out, error))));
                default -> throw new IllegalStateException("cleanrules read a document of an unknown kind");
            };
        } finally {
            Native.documentFree(document);
        }
        return actions == null ? content : bind(content, actions);
    }

    private static DocumentContent bind(DocumentContent content, Map<String, ? extends Action> actions) {
        return switch (content) {
            case Rule rule -> bindRule(rule, actions);
            case RuleSet rules -> rules.bind(actions);
            default -> content;
        };
    }

    private static Rule bindRule(Rule rule, Map<String, ? extends Action> actions) {
        return RuleSet.of(rule).bind(actions).rules().getFirst();
    }

    /**
     * Read a document from a file, its format given by its extension.
     *
     * @param file the file
     * @return what the document holds
     * @throws IllegalArgumentException if the extension does not tell the format
     * @throws UncheckedIOException if the file cannot be read
     * @throws CleanRulesException as {@link #loads(String, Format)}
     */
    public static DocumentContent load(Path file) {
        return load(file, null, null, null);
    }

    /**
     * Read a document from a file, its format given by its extension, and bind
     * its rules to an action registry.
     *
     * @param file the file
     * @param actions each action's name and its code, or null
     * @return what the document holds
     * @throws CleanRulesException as {@link #loads(String, Format, Map)}
     */
    public static DocumentContent load(Path file, Map<String, ? extends Action> actions) {
        return load(file, null, null, actions);
    }

    /**
     * Read a document from a file within limits, and bind its rules to an
     * action registry. A file larger than the size limit of its format
     * ({@link Limits#maxSize()} for JSON, {@link Limits#maxYamlSize()} for
     * YAML, {@link Limits#maxTextSize()} for text and CSV) is not read: a
     * {@link FormatException} with code {@code limit_exceeded}.
     *
     * @param file the file
     * @param format its format, or null for its extension's
     * @param limits the limits, or null for the defaults
     * @param actions each action's name and its code, or null
     * @return what the document holds
     * @throws IllegalArgumentException if the format is not given and the extension does not tell it
     * @throws UncheckedIOException if the file cannot be read
     * @throws CleanRulesException as {@link #loads(String, Format, Map)}
     */
    public static DocumentContent load(
            Path file, Format format, Limits limits, Map<String, ? extends Action> actions) {
        Objects.requireNonNull(file, "file");
        Format chosen = format == null ? formatOf(file) : format;
        return loads(read(file, chosen, limits), chosen, limits, actions);
    }

    /** A file's bytes, once its size is checked against the limit of its format. */
    private static byte[] read(Path file, Format format, Limits limits) {
        Limits given = limits == null ? Limits.defaults() : limits;
        long limit = switch (format) {
            case JSON -> given.maxSize();
            case YAML -> given.maxYamlSize();
            case TEXT, CSV -> given.maxTextSize();
        };
        String raise = switch (format) {
            case JSON -> "Limits.withMaxSize";
            case YAML -> "Limits.withMaxYamlSize";
            case TEXT, CSV -> "Limits.withMaxTextSize";
        };
        try {
            long size = Files.size(file);
            if (size > limit) {
                String message = name(file) + " has " + size + " bytes, more than the limit of " + limit;
                String hint = "raise the limit with " + raise + " if the file is trusted";
                throw new FormatException(
                        "limit_exceeded", message, hint, Build.json("limit_exceeded", message, hint, null, null), null);
            }
            return Files.readAllBytes(file);
        } catch (IOException failure) {
            throw new UncheckedIOException(failure);
        }
    }

    /**
     * Read one rule written in the rule syntax ({@code spec/text.md}), as
     * fuzzytool's {@code rule_from_text} does: without a {@code RULE} line it
     * is named {@code rule_1}. Line breaks are free. To read a condition or a
     * rule set written as text, use {@link #loads(String, Format)} with
     * {@link Format#TEXT}.
     *
     * <pre>{@code
     * Rule vip = ruleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15");
     * }</pre>
     *
     * <p>The two-argument forms take limits, code or a registry, told apart by
     * type, so a bare {@code null} as the second argument matches all three
     * and does not compile: use {@link #ruleFromText(String, Limits, Map)},
     * where both may be null.
     *
     * @param text the rule
     * @return the rule; its action, if it names one, is unbound
     * @throws RuleSyntaxException if the text does not follow the syntax, or
     *     holds a condition or a rule set instead of one rule
     * @throws ExpressionException if a condition breaks a construction rule
     * @throws RuleDefinitionException if the rule breaks a construction rule
     */
    public static Rule ruleFromText(String text) {
        return ruleFromText(text, null, null);
    }

    /**
     * Read one rule written in the rule syntax within limits ({@link Limits#maxTextSize()},
     * {@link Limits#maxDepth()}).
     *
     * @param text the rule
     * @param limits the limits, or null for the defaults
     * @return the rule
     * @throws CleanRulesException as {@link #ruleFromText(String)}
     */
    public static Rule ruleFromText(String text, Limits limits) {
        return ruleFromText(text, limits, null);
    }

    /**
     * Read one rule written in the rule syntax within limits, and bind its
     * action to a registry.
     *
     * @param text the rule
     * @param limits the limits, or null for the defaults
     * @param actions each action's name and its code, or null to leave a named action unbound
     * @return the rule
     * @throws RegistryException if the rule names an action that {@code actions} lacks
     * @throws CleanRulesException as {@link #ruleFromText(String)}
     */
    public static Rule ruleFromText(String text, Limits limits, Map<String, ? extends Action> actions) {
        Rule rule = readRule(text, limits);
        return actions == null ? rule : bindRule(rule, actions);
    }

    private static Rule readRule(String text, Limits limits) {
        byte[] bytes = Text.document(Objects.requireNonNull(text, "text"));
        Native.ensureLoaded();
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(Native.ruleFromText(scope.bytes(bytes), bytes.length, scope.limits(limits), out, scope.error()));
            return Rule.fromCore(out.get(ADDRESS, 0));
        }
    }

    /**
     * Read one rule written in the rule syntax, with code as its action: text
     * holds no code. When the text names an action ({@code DO notify}), the
     * code is bound to that name.
     *
     * @param text the rule
     * @param action the rule's action
     * @return the rule
     * @throws CleanRulesException as {@link #ruleFromText(String)}
     */
    public static Rule ruleFromText(String text, Action action) {
        Objects.requireNonNull(action, "action");
        return ruleFromText(text).action(action);
    }

    /**
     * Read one rule written in the rule syntax, and bind its action to a registry.
     *
     * @param text the rule
     * @param actions each action's name and its code
     * @return the rule
     * @throws RegistryException if the rule names an action that {@code actions} lacks
     * @throws CleanRulesException as {@link #ruleFromText(String)}
     */
    public static Rule ruleFromText(String text, Map<String, ? extends Action> actions) {
        return ruleFromText(text, null, Objects.requireNonNull(actions, "actions"));
    }

    // ---- Writing.

    /**
     * Write a condition, rule, rule set or decision table as compact JSON, in canonical form.
     *
     * @param value what to write
     * @return the document's text
     * @throws ExpressionException if {@code value} is a value node, not a condition
     * @throws UnsupportedException if a rule's action is code without a name
     */
    public static String dumps(DocumentContent value) {
        return dumps(value, Format.JSON, false);
    }

    /**
     * Write a condition, rule, rule set or decision table in a format, in canonical form.
     *
     * @param value what to write
     * @param format JSON (compact), YAML (block style), TEXT (the canonical
     *     text, {@code IF ... THEN ...}) or CSV (decision tables only)
     * @return the document's text
     * @throws UnsupportedException if a rule's action is code without a name,
     *     for CSV and anything but a decision table, and, in text, for a rule
     *     with metadata and for a decision table
     * @throws ExpressionException if {@code value} is a value node, not a condition
     */
    public static String dumps(DocumentContent value, Format format) {
        return dumps(value, format, false);
    }

    /**
     * Write a condition, rule, rule set or decision table in a format.
     *
     * @param value what to write
     * @param format the format
     * @param omitActions write rules whose action is code without it, instead
     *     of refusing them: a document holds an action's name, never code
     * @return the document's text
     * @throws CleanRulesException as {@link #dumps(DocumentContent, Format)}
     */
    public static String dumps(DocumentContent value, Format format, boolean omitActions) {
        Objects.requireNonNull(value, "value");
        Objects.requireNonNull(format, "format");
        return switch (value) {
            case DecisionTable table -> table.write(format);
            case Expr condition -> write(condition.with(pointer ->
                    Scope.value((out, error) -> Native.documentNewCondition(pointer, out, error))), format);
            case Rule rule -> {
                checkActions(List.of(rule), "/rule", omitActions);
                MemorySegment pointer = rule.acquire();
                MemorySegment document;
                try {
                    document = Scope.value((out, error) -> Native.documentNewRule(pointer, out, error));
                } finally {
                    rule.release();
                }
                yield write(document, format);
            }
            case RuleSet rules -> {
                checkActions(rules.rules(), "/ruleset/rules/", omitActions);
                yield rules.nativeRuleSet().write(format);
            }
        };
    }

    /** The text of an owned document, which this frees. */
    private static String write(MemorySegment document, Format format) {
        try {
            return Scope.text((out, len, error) ->
                    Native.documentWrite(document, Native.format(format), out, len, error));
        } finally {
            Native.documentFree(document);
        }
    }

    /** Refuse a rule whose action is code without a name, unless {@code omitActions}. */
    private static void checkActions(List<Rule> rules, String pointer, boolean omitActions) {
        if (omitActions) {
            return;
        }
        for (int position = 0; position < rules.size(); position++) {
            Rule rule = rules.get(position);
            // A named action is written as its name; only code cannot be.
            if (rule.action().isPresent() && rule.actionName().isEmpty()) {
                String message = "the action of rule " + Build.quote(rule.name())
                        + " cannot be written: it is code without a name";
                String hint = "name it (action(\"notify\")) and bind it with RuleSet.bind, or pass omitActions"
                        + " to write the rule without it";
                String at = (pointer.endsWith("/") ? pointer + position : pointer) + "/do";
                throw new UnsupportedException(
                        "unsupported", message, hint, Build.json("unsupported", message, hint, at, rule.name()), null);
            }
        }
    }

    /**
     * Write a condition, rule, rule set or decision table to a file, in the
     * format of its extension. A rule text ends with a line break; line breaks
     * are written as the document has them, on every platform.
     *
     * @param value what to write
     * @param file the file
     * @throws IllegalArgumentException if the extension does not tell the format
     * @throws UncheckedIOException if the file cannot be written
     * @throws CleanRulesException as {@link #dumps(DocumentContent, Format)}
     */
    public static void dump(DocumentContent value, Path file) {
        dump(value, file, null, false);
    }

    /**
     * Write a condition, rule, rule set or decision table to a file.
     *
     * @param value what to write
     * @param file the file
     * @param format the format, or null for its extension's
     * @param omitActions as {@link #dumps(DocumentContent, Format, boolean)}
     * @throws IllegalArgumentException if the format is not given and the extension does not tell it
     * @throws UncheckedIOException if the file cannot be written
     * @throws CleanRulesException as {@link #dumps(DocumentContent, Format)}
     */
    public static void dump(DocumentContent value, Path file, Format format, boolean omitActions) {
        Objects.requireNonNull(file, "file");
        Format chosen = format == null ? formatOf(file) : format;
        String text = dumps(value, chosen, omitActions);
        writeFile(file, chosen == Format.TEXT ? text + "\n" : text);
    }

    private static void writeFile(Path file, String text) {
        try {
            Files.writeString(file, text, StandardCharsets.UTF_8);
        } catch (IOException failure) {
            throw new UncheckedIOException(failure);
        }
    }

    /** The format of a file, from its extension. */
    private static Format formatOf(Path file) {
        String name = name(file);
        int dot = name.lastIndexOf('.');
        Format format = dot < 0 ? null : EXTENSIONS.get(name.substring(dot).toLowerCase(Locale.ROOT));
        if (format == null) {
            throw new IllegalArgumentException("cannot tell the format of " + Build.quote(name)
                    + " from its extension (.json, .yaml, .yml, .rules or .csv); pass one, such as Format.JSON");
        }
        return format;
    }

    private static String name(Path file) {
        Path name = file.getFileName();
        return name == null ? file.toString() : name.toString();
    }
}
