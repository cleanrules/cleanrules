package io.github.cleanrules;

/** A syntax that documents are read from and written in. */
public enum Format {
    /** JSON ({@code spec/json-format.md}). */
    JSON,
    /** YAML, the JSON subset of YAML 1.2 ({@code spec/json-format.md}, <i>YAML</i>). */
    YAML,
    /** The rule syntax ({@code spec/text.md}); for explanations and summaries, their text. */
    TEXT,
    /** CSV, for decision tables ({@code spec/tables.md}). */
    CSV
}
