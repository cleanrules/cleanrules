package io.github.cleanrules;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.internal.Bridge;
import io.github.cleanrules.internal.Build;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import io.github.cleanrules.internal.Values;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.function.Function;

/**
 * A decision table and the rule set it compiles into ({@code spec/tables.md}):
 * a grid of inputs, outputs and rows.
 *
 * <pre>{@code
 * DecisionTable table = DecisionTable.builder()
 *         .header("RULE", "customer.years", "order.total", "THEN discount")
 *         .row("vip", "> 5", "> 1000", new BigDecimal("0.15"))
 *         .row("loyal", "> 2", "-", new BigDecimal("0.05"))
 *         .build();
 * Decision decision = table.evaluate(Map.of("customer", Map.of("years", 9), "order", Map.of("total", 1500)));
 * }</pre>
 *
 * <p>Each header item is the text of a CSV header field ({@code "RULE"},
 * {@code "PRIORITY"}, {@code "DESCRIPTION"}, {@code "THEN key"},
 * {@code "THEN key IN (...)"} or an input expression), an input {@link Expr},
 * or a {@link TableOutput}. A {@link String} cell is read as in the table's
 * CSV, in the rule syntax: {@code "> 5"}, {@code "-"}, and {@code "'VIP'"} for
 * the string, since {@code "VIP"} is the field named {@code VIP}. An
 * {@link Expr} cell is an input's condition, which tests its column
 * ({@code field("age").lt(18)} under {@code age}), or an output's value;
 * {@code null} and {@code Optional.empty()} are empty cells; any other cell is
 * a constant (under an input {@code x}, the test {@code x = value}; under an
 * output, the value). Rows may be shorter than the header.
 *
 * <p>Or read one with {@link Documents#load(java.nio.file.Path)} or
 * {@link Documents#loads(String, Format)} (CSV, JSON or YAML), and write it
 * with {@link Documents#dumps(DocumentContent, Format)}. The core reads,
 * checks and compiles every table, so a table means the same in every
 * language; its errors point into the grid: {@code /header/<c>} or
 * {@code /rows/<r>/<c>}, counting from 0. Tables are immutable and may be
 * shared between threads; a table holds native memory, which the cleaner
 * frees once it is unreachable.
 */
public final class DecisionTable implements DocumentContent {
    private final Handle handle;
    private final TablePolicy hitPolicy;
    private final Aggregate aggregate;
    private final String logic;
    private final boolean strict;
    private final List<Expr> inputs;
    private final List<TableOutput> outputs;
    private final int rowCount;
    private volatile List<TableRow> rows;
    private volatile RuleSet ruleSet;

    private DecisionTable(MemorySegment document) {
        this.handle = Handle.own(this, document, Native::documentFree, "DecisionTable");
        Map<String, Object> table = Read.map(
                with(pointer -> Values.take(
                        Scope.value((out, error) -> Native.documentTableValue(pointer, out, error)), List.of())));
        this.hitPolicy = TablePolicy.fromJsonName((String) table.get("hit_policy"));
        Object combine = table.get("aggregate");
        this.aggregate = combine == null ? null : Aggregate.fromJsonName((String) combine);
        this.logic = (String) table.get("logic");
        this.strict = (Boolean) table.get("strict");
        int count = Math.toIntExact((Long) table.get("input_count"));
        List<Expr> columns = new ArrayList<>(count);
        for (int c = 0; c < count; c++) {
            long column = c;
            columns.add(with(pointer ->
                    Expr.make((out, error, scope) -> Native.documentTableInput(pointer, column, out, error))));
        }
        this.inputs = Collections.unmodifiableList(columns);
        List<TableOutput> made = new ArrayList<>();
        for (Object item : Read.list(table.get("outputs"))) {
            Map<String, Object> output = Read.map(item);
            made.add(new TableOutput((String) output.get("name"), Read.list(output.get("values")).stream()
                    .map(Object.class::cast)
                    .toList()));
        }
        this.outputs = Collections.unmodifiableList(made);
        this.rowCount = Math.toIntExact((Long) table.get("row_count"));
    }

    /** The table of an owned {@code cr_document *} holding one. */
    static DecisionTable fromDocument(MemorySegment owned) {
        return new DecisionTable(owned);
    }

    /**
     * A table of a header and rows, with the defaults: {@link TablePolicy#FIRST},
     * no aggregate, Boolean logic, not strict.
     *
     * @param header the columns: texts, input {@link Expr}s and {@link TableOutput}s
     * @param rows the rows, each the cells in column order
     * @return the table
     * @throws CleanRulesException as {@link Builder#build()}
     */
    public static DecisionTable of(List<?> header, Iterable<? extends Iterable<?>> rows) {
        return builder().header(header).rows(rows).build();
    }

    /**
     * A builder, for a table with a hit policy, an aggregate, a logic, strict mode or limits.
     *
     * @return a new builder
     */
    public static Builder builder() {
        return new Builder();
    }

    /** A table being put together; not for several threads at once. */
    public static final class Builder {
        private final List<Object> header = new ArrayList<>();
        private final List<List<Object>> rows = new ArrayList<>();
        private TablePolicy hitPolicy = TablePolicy.FIRST;
        private Aggregate aggregate;
        private String logic = "boolean";
        private boolean strict;
        private Limits limits;

        private Builder() {}

        /**
         * Add columns, after those added before.
         *
         * @param items texts, input {@link Expr}s and {@link TableOutput}s
         * @return this builder
         */
        public Builder header(Object... items) {
            return header(List.of(items));
        }

        /**
         * Add columns, after those added before.
         *
         * @param items texts, input {@link Expr}s and {@link TableOutput}s
         * @return this builder
         * @throws IllegalArgumentException for an item of another type
         */
        public Builder header(Iterable<?> items) {
            for (Object item : items) {
                if (!(item instanceof String || item instanceof Expr || item instanceof TableOutput)) {
                    throw new IllegalArgumentException(
                            "a header item is a String, an Expr or a TableOutput, not " + describe(item));
                }
                header.add(item);
            }
            return this;
        }

        /**
         * Add a row: its cells in column order.
         *
         * @param cells the cells; {@code null} is an empty cell
         * @return this builder
         */
        public Builder row(Object... cells) {
            return row(Arrays.asList(cells));
        }

        /**
         * Add a row: its cells in column order.
         *
         * @param cells the cells; {@code null} is an empty cell
         * @return this builder
         */
        public Builder row(Iterable<?> cells) {
            Objects.requireNonNull(cells, "cells");
            List<Object> row = new ArrayList<>();
            for (Object cell : cells) {
                row.add(cell);
            }
            rows.add(row);
            return this;
        }

        /**
         * Add rows, after those added before.
         *
         * @param more the rows, each its cells in column order
         * @return this builder
         */
        public Builder rows(Iterable<? extends Iterable<?>> more) {
            Objects.requireNonNull(more, "rows");
            for (Iterable<?> row : more) {
                row(row);
            }
            return this;
        }

        /**
         * The hit policy; {@link TablePolicy#FIRST} by default.
         *
         * @param hitPolicy the hit policy
         * @return this builder
         */
        public Builder hitPolicy(TablePolicy hitPolicy) {
            this.hitPolicy = Objects.requireNonNull(hitPolicy, "hitPolicy");
            return this;
        }

        /**
         * With {@link TablePolicy#COLLECT}, how the outputs combine; null for none.
         *
         * @param aggregate the aggregate, or null
         * @return this builder
         */
        public Builder aggregate(Aggregate aggregate) {
            this.aggregate = aggregate;
            return this;
        }

        /**
         * The logic, by name; {@code boolean} by default.
         *
         * @param logic the logic's name
         * @return this builder
         */
        public Builder logic(String logic) {
            this.logic = Objects.requireNonNull(logic, "logic");
            return this;
        }

        /**
         * Whether evaluation is strict; false by default.
         *
         * @param strict whether evaluation is strict
         * @return this builder
         */
        public Builder strict(boolean strict) {
            this.strict = strict;
            return this;
        }

        /**
         * The limits of reading the table: {@link Limits#maxTextSize()} for
         * each text, {@link Limits#maxDepth()}, {@link Limits#maxTableSize()}.
         *
         * @param limits the limits, or null for the defaults
         * @return this builder
         */
        public Builder limits(Limits limits) {
            this.limits = limits;
            return this;
        }

        /**
         * The table, read, checked and compiled by the core, one row at a time.
         *
         * @return the table
         * @throws FormatException for an empty or repeated column, a row with
         *     more cells than the header, or a name, priority or description
         *     that is not one
         * @throws RuleSyntaxException for a header or cell whose text does not follow the syntax
         * @throws ExpressionException for an input that is a condition, or a
         *     cell that breaks a construction rule (a NaN...)
         * @throws RuleDefinitionException for repeated output keys or rule
         *     names, an output cell that is not one of its column's values, an
         *     aggregate without collect, and the other rules of {@code spec/tables.md}
         * @throws UnsupportedException for an {@link Expr} cell that does not test its column
         * @throws RegistryException for an unknown logic
         */
        public DecisionTable build() {
            Native.ensureLoaded();
            byte[] policy = Text.name(hitPolicy.jsonName());
            byte[] combine = aggregate == null ? null : Text.name(aggregate.jsonName());
            byte[] logicName = logic.equals("boolean") ? null : Text.name(logic);
            MemorySegment grid;
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.pointer();
                scope.check(Native.gridNewOpen(
                        scope.bytes(policy),
                        policy.length,
                        combine == null ? MemorySegment.NULL : scope.bytes(combine),
                        combine == null ? 0 : combine.length,
                        logicName == null ? MemorySegment.NULL : nonNull(scope, logicName),
                        logicName == null ? 0 : logicName.length,
                        strict,
                        scope.limits(limits),
                        out,
                        scope.error()));
                grid = out.get(ADDRESS, 0);
            }
            boolean finished = false;
            try {
                for (int c = 0; c < header.size(); c++) {
                    column(grid, header.get(c), c);
                }
                for (int r = 0; r < rows.size(); r++) {
                    row(grid, rows.get(r), r);
                }
                finished = true;
                return new DecisionTable(Scope.value((out, error) -> Native.gridFinish(grid, out, error)));
            } finally {
                if (!finished) {
                    Native.gridFree(grid);
                }
            }
        }

        /** {@code bytes} as a pointer that is never NULL: an empty name is not a missing one. */
        private static MemorySegment nonNull(Scope scope, byte[] bytes) {
            return bytes.length == 0 ? scope.bytes(new byte[] {0}) : scope.bytes(bytes);
        }

        private static void column(MemorySegment grid, Object item, int c) {
            try (Scope scope = new Scope()) {
                switch (item) {
                    case String text -> {
                        byte[] bytes = Text.name(text);
                        scope.check(Native.gridAddColumnText(grid, scope.bytes(bytes), bytes.length, scope.error()));
                    }
                    case Expr input -> input.with(pointer -> {
                        scope.check(Native.gridAddColumnInput(grid, pointer, scope.error()));
                        return null;
                    });
                    case TableOutput output -> {
                        byte[] name = Text.name(output.name());
                        MemorySegment values = Build.constants(output.values(), "/header/" + c);
                        scope.onClose(() -> Native.valueFree(values));
                        scope.check(Native.gridAddColumnOutput(
                                grid, scope.bytes(name), name.length, values, scope.error()));
                    }
                    default -> throw new IllegalStateException("a header item of another type: " + item);
                }
            }
        }

        private static void row(MemorySegment grid, List<Object> cells, int r) {
            List<Object> values = new ArrayList<>(cells.size());
            List<Expr> nodes = new ArrayList<>(cells.size());
            for (int c = 0; c < cells.size(); c++) {
                Object cell = cells.get(c);
                while (cell instanceof Optional<?> optional) {
                    cell = optional.orElse(null);
                }
                if (cell instanceof Expr node) {
                    values.add(null);
                    nodes.add(node);
                } else {
                    if (!(cell instanceof String)) {
                        Build.check(cell, "/rows/" + r + "/" + c);
                    }
                    values.add(cell);
                    nodes.add(null);
                }
            }
            List<Expr> held = nodes.stream().filter(Objects::nonNull).toList();
            MemorySegment row = Build.data(values);
            try {
                Expr.with(held, pointers -> {
                    MemorySegment[] all = new MemorySegment[nodes.size()];
                    for (int c = 0, next = 0; c < all.length; c++) {
                        all[c] = nodes.get(c) == null ? MemorySegment.NULL : pointers[next++];
                    }
                    try (Scope scope = new Scope()) {
                        scope.check(Native.gridPushRowNodes(grid, row, scope.pointers(all), all.length, scope.error()));
                    }
                    return null;
                });
            } finally {
                Native.valueFree(row);
            }
        }

        private static String describe(Object item) {
            return item == null ? "null" : item.getClass().getName() + " " + item;
        }
    }

    private <T> T with(Function<MemorySegment, T> call) {
        MemorySegment pointer = handle.acquire();
        try {
            return call.apply(pointer);
        } finally {
            handle.release();
        }
    }

    /** The native document, held until {@link #release()}. */
    MemorySegment acquire() {
        return handle.acquire();
    }

    /** End what {@link #acquire()} began. */
    void release() {
        handle.release();
    }

    // ---- What it holds.

    /**
     * The hit policy.
     *
     * @return the hit policy
     */
    public TablePolicy hitPolicy() {
        return hitPolicy;
    }

    /**
     * The {@link TablePolicy#COLLECT} aggregate, if any.
     *
     * @return the aggregate, or empty
     */
    public Optional<Aggregate> aggregate() {
        return Optional.ofNullable(aggregate);
    }

    /**
     * The logic's name.
     *
     * @return the name
     */
    public String logic() {
        return logic;
    }

    /**
     * Whether evaluation is strict.
     *
     * @return whether it is strict
     */
    public boolean isStrict() {
        return strict;
    }

    /**
     * The input expressions, in column order.
     *
     * @return the inputs, unmodifiable
     */
    public List<Expr> inputs() {
        return inputs;
    }

    /**
     * The output columns, in order.
     *
     * @return the outputs, unmodifiable
     */
    public List<TableOutput> outputs() {
        return outputs;
    }

    /**
     * The rows, in order; made when first read.
     *
     * @return the rows, unmodifiable
     */
    public List<TableRow> rows() {
        List<TableRow> made = rows;
        if (made == null) {
            made = with(this::readRows);
            rows = made;
        }
        return made;
    }

    private List<TableRow> readRows(MemorySegment pointer) {
        int columns = inputs.size() + outputs.size();
        List<TableRow> read = new ArrayList<>(rowCount);
        for (int r = 0; r < rowCount; r++) {
            long position = r;
            Map<String, Object> row = Read.map(Values.take(
                    Scope.value((out, error) -> Native.documentTableRow(pointer, position, out, error)), List.of()));
            List<Optional<Expr>> cells = new ArrayList<>(columns);
            for (int c = 0; c < columns; c++) {
                long column = c;
                try (Scope scope = new Scope()) {
                    MemorySegment out = scope.pointer();
                    scope.check(Native.documentTableCell(pointer, position, column, out, scope.error()));
                    MemorySegment node = out.get(ADDRESS, 0);
                    cells.add(node.equals(MemorySegment.NULL) ? Optional.empty() : Optional.of(Expr.wrap(node)));
                }
            }
            read.add(new TableRow(
                    (String) row.get("name"),
                    (Long) row.get("priority"),
                    (String) row.get("description"),
                    cells.subList(0, inputs.size()),
                    cells.subList(inputs.size(), columns)));
        }
        return Collections.unmodifiableList(read);
    }

    /**
     * The rule set the table compiles into: one rule per row ({@code spec/tables.md}, <i>Compiling</i>).
     *
     * @return the rule set
     */
    public RuleSet ruleSet() {
        RuleSet made = ruleSet;
        if (made == null) {
            MemorySegment compiled = with(pointer ->
                    Scope.value((out, error) -> Native.rulesetFromDocument(pointer, out, error)));
            made = RuleSet.fromNative(Bridge.ffi().ruleSet(compiled));
            ruleSet = made;
        }
        return made;
    }

    // ---- Evaluating.

    /**
     * Evaluate one input with the compiled rule set ({@link RuleSet#evaluate(Object)}).
     *
     * @param data the input
     * @return the decision
     * @throws CleanRulesException if the evaluation fails
     */
    public Decision evaluate(Object data) {
        return ruleSet().evaluate(data);
    }

    /**
     * Evaluate one input within limits.
     *
     * @param data the input
     * @param limits the limits, or null for the defaults
     * @return the decision
     * @throws CleanRulesException if the evaluation fails
     */
    public Decision evaluate(Object data, Limits limits) {
        return ruleSet().evaluate(data, limits);
    }

    /**
     * Evaluate a batch with the compiled rule set ({@link RuleSet#evaluateMany(Iterable)}).
     *
     * @param rows the inputs
     * @return the decisions, one per input
     * @throws CleanRulesException if an evaluation fails, with the input's row
     */
    public List<Decision> evaluateMany(Iterable<?> rows) {
        return ruleSet().evaluateMany(rows);
    }

    /**
     * Evaluate a batch within limits.
     *
     * @param rows the inputs
     * @param limits the limits, or null for the defaults
     * @return the decisions, one per input
     * @throws CleanRulesException if an evaluation fails, with the input's row
     */
    public List<Decision> evaluateMany(Iterable<?> rows, Limits limits) {
        return ruleSet().evaluateMany(rows, limits);
    }

    /**
     * The summary of the compiled rule set ({@link RuleSet#summary()}).
     *
     * @return the summary
     */
    public Summary summary() {
        return ruleSet().summary();
    }

    /**
     * The decision table document of the JSON format ({@code spec/tables.md},
     * <i>JSON</i>), as {@link Documents#dumps(DocumentContent)} writes it.
     *
     * @return the JSON text
     */
    public String toJson() {
        return write(Format.JSON);
    }

    /** The table's document in {@code format}. */
    String write(Format format) {
        return with(pointer -> Scope.text(
                (out, len, error) -> Native.documentWrite(pointer, Native.format(format), out, len, error)));
    }

    /**
     * The hit policy and the size, such as {@code DecisionTable(first: 2 rows, 3 columns)}.
     *
     * @return the text
     */
    @Override
    public String toString() {
        return "DecisionTable(" + hitPolicy.jsonName() + ": " + rowCount + " rows, "
                + (inputs.size() + outputs.size()) + " columns)";
    }
}
