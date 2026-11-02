//! Reading documents against the `read` conformance vectors: conditions, rules,
//! rule sets, and decisions read with their rule set.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use cleanrules::Error;
use cleanrules::format::{
    Document, read_decision_document, read_document, read_ruleset, write_decision, write_document,
    write_ruleset,
};
use cleanrules::json::{Json, JsonNumber, Limits, to_string};
use cleanrules::table::DecisionTable;
use common::{get, int, limits_of, same, text, vectors};

const FILES: [&str; 10] = [
    "format/documents.json",
    "format/values.json",
    "format/conditions.json",
    "format/limits.json",
    "format/rules.json",
    "format/rulesets.json",
    "format/decisions.json",
    "format/yaml.json",
    "format/text.json",
    "format/tables.json",
];

/// A decision as the document it is written in.
fn decision_document(decision: &cleanrules::rules::Decision<'_>) -> Json {
    Json::Object(vec![
        (
            "version".into(),
            Json::Number(JsonNumber::from_literal("1")),
        ),
        ("decision".into(), write_decision(decision)),
    ])
}

/// What a vector reads: the document, or its text when it is written as text
/// (`expect.printed`), or a decision table's forms.
enum Read {
    Document(Json),
    Text(String),
    Table(Table),
}

/// A decision table as a document, as CSV and as the rule set it compiles into.
struct Table {
    document: Json,
    csv: String,
    compiled: Json,
}

fn table_forms(table: &DecisionTable) -> Table {
    Table {
        document: write_document(&Document::DecisionTable(table.clone())),
        csv: table.to_csv(),
        compiled: write_ruleset(table.ruleset()),
    }
}

/// Read the vector's document; write it as text when the vector asks.
fn read_or_print(given: &Json, expect: &Json, source: &str) -> Result<Read, Error> {
    if let Some(csv) = text(get(given, "table_csv")) {
        return DecisionTable::from_csv(csv, &limits_of(given))
            .map(|t| Read::Table(table_forms(&t)));
    }
    if text(get(given, "write")) == Some("csv")
        || get(expect, "csv").is_some()
        || get(expect, "compiled").is_some()
    {
        return match read_document(source, &limits_of(given))? {
            Document::DecisionTable(table) => Ok(Read::Table(table_forms(&table))),
            other => Ok(Read::Document(write_document(&other))),
        };
    }
    let document = if let Some(rules_text) = text(get(given, "rules_text")) {
        cleanrules::text::parse(rules_text, &limits_of(given))?
    } else if text(get(given, "write")) == Some("text") {
        read_document(source, &limits_of(given))?
    } else {
        return read(given, source).map(Read::Document);
    };
    let written = write_document(&document);
    if get(expect, "document").is_some_and(|wanted| !same(&written, wanted))
        || (get(expect, "printed").is_none() && get(given, "write").is_none())
    {
        return Ok(Read::Document(written));
    }
    cleanrules::text::to_string(&document).map(Read::Text)
}

/// Read the vector's document, as a decision when a rule set is given.
fn read(given: &Json, source: &str) -> Result<Json, Error> {
    #[cfg(feature = "yaml")]
    if get(given, "yaml").is_some() {
        if let Some(ruleset_json) = get(given, "ruleset") {
            let ruleset = read_ruleset(ruleset_json, "/ruleset", &Limits::default()).unwrap();
            return cleanrules::format::read_yaml_decision_document(
                source,
                &ruleset,
                &limits_of(given),
            )
            .map(|decision| decision_document(&decision));
        }
        return cleanrules::format::read_yaml_document(source, &limits_of(given))
            .map(|d| write_document(&d));
    }
    match get(given, "ruleset") {
        Some(ruleset_json) => {
            let ruleset = read_ruleset(ruleset_json, "/ruleset", &Limits::default()).unwrap();
            read_decision_document(source, &ruleset, &limits_of(given))
                .map(|decision| decision_document(&decision))
        }
        None => read_document(source, &limits_of(given)).map(|d| write_document(&d)),
    }
}

fn check(vector: &Json) -> Option<String> {
    let given = get(vector, "given").unwrap();
    if get(given, "yaml").is_some() && !cfg!(feature = "yaml") {
        return None; // YAML vectors need the `yaml` feature.
    }
    let source = match (
        text(get(given, "text"))
            .or(text(get(given, "yaml")))
            .or(text(get(given, "rules_text")))
            .or(text(get(given, "table_csv"))),
        get(given, "document"),
    ) {
        (Some(source), _) => source.to_owned(),
        (None, Some(document)) => to_string(document),
        _ => panic!("nothing given"),
    };
    let expect = get(vector, "expect").unwrap();
    match (read_or_print(given, expect, &source), get(expect, "error")) {
        (Ok(Read::Text(printed)), None) => {
            let wanted = text(get(expect, "printed")).unwrap();
            (printed != wanted).then(|| format!("printed {printed:?}, expected {wanted:?}"))
        }
        (Ok(Read::Table(table)), None) => {
            let mut problems = Vec::new();
            if let Some(wanted) = get(expect, "document")
                && !same(&table.document, wanted)
            {
                problems.push(format!(
                    "wrote {}, expected {}",
                    to_string(&table.document),
                    to_string(wanted)
                ));
            }
            if let Some(wanted) = text(get(expect, "csv"))
                && table.csv != wanted
            {
                problems.push(format!("wrote CSV {:?}, expected {wanted:?}", table.csv));
            }
            if let Some(wanted) = get(expect, "compiled")
                && !same(&table.compiled, wanted)
            {
                problems.push(format!(
                    "compiled {}, expected {}",
                    to_string(&table.compiled),
                    to_string(wanted)
                ));
            }
            (!problems.is_empty()).then(|| problems.join("; "))
        }
        (Ok(Read::Table(table)), Some(error)) => Some(format!(
            "read {}, expected {}",
            to_string(&table.document),
            to_string(error)
        )),
        (Ok(Read::Text(printed)), Some(error)) => Some(format!(
            "printed {printed:?}, expected {}",
            to_string(error)
        )),
        (Ok(Read::Document(written)), None) => {
            let wanted = get(expect, "document").unwrap();
            (!same(&written, wanted)).then(|| {
                format!(
                    "wrote {}, expected {}",
                    to_string(&written),
                    to_string(wanted)
                )
            })
        }
        (Ok(Read::Document(written)), Some(error)) => Some(format!(
            "read {}, expected {}",
            to_string(&written),
            to_string(error)
        )),
        (Err(error), None) => Some(format!(
            "{} at {:?}: {error}",
            error.code(),
            error.context()
        )),
        (Err(error), Some(wanted)) => {
            let context = error.context();
            let ok = error.code().as_str() == text(get(wanted, "code")).unwrap()
                && (get(wanted, "pointer").is_none()
                    || context.pointer.as_deref() == text(get(wanted, "pointer")))
                && (get(wanted, "line").is_none() || context.line == int(get(wanted, "line")))
                && (get(wanted, "column").is_none()
                    || context.column == int(get(wanted, "column")));
            (!ok).then(|| {
                format!(
                    "got {} pointer={:?} line={:?} column={:?} ({error}), expected {}",
                    error.code(),
                    context.pointer,
                    context.line,
                    context.column,
                    to_string(wanted)
                )
            })
        }
    }
}

#[test]
fn read_vectors() {
    let (mut run, mut failures) = (0, Vec::new());
    for file in FILES {
        let all = vectors(file);
        let Json::Array(list) = &all else { panic!() };
        for vector in list {
            run += 1;
            if let Some(problem) = check(vector) {
                failures.push(format!("{}: {problem}", text(get(vector, "id")).unwrap()));
            }
        }
    }
    println!("{run} read vectors run");
    assert!(run >= 187, "only {run} read vectors");
    assert!(
        failures.is_empty(),
        "{} of {run} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
