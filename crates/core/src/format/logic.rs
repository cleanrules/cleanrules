//! Logics in JSON (`spec/json-format.md`, *Logics*): a name, or an object
//! with the name, the parameter and the complement.

use crate::error::Result;
use crate::format::rules::{at, format_error, get, no_unknown_keys};
use crate::json::Json;
use crate::logic::{AnyLogic, Complement, LogicName, Norm};

/// The keys of a logic's parameters, in the order of the table.
const PARAMETERS: [&str; 3] = ["gamma", "p", "s"];
const KEYS: [&str; 5] = ["name", "gamma", "p", "s", "complement"];
const COMPLEMENT_KEYS: [&str; 3] = ["name", "lam", "w"];

/// A key holding a number, if present: at most one of `keys`.
fn parameter(
    members: &[(String, Json)],
    keys: &[&str],
    pointer: &str,
) -> Result<Option<(String, f64)>> {
    let mut found: Option<(String, f64)> = None;
    for &key in keys {
        let Some(json) = get(members, key) else {
            continue;
        };
        let Json::Number(number) = json else {
            return Err(format_error(
                &at(pointer, key),
                format!("{key:?} must be a number"),
            ));
        };
        if let Some((first, _)) = &found {
            return Err(format_error(
                &at(pointer, key),
                format!("{first:?} and {key:?} are given; a logic takes one parameter"),
            ));
        }
        found = Some((key.to_owned(), number.as_f64()));
    }
    Ok(found)
}

/// The name under `name`, a string.
fn name_of<'j>(members: &'j [(String, Json)], pointer: &str) -> Result<&'j str> {
    match get(members, "name") {
        Some(Json::String(name)) => Ok(name),
        Some(_) => Err(format_error(
            &at(pointer, "name"),
            "\"name\" must be a string",
        )),
        None => Err(format_error(
            &at(pointer, "name"),
            "the key \"name\" is missing",
        )),
    }
}

/// A logic or a complement whose shape is valid: its name, where the name
/// is, its parameter, and where the parameter is (or the whole, without one).
struct Parts<'j> {
    name: &'j str,
    name_at: String,
    parameter: Option<(String, f64)>,
    parameter_at: String,
}

impl Parts<'_> {
    fn parameter(&self) -> Option<(&str, f64)> {
        self.parameter
            .as_ref()
            .map(|(key, value)| (key.as_str(), *value))
    }
}

/// The shape of a logic or a complement: a name, or an object with `name`,
/// `keys` (at most one, a number) and, for a logic, `complement`.
fn shape<'j>(
    json: &'j Json,
    pointer: &str,
    keys: &[&str],
    known: &[&str],
    what: &str,
) -> Result<(Parts<'j>, Option<&'j Json>)> {
    match json {
        Json::String(name) => Ok((
            Parts {
                name,
                name_at: pointer.to_owned(),
                parameter: None,
                parameter_at: pointer.to_owned(),
            },
            None,
        )),
        Json::Object(members) => {
            let name = name_of(members, pointer)?;
            let parameter = parameter(members, keys, pointer)?;
            let complement = get(members, "complement");
            no_unknown_keys(members, known, pointer)?;
            let parameter_at = parameter
                .as_ref()
                .map_or_else(|| pointer.to_owned(), |(key, _)| at(pointer, key));
            Ok((
                Parts {
                    name,
                    name_at: at(pointer, "name"),
                    parameter,
                    parameter_at,
                },
                complement,
            ))
        }
        _ => Err(format_error(
            pointer,
            format!("a {what} is a name or an object with \"name\""),
        )),
    }
}

/// Read a logic in its JSON form (`spec/json-format.md`, *Logics*); `pointer`
/// locates it in errors. The logic module checks it; this adds the pointers.
///
/// # Errors
///
/// `format_error` for its shape, `registry_error` for an unknown logic or
/// complement, and `rule_definition_error` for the logic's parameter and then
/// the complement's, in that order.
pub fn read_logic(json: &Json, pointer: &str) -> Result<AnyLogic> {
    // 1. The shape, the logic's and then the complement's.
    let (logic, complement) = shape(json, pointer, &PARAMETERS, &KEYS, "logic")?;
    let complement_at = at(pointer, "complement");
    let complement = complement
        .map(|json| {
            shape(
                json,
                &complement_at,
                &COMPLEMENT_KEYS[1..],
                &COMPLEMENT_KEYS,
                "complement",
            )
        })
        .transpose()?
        .map(|(parts, _)| parts);
    // 2. The names.
    LogicName::from_name(logic.name).map_err(|e| e.at_pointer(&logic.name_at))?;
    if let Some(complement) = &complement {
        Complement::registered(complement.name).map_err(|e| e.at_pointer(&complement.name_at))?;
    }
    // 3. The logic's parameter, then the complement's.
    let plain = AnyLogic::from_parts(logic.name, logic.parameter(), None)
        .map_err(|e| e.at_pointer(&logic.parameter_at))?;
    let Some(complement) = complement else {
        return Ok(plain);
    };
    let built = Complement::from_name(complement.name, complement.parameter())
        .map_err(|e| e.at_pointer(&complement.parameter_at))?;
    if built == Complement::Standard {
        return Ok(plain);
    }
    AnyLogic::from_parts(logic.name, logic.parameter(), Some(built))
        .map_err(|e| e.at_pointer(&complement_at))
}

fn float(value: f64) -> Json {
    crate::format::write_value(&crate::value::Value::Float(value))
}

/// Write a logic in its JSON form: its name when it has no parameter and the
/// standard complement, and otherwise an object (`spec/json-format.md`,
/// *Logics*).
#[must_use]
pub fn write_logic(logic: &AnyLogic) -> Json {
    let AnyLogic::Fuzzy(fuzzy) = logic else {
        return Json::String(logic.name().as_str().into());
    };
    let parameter = match fuzzy.norm() {
        // `gamma` is written only when it is not its default, 0.
        Norm::Hamacher { gamma: 0.0 } => None,
        norm => norm.parameter(),
    };
    let complement = fuzzy.complement();
    let name = Json::String(logic.name().as_str().into());
    if parameter.is_none() && complement == Complement::Standard {
        return name;
    }
    let mut members = vec![("name".to_owned(), name)];
    if let Some((key, value)) = parameter {
        members.push((key.to_owned(), float(value)));
    }
    if complement != Complement::Standard {
        let name = Json::String(complement.as_str().into());
        let written = match complement.parameter() {
            None => name,
            Some((key, value)) => Json::Object(vec![
                ("name".to_owned(), name),
                (key.to_owned(), float(value)),
            ]),
        };
        members.push(("complement".to_owned(), written));
    }
    Json::Object(members)
}

#[cfg(test)]
mod tests {
    use super::{read_logic, write_logic};
    use crate::error::ErrorCode;
    use crate::json::{Limits, parse, to_string};
    use crate::logic::{AnyLogic, Complement, Fuzzy, Kleene, LogicName, Norm};

    fn read(text: &str) -> crate::error::Result<AnyLogic> {
        read_logic(&parse(text, &Limits::default()).unwrap(), "/logic")
    }

    fn fuzzy(norm: Norm, complement: Complement) -> AnyLogic {
        AnyLogic::Fuzzy(Fuzzy::new(norm, complement))
    }

    #[test]
    fn logics_read_and_write_back() {
        let cases = [
            (r#""boolean""#, AnyLogic::Boolean(Kleene)),
            (r#""min""#, fuzzy(Norm::Min, Complement::Standard)),
            (
                r#""hamacher""#,
                fuzzy(Norm::Hamacher { gamma: 0.0 }, Complement::Standard),
            ),
            (
                r#"{"name":"hamacher","gamma":2.0}"#,
                fuzzy(Norm::Hamacher { gamma: 2.0 }, Complement::Standard),
            ),
            (
                r#"{"name":"yager","p":2.0}"#,
                fuzzy(Norm::Yager { p: 2.0 }, Complement::Standard),
            ),
            (
                r#"{"name":"frank","s":0.5}"#,
                fuzzy(Norm::Frank { s: 0.5 }, Complement::Standard),
            ),
            (
                r#"{"name":"dombi","p":1.5,"complement":{"name":"sugeno","lam":0.5}}"#,
                fuzzy(Norm::Dombi { p: 1.5 }, Complement::Sugeno { lam: 0.5 }),
            ),
            (
                r#"{"name":"prod","complement":{"name":"yager","w":2.0}}"#,
                fuzzy(Norm::Prod, Complement::Yager { w: 2.0 }),
            ),
        ];
        for (text, logic) in cases {
            assert_eq!(read(text).unwrap(), logic, "{text}");
            assert_eq!(to_string(&write_logic(&logic)), text);
        }
        // Other spellings of the same logic, fuzzytool's aliases among them.
        let same = [
            (r#""product""#, r#""prod""#),
            (r#"{"name":"product","complement":"zadeh"}"#, r#""prod""#),
            (
                r#"{"name":"min","complement":{"name":"zadeh"}}"#,
                r#""min""#,
            ),
            (r#"{"name":"min"}"#, r#""min""#),
            (r#"{"name":"min","complement":"standard"}"#, r#""min""#),
            (r#"{"name":"hamacher","gamma":0}"#, r#""hamacher""#),
            (r#"{"name":"yager","p":2}"#, r#"{"name":"yager","p":2.0}"#),
            (
                r#"{"name":"min","complement":{"name":"standard"}}"#,
                r#""min""#,
            ),
        ];
        for (text, written) in same {
            assert_eq!(
                to_string(&write_logic(&read(text).unwrap())),
                written,
                "{text}"
            );
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One table of cases.
    fn errors_in_the_order_of_the_spec() {
        let cases = [
            ("3", ErrorCode::FormatError, "/logic"),
            ("{}", ErrorCode::FormatError, "/logic/name"),
            (r#"{"name":1}"#, ErrorCode::FormatError, "/logic/name"),
            (
                r#"{"name":"yager","p":"2"}"#,
                ErrorCode::FormatError,
                "/logic/p",
            ),
            (
                r#"{"name":"yager","p":2,"s":3}"#,
                ErrorCode::FormatError,
                "/logic/s",
            ),
            (
                r#"{"name":"nope","alpha":1}"#,
                ErrorCode::FormatError,
                "/logic/alpha",
            ),
            (
                r#"{"name":"min","complement":3}"#,
                ErrorCode::FormatError,
                "/logic/complement",
            ),
            (
                r#"{"name":"min","complement":{"name":"sugeno","lam":1,"w":2}}"#,
                ErrorCode::FormatError,
                "/logic/complement/w",
            ),
            (
                r#"{"name":"min","complement":{"name":"sugeno","x":1}}"#,
                ErrorCode::FormatError,
                "/logic/complement/x",
            ),
            (r#""fuzzy""#, ErrorCode::RegistryError, "/logic"),
            (
                r#"{"name":"nope","p":-1}"#,
                ErrorCode::RegistryError,
                "/logic/name",
            ),
            (
                r#"{"name":"min","complement":"nope"}"#,
                ErrorCode::RegistryError,
                "/logic/complement",
            ),
            (
                r#"{"name":"min","complement":{"name":"nope"}}"#,
                ErrorCode::RegistryError,
                "/logic/complement/name",
            ),
            (r#""yager""#, ErrorCode::RuleDefinitionError, "/logic"),
            (
                r#"{"name":"frank"}"#,
                ErrorCode::RuleDefinitionError,
                "/logic",
            ),
            (
                r#"{"name":"hamacher","p":1}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/p",
            ),
            (
                r#"{"name":"min","s":2}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/s",
            ),
            (
                r#"{"name":"frank","s":1}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/s",
            ),
            (
                r#"{"name":"dombi","p":0}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/p",
            ),
            (
                r#"{"name":"min","complement":"sugeno"}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/complement",
            ),
            (
                r#"{"name":"min","complement":{"name":"yager","lam":1}}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/complement/lam",
            ),
            (
                r#"{"name":"min","complement":{"name":"standard","w":1}}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/complement/w",
            ),
            (
                r#"{"name":"min","complement":{"name":"sugeno","lam":-1}}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/complement/lam",
            ),
            (
                r#"{"name":"boolean","complement":"yager"}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/complement",
            ),
            (
                r#"{"name":"boolean","complement":{"name":"sugeno","lam":1}}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/complement",
            ),
            // The logic's parameter before the complement's.
            (
                r#"{"name":"frank","complement":{"name":"sugeno","lam":-1}}"#,
                ErrorCode::RuleDefinitionError,
                "/logic",
            ),
            (
                r#"{"name":"yager","p":0,"complement":"sugeno"}"#,
                ErrorCode::RuleDefinitionError,
                "/logic/p",
            ),
            // The names before the parameters.
            (
                r#"{"name":"frank","complement":"nope"}"#,
                ErrorCode::RegistryError,
                "/logic/complement",
            ),
        ];
        for (text, code, pointer) in cases {
            let error = read(text).unwrap_err();
            assert_eq!(error.code(), code, "{text}: {error}");
            assert_eq!(error.context().pointer.as_deref(), Some(pointer), "{text}");
        }
        let error = read(r#"{"name":"min","complement":"nope"}"#).unwrap_err();
        assert_eq!(error.context().name.as_deref(), Some("nope"));
        assert_eq!(error.context().available, ["standard", "sugeno", "yager"]);
        assert_eq!(read(r#""boolean""#).unwrap().name(), LogicName::Boolean);
    }
}
