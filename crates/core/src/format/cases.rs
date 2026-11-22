//! Case files in JSON (`spec/tests.md`).

use std::collections::BTreeSet;

use crate::cases::{
    Case, Cases, Expectation, ExpectationParts, ExpectedOutput, alone, check_error_code, check_id,
    empty_rule_id, repeated_id, repeated_rule_id, unexpected_code,
};
use crate::error::{ErrorCode, Result};
use crate::format::rules::{at, format_error, get, no_unknown_keys};
use crate::format::value::{ValueContext, read_value, write_value};
use crate::json::{Json, by_key};
use crate::value::Map;

const FILE_KEYS: [&str; 3] = ["description", "metadata", "items"];
const CASE_KEYS: [&str; 4] = ["id", "description", "input", "expect"];
const EXPECT_KEYS: [&str; 5] = ["selected", "output", "outputs", "unknown", "error"];
const ERROR_KEYS: [&str; 1] = ["code"];

/// Read a case file's content (the object under `"cases"`), in pre-order
/// (`spec/tests.md`, *Reading and writing*).
pub(crate) fn read_cases(json: &Json, pointer: &str) -> Result<Cases> {
    let Json::Object(members) = json else {
        return Err(format_error(
            pointer,
            "\"cases\" must be an object with the cases under \"items\"",
        ));
    };
    let description = match get(members, "description") {
        None => None,
        Some(Json::String(text)) => Some(text.clone()),
        Some(_) => {
            return Err(format_error(
                &at(pointer, "description"),
                "\"description\" must be a string",
            ));
        }
    };
    let metadata = match get(members, "metadata") {
        None => Map::new(),
        Some(Json::Object(entries)) => {
            let base = at(pointer, "metadata");
            let mut map = Map::new();
            // Metadata keys are taken literally; their values are data values.
            for (key, value) in by_key(entries) {
                map.insert(
                    key.clone(),
                    read_value(value, &at(&base, key), ValueContext::Data)?,
                );
            }
            map
        }
        Some(_) => {
            return Err(format_error(
                &at(pointer, "metadata"),
                "\"metadata\" must be an object",
            ));
        }
    };
    let items_at = at(pointer, "items");
    let items = match get(members, "items") {
        Some(Json::Array(items)) => items,
        Some(_) => {
            return Err(format_error(
                &items_at,
                "\"items\" must be an array of cases",
            ));
        }
        None => return Err(format_error(&items_at, "the key \"items\" is missing")),
    };
    let cases = read_items(items, &items_at)?;
    no_unknown_keys(members, &FILE_KEYS, pointer)?;
    Ok(Cases::read(description, metadata, cases))
}

fn read_items(items: &[Json], pointer: &str) -> Result<Vec<Case>> {
    let mut cases: Vec<Case> = Vec::with_capacity(items.len());
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        let here = format!("{pointer}/{i}");
        let Json::Object(members) = item else {
            return Err(format_error(&here, "a case must be an object"));
        };
        let id_at = at(&here, "id");
        let id = match get(members, "id") {
            Some(Json::String(id)) => id,
            Some(_) => return Err(format_error(&id_at, "\"id\" must be a string")),
            None => return Err(format_error(&id_at, "the key \"id\" is missing")),
        };
        check_id(id).map_err(|e| e.at_pointer(id_at.clone()))?;
        if !ids.insert(id) {
            return Err(repeated_id(id).at_pointer(id_at));
        }
        let description = match get(members, "description") {
            None => None,
            Some(Json::String(text)) => Some(text.clone()),
            Some(_) => {
                return Err(format_error(
                    &at(&here, "description"),
                    "\"description\" must be a string",
                ));
            }
        };
        let input_at = at(&here, "input");
        let input = match get(members, "input") {
            Some(input) => read_value(input, &input_at, ValueContext::Data)?,
            None => return Err(format_error(&input_at, "the key \"input\" is missing")),
        };
        let expect = match get(members, "expect") {
            None => Expectation::new(),
            Some(Json::Object(entries)) => read_expectation(entries, &at(&here, "expect"))?,
            Some(_) => {
                return Err(format_error(
                    &at(&here, "expect"),
                    "\"expect\" must be an object",
                ));
            }
        };
        no_unknown_keys(members, &CASE_KEYS, &here)?;
        cases.push(Case::read(id.clone(), description, input, expect));
    }
    Ok(cases)
}

fn read_expectation(members: &[(String, Json)], pointer: &str) -> Result<Expectation> {
    let selected = rule_ids(
        get(members, "selected"),
        &at(pointer, "selected"),
        "selected",
    )?;
    let output_at = at(pointer, "output");
    let output = match get(members, "output") {
        None => None,
        Some(Json::Null) => Some(ExpectedOutput::NothingSelected),
        Some(Json::Object(entries)) => {
            Some(ExpectedOutput::Includes(read_output(entries, &output_at)?))
        }
        Some(_) => {
            return Err(format_error(
                &output_at,
                "\"output\" must be an object of payload keys, or null",
            ));
        }
    };
    let list_at = at(pointer, "outputs");
    let outputs = match get(members, "outputs") {
        None => None,
        Some(Json::Array(items)) => {
            let mut outputs = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let here = format!("{list_at}/{i}");
                let Json::Object(entries) = item else {
                    return Err(format_error(
                        &here,
                        "an output must be an object of payload keys",
                    ));
                };
                outputs.push(read_output(entries, &here)?);
            }
            Some(outputs)
        }
        Some(_) => {
            return Err(format_error(
                &list_at,
                "\"outputs\" must be an array of outputs",
            ));
        }
    };
    let unknown = rule_ids(get(members, "unknown"), &at(pointer, "unknown"), "unknown")?
        .map(|rules| rules.into_iter().collect());
    let error_at = at(pointer, "error");
    let error = match get(members, "error") {
        None => None,
        Some(Json::Object(entries)) => Some(read_error(entries, &error_at)?),
        Some(_) => {
            return Err(format_error(
                &error_at,
                "\"error\" must be an object with the error's \"code\"",
            ));
        }
    };
    no_unknown_keys(members, &EXPECT_KEYS, pointer)?;
    let parts = ExpectationParts {
        selected,
        output,
        outputs,
        unknown,
        error,
    };
    let others = parts.selected.is_some()
        || parts.output.is_some()
        || parts.outputs.is_some()
        || parts.unknown.is_some();
    if parts.error.is_some() && others {
        return Err(alone().at_pointer(error_at));
    }
    Ok(Expectation::read(parts))
}

/// An array of rules by effective id: strings, not empty, each named once.
fn rule_ids(json: Option<&Json>, pointer: &str, key: &str) -> Result<Option<Vec<String>>> {
    let items = match json {
        None => return Ok(None),
        Some(Json::Array(items)) => items,
        Some(_) => {
            return Err(format_error(
                pointer,
                format!("{key:?} must be an array of rule ids"),
            ));
        }
    };
    let mut rules = Vec::with_capacity(items.len());
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        let here = format!("{pointer}/{i}");
        let Json::String(id) = item else {
            return Err(format_error(&here, "a rule is named by its id, a string"));
        };
        if id.is_empty() {
            return Err(empty_rule_id(key).at_pointer(here));
        }
        if !seen.insert(id) {
            return Err(repeated_rule_id(id, key).at_pointer(here));
        }
        rules.push(id.clone());
    }
    Ok(Some(rules))
}

/// An expected output: payload keys, taken literally, by code point, mapped
/// to data values, which are never out of range.
fn read_output(entries: &[(String, Json)], pointer: &str) -> Result<Map> {
    let mut map = Map::new();
    for (key, value) in by_key(entries) {
        map.insert(
            key.clone(),
            read_value(value, &at(pointer, key), ValueContext::Expected)?,
        );
    }
    Ok(map)
}

fn read_error(members: &[(String, Json)], pointer: &str) -> Result<ErrorCode> {
    let code_at = at(pointer, "code");
    let code = match get(members, "code") {
        Some(Json::String(name)) => ErrorCode::ALL
            .into_iter()
            .find(|code| code.as_str() == name)
            .filter(|code| check_error_code(*code).is_ok())
            .ok_or_else(|| unexpected_code(name).at_pointer(code_at.clone()))?,
        Some(_) => return Err(format_error(&code_at, "\"code\" must be a string")),
        None => return Err(format_error(&code_at, "the key \"code\" is missing")),
    };
    no_unknown_keys(members, &ERROR_KEYS, pointer)?;
    Ok(code)
}

fn write_output(map: &Map) -> Json {
    Json::Object(
        map.iter()
            .map(|(key, value)| (key.to_owned(), write_value(value)))
            .collect(),
    )
}

fn strings<'a>(items: impl IntoIterator<Item = &'a String>) -> Json {
    Json::Array(items.into_iter().cloned().map(Json::String).collect())
}

fn write_expectation(expect: &Expectation) -> Json {
    let mut members: Vec<(String, Json)> = Vec::new();
    if let Some(selected) = expect.selected() {
        members.push(("selected".into(), strings(selected)));
    }
    match expect.output() {
        None => {}
        Some(ExpectedOutput::NothingSelected) => members.push(("output".into(), Json::Null)),
        Some(ExpectedOutput::Includes(map)) => members.push(("output".into(), write_output(map))),
    }
    if let Some(outputs) = expect.outputs() {
        members.push((
            "outputs".into(),
            Json::Array(outputs.iter().map(write_output).collect()),
        ));
    }
    if let Some(unknown) = expect.unknown() {
        members.push(("unknown".into(), strings(unknown)));
    }
    if let Some(code) = expect.error() {
        members.push((
            "error".into(),
            Json::Object(vec![("code".into(), Json::String(code.as_str().into()))]),
        ));
    }
    Json::Object(members)
}

/// Write a case file's content, keys in the order of `spec/tests.md`.
#[must_use]
pub fn write_cases(cases: &Cases) -> Json {
    let mut file: Vec<(String, Json)> = Vec::new();
    if let Some(description) = cases.description() {
        file.push(("description".into(), Json::String(description.into())));
    }
    if !cases.metadata().is_empty() {
        file.push(("metadata".into(), write_output(cases.metadata())));
    }
    let items = cases
        .iter()
        .map(|case| {
            let mut members: Vec<(String, Json)> =
                vec![("id".into(), Json::String(case.id().into()))];
            if let Some(description) = case.description() {
                members.push(("description".into(), Json::String(description.into())));
            }
            members.push(("input".into(), write_value(case.input())));
            if !case.expect().is_empty() {
                members.push(("expect".into(), write_expectation(case.expect())));
            }
            Json::Object(members)
        })
        .collect();
    file.push(("items".into(), Json::Array(items)));
    Json::Object(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::Case;
    use crate::format::{Document, DocumentFormat, read_document, write_document_as};
    use crate::json::Limits;
    use crate::value::Value;
    use crate::visual::{DiagramFormat, draw_document};

    const FILE: &str = r#"{"version": 1, "cases": {"metadata": {"owner": "risk"}, "items": [
        {"id": "a", "description": "d", "input": {"x": 1},
         "expect": {"selected": ["r"], "output": {"k": {"$decimal": "1.50"}},
                    "outputs": [{"k": {"$decimal": "1.50"}}], "unknown": ["z", "y"]}},
        {"id": "b", "input": {}, "expect": {"output": null}},
        {"id": "c", "input": {}, "expect": {"error": {"code": "hit_policy_error"}}},
        {"id": "d", "input": 1}
    ], "description": "f"}}"#;

    fn read(text: &str) -> Cases {
        match read_document(text, &Limits::default()).unwrap() {
            Document::Cases(cases) => cases,
            other => panic!("not a case file: {other:?}"),
        }
    }

    #[test]
    fn a_case_file_reads_and_writes_back() {
        let cases = read(FILE);
        assert_eq!(cases.len(), 4);
        assert_eq!(cases.description(), Some("f"));
        assert_eq!(cases.metadata().len(), 1);
        let a = cases.get("a").unwrap();
        assert_eq!(a.description(), Some("d"));
        assert_eq!(a.expect().selected(), Some(&["r".to_owned()][..]));
        assert_eq!(
            a.expect().unknown().unwrap().iter().collect::<Vec<_>>(),
            ["y", "z"]
        );
        assert!(matches!(
            cases.get("b").unwrap().expect().output(),
            Some(ExpectedOutput::NothingSelected)
        ));
        assert_eq!(
            cases.get("c").unwrap().expect().error(),
            Some(ErrorCode::HitPolicyError)
        );
        assert!(cases.get("d").unwrap().expect().is_empty());
        let json = write_document_as(&cases.clone().into_document(), DocumentFormat::Json).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"version":1,"cases":{"description":"f","metadata":{"owner":"risk"},"#,
                r#""items":[{"id":"a","description":"d","input":{"x":1},"#,
                r#""expect":{"selected":["r"],"output":{"k":{"$decimal":"1.50"}},"#,
                r#""outputs":[{"k":{"$decimal":"1.50"}}],"unknown":["y","z"]}},"#,
                r#"{"id":"b","input":{},"expect":{"output":null}},"#,
                r#"{"id":"c","input":{},"expect":{"error":{"code":"hit_policy_error"}}},"#,
                r#"{"id":"d","input":1}]}}"#
            )
        );
        let again = read(&json);
        assert_eq!(
            write_document_as(&again.into_document(), DocumentFormat::Json).unwrap(),
            json
        );
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn a_case_file_reads_and_writes_in_yaml() {
        use crate::format::read_document_as;
        let document = read(FILE).into_document();
        let yaml = write_document_as(&document, DocumentFormat::Yaml).unwrap();
        assert!(yaml.starts_with("version: 1\ncases:\n"), "{yaml}");
        let again =
            read_document_as(yaml.as_bytes(), DocumentFormat::Yaml, &Limits::default()).unwrap();
        assert_eq!(again.canonical_bytes(), document.canonical_bytes());
    }

    #[test]
    fn a_case_file_is_built_in_code_as_it_is_read() {
        let mut input = Map::new();
        input.insert("x", Value::Int(1));
        let built = Cases::new([Case::new("d", Value::Int(1)).unwrap()]).unwrap();
        let read_back = read(r#"{"version": 1, "cases": {"items": [{"id": "d", "input": 1}]}}"#);
        assert_eq!(
            built.into_document().canonical_bytes(),
            read_back.into_document().canonical_bytes()
        );
        assert!(matches!(
            Case::new("x", Value::Map(input)).unwrap().input(),
            Value::Map(map) if map.len() == 1
        ));
    }

    #[test]
    fn a_case_file_gives_no_rules_no_text_and_no_diagram() {
        let document = read(FILE).into_document();
        assert_eq!(
            write_document_as(&document, DocumentFormat::Text)
                .unwrap_err()
                .code(),
            ErrorCode::Unsupported
        );
        assert_eq!(
            write_document_as(&document, DocumentFormat::Csv)
                .unwrap_err()
                .code(),
            ErrorCode::Unsupported
        );
        assert_eq!(
            draw_document(&document, None, DiagramFormat::Dot, &Limits::default())
                .unwrap_err()
                .code(),
            ErrorCode::Unsupported
        );
        assert!(document.digest().to_string().starts_with("sha256:"));
        assert_eq!(
            document.into_ruleset().unwrap_err().code(),
            ErrorCode::FormatError
        );
    }

    #[test]
    fn errors_point_into_the_file() {
        let pointer = |text: &str| {
            read_document(text, &Limits::default())
                .unwrap_err()
                .context()
                .pointer
                .clone()
        };
        let case = |body: &str| format!(r#"{{"version": 1, "cases": {{"items": [{body}]}}}}"#);
        assert_eq!(
            pointer(&case(
                r#"{"id": "a", "input": {}, "expect": {"unknown": [""]}}"#
            )),
            Some("/cases/items/0/expect/unknown/0".to_owned())
        );
        assert_eq!(
            pointer(&case(
                r#"{"id": "a", "input": {}, "expect": {"error": {"code": 1}}}"#
            )),
            Some("/cases/items/0/expect/error/code".to_owned())
        );
        assert_eq!(
            pointer(&case(
                r#"{"id": "a", "input": {}, "expect": {"outputs": [[]]}}"#
            )),
            Some("/cases/items/0/expect/outputs/0".to_owned())
        );
        assert_eq!(
            pointer(&case(
                r#"{"id": "a", "input": {}, "expect": {"unknown": 1}}"#
            )),
            Some("/cases/items/0/expect/unknown".to_owned())
        );
    }
}
