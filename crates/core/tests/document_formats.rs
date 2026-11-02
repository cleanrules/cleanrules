//! Reading and writing documents by format, from bytes (`read_document_as`,
//! `write_document_as`).

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use cleanrules::ErrorCode;
use cleanrules::format::{Document, DocumentFormat, read_document_as, write_document_as};
use cleanrules::json::Limits;

const JSON: &str = r#"{"version":1,"rule":{"name":"vip","when":{"op":"gt","left":{"op":"field","path":["years"]},"right":{"op":"const","value":5}}}}"#;
const TEXT: &str = "RULE vip\nIF years > 5";
const CSV: &str = "HIT POLICY,first\nRULE,years,THEN discount\nvip,> 5,0.15\n";

fn read(text: &[u8], format: DocumentFormat) -> cleanrules::Result<Document> {
    read_document_as(text, format, &Limits::default())
}

#[test]
fn every_format_reads_and_writes() {
    let rule = read(JSON.as_bytes(), DocumentFormat::Json).unwrap();
    assert_eq!(
        write_document_as(&rule, DocumentFormat::Json).unwrap(),
        JSON
    );
    assert_eq!(
        write_document_as(&rule, DocumentFormat::Text).unwrap(),
        TEXT
    );
    let from_text = read(TEXT.as_bytes(), DocumentFormat::Text).unwrap();
    assert_eq!(
        write_document_as(&from_text, DocumentFormat::Json).unwrap(),
        JSON
    );
    let table = read(CSV.as_bytes(), DocumentFormat::Csv).unwrap();
    assert!(matches!(table, Document::DecisionTable(_)));
    assert_eq!(write_document_as(&table, DocumentFormat::Csv).unwrap(), CSV);
}

#[cfg(feature = "yaml")]
#[test]
fn yaml_reads_and_writes() {
    let rule = read(JSON.as_bytes(), DocumentFormat::Json).unwrap();
    let yaml = write_document_as(&rule, DocumentFormat::Yaml).unwrap();
    let back = read(yaml.as_bytes(), DocumentFormat::Yaml).unwrap();
    assert_eq!(
        write_document_as(&back, DocumentFormat::Json).unwrap(),
        JSON
    );
}

#[cfg(not(feature = "yaml"))]
#[test]
fn yaml_is_unsupported_without_its_feature() {
    let error = read(b"version: 1\n", DocumentFormat::Yaml).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Unsupported);
    let rule = read(JSON.as_bytes(), DocumentFormat::Json).unwrap();
    let error = write_document_as(&rule, DocumentFormat::Yaml).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Unsupported);
}

#[test]
fn bytes_that_are_not_utf8_are_located_in_every_format() {
    let formats = [
        DocumentFormat::Json,
        #[cfg(feature = "yaml")]
        DocumentFormat::Yaml,
        DocumentFormat::Text,
        DocumentFormat::Csv,
    ];
    for format in formats {
        let error = read(b"RULE a\nIF \xff\n", format).unwrap_err();
        assert_eq!(error.code(), ErrorCode::FormatError, "{format:?}");
        let context = error.context();
        assert_eq!(
            (context.line, context.column),
            (Some(2), Some(4)),
            "{format:?}"
        );
    }
}

#[test]
fn only_a_decision_table_is_written_as_csv() {
    let rule = read(JSON.as_bytes(), DocumentFormat::Json).unwrap();
    let error = write_document_as(&rule, DocumentFormat::Csv).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Unsupported);
    assert_eq!(error.context().operation.as_deref(), Some("csv"));
}

#[test]
fn formats_have_names() {
    for format in [
        DocumentFormat::Json,
        DocumentFormat::Yaml,
        DocumentFormat::Text,
        DocumentFormat::Csv,
    ] {
        assert_eq!(DocumentFormat::from_name(format.as_str()), Some(format));
    }
    assert_eq!(DocumentFormat::from_name("xml"), None);
}

#[test]
fn one_size_limit_for_every_text() {
    let limits = Limits::default().with_max_size(10);
    assert_eq!(
        (limits.max_size, limits.max_yaml_size, limits.max_text_size),
        (10, 10, 10)
    );
    let error = read_document_as(TEXT.as_bytes(), DocumentFormat::Text, &limits).unwrap_err();
    assert_eq!(error.code(), ErrorCode::LimitExceeded);
}

#[test]
fn every_document_but_a_condition_gives_a_rule_set() {
    let one = read(TEXT.as_bytes(), DocumentFormat::Text).unwrap();
    assert!(matches!(one, Document::Rule(_)));
    let ruleset = one.into_ruleset().unwrap();
    assert_eq!(ruleset.rules().len(), 1);
    assert_eq!(ruleset.hit_policy().as_str(), "first");
    assert_eq!(ruleset.logic().as_str(), "boolean");
    assert!(!ruleset.strict());
    let table = read(CSV.as_bytes(), DocumentFormat::Csv).unwrap();
    assert_eq!(table.into_ruleset().unwrap().rules()[0].name(), "vip");
    let set = read(
        b"HIT POLICY priority\nRULE a\nIF x > 1",
        DocumentFormat::Text,
    )
    .unwrap();
    assert_eq!(
        set.into_ruleset().unwrap().hit_policy().as_str(),
        "priority"
    );
    let condition = read(b"x > 1", DocumentFormat::Text).unwrap();
    let error = condition.into_ruleset().unwrap_err();
    assert_eq!(error.code(), ErrorCode::FormatError);
    assert!(error.hint().is_some());
}

#[test]
fn outputs_are_written_by_format() {
    use cleanrules::explain::{explain, summary};
    use cleanrules::format::{ValueContext, read_value};
    use cleanrules::format::{
        read_decision_document_as, write_decision_as, write_explanation_as, write_summary_as,
    };
    use cleanrules::json::parse;

    let ruleset = read(TEXT.as_bytes(), DocumentFormat::Text)
        .unwrap()
        .into_ruleset()
        .unwrap();
    let input = read_value(
        &parse(r#"{"years": 7}"#, &Limits::default()).unwrap(),
        "",
        ValueContext::Data,
    )
    .unwrap();
    let decision = ruleset.evaluate(&input).unwrap();
    let json = write_decision_as(&decision, DocumentFormat::Json).unwrap();
    assert!(json.starts_with(r#"{"version":1,"decision":{"#), "{json}");
    let read = read_decision_document_as(
        json.as_bytes(),
        DocumentFormat::Json,
        &ruleset,
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(
        write_decision_as(&read, DocumentFormat::Json).unwrap(),
        json
    );
    for format in [DocumentFormat::Text, DocumentFormat::Csv] {
        let error = write_decision_as(&decision, format).unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
        let error =
            read_decision_document_as(json.as_bytes(), format, &ruleset, &Limits::default())
                .unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
    }
    let explanation = explain(&decision);
    let text = write_explanation_as(&explanation, DocumentFormat::Text).unwrap();
    assert_eq!(text, explanation.to_string());
    assert!(
        write_explanation_as(&explanation, DocumentFormat::Json)
            .unwrap()
            .starts_with(r#"{"version":1,"explanation":{"#)
    );
    let summary = summary(&ruleset);
    assert_eq!(
        write_summary_as(&summary, DocumentFormat::Text).unwrap(),
        summary.to_string()
    );
    assert!(
        write_summary_as(&summary, DocumentFormat::Json)
            .unwrap()
            .starts_with(r#"{"version":1,"summary":{"#)
    );
    let error = write_summary_as(&summary, DocumentFormat::Csv).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Unsupported);
    #[cfg(feature = "yaml")]
    {
        let yaml = write_decision_as(&decision, DocumentFormat::Yaml).unwrap();
        let read = read_decision_document_as(
            yaml.as_bytes(),
            DocumentFormat::Yaml,
            &ruleset,
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(
            write_decision_as(&read, DocumentFormat::Json).unwrap(),
            json
        );
    }
}
