//! Bundles in JSON (`spec/bundles.md`).

use crate::bundle::{
    Bundle, BundleRules, CaseFile, Contract, Engine, EngineLimits, LIMIT_NAMES, MAX_RECORDED_LIMIT,
    check_case_path, check_engine, check_id, check_revision,
};
use crate::digest::Digest;
use crate::error::{Error, ErrorCode, Result};
use crate::format::rules::{
    RawRuleSet, at, build_ruleset, format_error, get, no_unknown_keys, shape_ruleset, write_ruleset,
};
use crate::format::table::read_table;
use crate::format::write_table;
use crate::json::{Json, JsonNumber, Limits, by_key, pointer_token, same};

const BUNDLE_KEYS: [&str; 8] = [
    "id",
    "revision",
    "ruleset",
    "decision_table",
    "ruleset_digest",
    "engine",
    "contract",
    "cases",
];
const ENGINE_KEYS: [&str; 3] = ["name", "version", "limits"];
const CONTRACT_KEYS: [&str; 2] = ["input", "output"];
const OUTPUT_KEYS: [&str; 1] = ["keys"];
const CASE_KEYS: [&str; 2] = ["path", "digest"];

fn mismatch(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::DecisionMismatch, message).at_pointer(pointer)
}

fn object<'j>(json: Option<&'j Json>, pointer: &str, what: &str) -> Result<&'j [(String, Json)]> {
    match json {
        Some(Json::Object(members)) => Ok(members),
        Some(_) => Err(format_error(pointer, format!("{what} must be an object"))),
        None => Err(format_error(pointer, format!("the key {what} is missing"))),
    }
}

fn string<'j>(json: Option<&'j Json>, pointer: &str, what: &str) -> Result<&'j str> {
    match json {
        Some(Json::String(text)) => Ok(text),
        Some(_) => Err(format_error(pointer, format!("{what} must be a string"))),
        None => Err(format_error(pointer, format!("the key {what} is missing"))),
    }
}

fn digest(json: Option<&Json>, pointer: &str, what: &str) -> Result<Digest> {
    string(json, pointer, what)?
        .parse()
        .map_err(|e: Error| e.at_pointer(pointer))
}

/// The rules of a bundle, read to their shape: a rule set is built after the
/// rest of the bundle's shape (`spec/json-format.md`, *Errors and limits*); a
/// decision table is read where it stands (`spec/tables.md`).
enum RawRules {
    RuleSet(RawRuleSet),
    DecisionTable(crate::table::DecisionTable),
}

/// Read a bundle's content (the object under `"bundle"`); `checked_top`
/// checks the document's top level after the content's shape.
pub(crate) fn read_bundle(
    json: &Json,
    pointer: &str,
    limits: &Limits,
    check_top: impl FnOnce() -> Result<()>,
) -> Result<Bundle> {
    let Json::Object(members) = json else {
        return Err(format_error(pointer, "a bundle must be an object"));
    };
    let id = string(get(members, "id"), &at(pointer, "id"), "\"id\"")?;
    check_id(id).map_err(|e| e.at_pointer(at(pointer, "id")))?;
    let revision = string(
        get(members, "revision"),
        &at(pointer, "revision"),
        "\"revision\"",
    )?;
    check_revision(revision).map_err(|e| e.at_pointer(at(pointer, "revision")))?;
    let raw = match (get(members, "ruleset"), get(members, "decision_table")) {
        (Some(ruleset), None) => {
            RawRules::RuleSet(shape_ruleset(ruleset, &at(pointer, "ruleset"), limits)?)
        }
        (None, Some(table)) => {
            RawRules::DecisionTable(read_table(table, &at(pointer, "decision_table"), limits)?)
        }
        _ => {
            return Err(format_error(
                pointer,
                "a bundle holds exactly one of \"ruleset\" and \"decision_table\"",
            )
            .with_hint("give the rules as a rule set or as a decision table"));
        }
    };
    let digest_at = at(pointer, "ruleset_digest");
    let stated = digest(
        get(members, "ruleset_digest"),
        &digest_at,
        "\"ruleset_digest\"",
    )?;
    let engine = read_engine(get(members, "engine"), &at(pointer, "engine"))?;
    let contract_at = at(pointer, "contract");
    let contract = read_contract(get(members, "contract"), &contract_at)?;
    let cases = read_cases(get(members, "cases"), &at(pointer, "cases"))?;
    no_unknown_keys(members, &BUNDLE_KEYS, pointer)?;
    check_top()?;
    let rules = match raw {
        RawRules::RuleSet(raw) => BundleRules::RuleSet(build_ruleset(raw)?),
        RawRules::DecisionTable(table) => BundleRules::DecisionTable(table),
    };
    let actual = rules.ruleset().digest();
    if stated != actual {
        return Err(mismatch(
            &digest_at,
            format!("the bundle states the rule set digest {stated}, but its rules have {actual}"),
        )
        .with_hint("the rules or the digest were edited: make the bundle again"));
    }
    if engine.is_current() {
        let derived = Contract::derive(rules.ruleset());
        if !same(derived.input(), contract.input()) {
            return Err(mismatch(
                &at(&contract_at, "input"),
                "the input schema is not the one this engine derives from the rules",
            )
            .with_hint("the contract was edited or is stale: make the bundle again"));
        }
        if derived.output_keys() != contract.output_keys() {
            return Err(mismatch(
                &at(&contract_at, "output"),
                "the output keys are not those of the rules",
            )
            .with_hint("the contract was edited or is stale: make the bundle again"));
        }
    }
    Ok(Bundle::read(
        id.to_owned(),
        revision.to_owned(),
        rules,
        engine,
        contract,
        cases,
    ))
}

fn read_engine(json: Option<&Json>, pointer: &str) -> Result<Engine> {
    let members = object(json, pointer, "\"engine\"")?;
    let name = string(get(members, "name"), &at(pointer, "name"), "\"name\"")?;
    let version = string(
        get(members, "version"),
        &at(pointer, "version"),
        "\"version\"",
    )?;
    let (name_ok, version_ok) = check_engine(name, version);
    name_ok.map_err(|e| e.at_pointer(at(pointer, "name")))?;
    version_ok.map_err(|e| e.at_pointer(at(pointer, "version")))?;
    let limits_at = at(pointer, "limits");
    let limits = object(get(members, "limits"), &limits_at, "\"limits\"")?;
    let mut values = [0u64; 6];
    for (value, name) in values.iter_mut().zip(LIMIT_NAMES) {
        let here = at(&limits_at, name);
        *value = match get(limits, name) {
            Some(Json::Number(n)) => n
                .as_i128()
                .and_then(|n| u64::try_from(n).ok())
                .filter(|n| (1..=MAX_RECORDED_LIMIT).contains(n))
                .ok_or_else(|| {
                    format_error(
                        &here,
                        format!("{name:?} must be an integer from 1 to 2^53 - 1"),
                    )
                })?,
            Some(_) => {
                return Err(format_error(&here, format!("{name:?} must be an integer")));
            }
            None => {
                return Err(format_error(&here, format!("the key {name:?} is missing")));
            }
        };
    }
    no_unknown_keys(limits, &LIMIT_NAMES, &limits_at)?;
    no_unknown_keys(members, &ENGINE_KEYS, pointer)?;
    Engine::new(name, version, EngineLimits::from_values(values)).map_err(|e| e.at_pointer(pointer))
}

fn read_contract(json: Option<&Json>, pointer: &str) -> Result<Contract> {
    let members = object(json, pointer, "\"contract\"")?;
    let input_at = at(pointer, "input");
    let mut input = match get(members, "input") {
        Some(input @ Json::Object(_)) => input.clone(),
        Some(_) => return Err(format_error(&input_at, "\"input\" must be an object")),
        None => return Err(format_error(&input_at, "the key \"input\" is missing")),
    };
    check_numbers(&input, &input_at)?;
    canonical_numbers(&mut input);
    let output_at = at(pointer, "output");
    let output = object(get(members, "output"), &output_at, "\"output\"")?;
    let keys_at = at(&output_at, "keys");
    let keys = match get(output, "keys") {
        Some(Json::Array(items)) => {
            let mut keys: Vec<String> = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let Json::String(key) = item else {
                    return Err(format_error(
                        &format!("{keys_at}/{i}"),
                        "an output key must be a string",
                    ));
                };
                if keys.contains(key) {
                    return Err(format_error(
                        &format!("{keys_at}/{i}"),
                        format!("the output key {key:?} is given twice"),
                    ));
                }
                keys.push(key.clone());
            }
            keys
        }
        Some(_) => return Err(format_error(&keys_at, "\"keys\" must be an array")),
        None => return Err(format_error(&keys_at, "the key \"keys\" is missing")),
    };
    no_unknown_keys(output, &OUTPUT_KEYS, &output_at)?;
    no_unknown_keys(members, &CONTRACT_KEYS, pointer)?;
    Ok(Contract::recorded(input, keys))
}

/// Every number in a contract is an integer literal of at most 2⁵³ − 1 in
/// magnitude (`spec/bundles.md`, *Contract*): the first other one, in
/// pre-order and by key, is a `format_error`.
fn check_numbers(json: &Json, pointer: &str) -> Result<()> {
    let mut pending: Vec<(&Json, String)> = vec![(json, pointer.to_owned())];
    while let Some((value, here)) = pending.pop() {
        match value {
            Json::Number(n) => {
                let fits = n
                    .as_i128()
                    .is_some_and(|n| n.unsigned_abs() <= u128::from(MAX_RECORDED_LIMIT));
                if !fits {
                    return Err(format_error(
                        &here,
                        "a number in a contract is an integer from -(2^53 - 1) to 2^53 - 1",
                    ));
                }
            }
            Json::Array(items) => pending.extend(
                items
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, item)| (item, format!("{here}/{i}"))),
            ),
            Json::Object(members) => pending.extend(
                by_key(members)
                    .into_iter()
                    .rev()
                    .map(|(key, item)| (item, format!("{here}/{}", pointer_token(key)))),
            ),
            _ => {}
        }
    }
    Ok(())
}

/// Write every number, already checked, as its canonical integer literal
/// (`-0` is `0`).
fn canonical_numbers(json: &mut Json) {
    let mut pending: Vec<&mut Json> = vec![json];
    while let Some(value) = pending.pop() {
        match value {
            Json::Number(n) => {
                if let Some(int) = n.as_i128() {
                    *n = JsonNumber::from_literal(int.to_string());
                }
            }
            Json::Array(items) => pending.extend(items.iter_mut()),
            Json::Object(members) => pending.extend(members.iter_mut().map(|(_, v)| v)),
            _ => {}
        }
    }
}

fn read_cases(json: Option<&Json>, pointer: &str) -> Result<Vec<CaseFile>> {
    let items = match json {
        None => return Ok(Vec::new()),
        Some(Json::Array(items)) => items,
        Some(_) => return Err(format_error(pointer, "\"cases\" must be an array")),
    };
    let mut cases: Vec<CaseFile> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let here = format!("{pointer}/{i}");
        let Json::Object(members) = item else {
            return Err(format_error(
                &here,
                "a case file reference must be an object",
            ));
        };
        let path_at = at(&here, "path");
        let path = string(get(members, "path"), &path_at, "\"path\"")?;
        check_case_path(path).map_err(|e| e.at_pointer(path_at.clone()))?;
        if cases.iter().any(|case| case.path() == path) {
            return Err(format_error(
                &path_at,
                format!("the case file {path:?} is given twice"),
            ));
        }
        let digest = digest(get(members, "digest"), &at(&here, "digest"), "\"digest\"")?;
        no_unknown_keys(members, &CASE_KEYS, &here)?;
        cases.push(CaseFile::with_digest(path, digest).map_err(|e| e.at_pointer(path_at))?);
    }
    Ok(cases)
}

fn int(n: u64) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

fn text(s: &str) -> Json {
    Json::String(s.to_owned())
}

/// Write a bundle's content, keys in the order of `spec/bundles.md`.
#[must_use]
pub fn write_bundle(bundle: &Bundle) -> Json {
    let mut members: Vec<(String, Json)> = vec![
        ("id".into(), text(bundle.id())),
        ("revision".into(), text(bundle.revision())),
    ];
    members.push(match bundle.rules() {
        BundleRules::RuleSet(ruleset) => ("ruleset".into(), write_ruleset(ruleset)),
        BundleRules::DecisionTable(table) => ("decision_table".into(), write_table(table)),
    });
    members.push((
        "ruleset_digest".into(),
        text(&bundle.ruleset_digest().to_string()),
    ));
    let engine = bundle.engine();
    let limits = LIMIT_NAMES
        .iter()
        .zip(engine.limits().values())
        .map(|(name, n)| ((*name).to_owned(), int(n)))
        .collect();
    members.push((
        "engine".into(),
        Json::Object(vec![
            ("name".into(), text(engine.name())),
            ("version".into(), text(engine.version())),
            ("limits".into(), Json::Object(limits)),
        ]),
    ));
    let contract = bundle.contract();
    members.push((
        "contract".into(),
        Json::Object(vec![
            ("input".into(), contract.input().clone()),
            (
                "output".into(),
                Json::Object(vec![(
                    "keys".into(),
                    Json::Array(contract.output_keys().iter().map(|k| text(k)).collect()),
                )]),
            ),
        ]),
    ));
    if !bundle.cases().is_empty() {
        members.push((
            "cases".into(),
            Json::Array(
                bundle
                    .cases()
                    .iter()
                    .map(|case| {
                        Json::Object(vec![
                            ("path".into(), text(case.path())),
                            ("digest".into(), text(&case.digest().to_string())),
                        ])
                    })
                    .collect(),
            ),
        ));
    }
    Json::Object(members)
}
