//! The `audit` and `draw` conformance vectors (`conformance/vectors/audit`,
//! `conformance/vectors/visual`) through the command line: each document on
//! standard input, its report or diagram read from standard output, and the
//! exit code checked against what the vector expects.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use cleanrules::format::{ValueContext, read_value};
use cleanrules::json::{Json, Limits, parse, same, to_string};
use cleanrules::rules::Decision;

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn cleanrules(args: &[String], stdin: &str) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cleanrules"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    Run {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn get<'a>(json: &'a Json, key: &str) -> Option<&'a Json> {
    match json {
        Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn text(json: Option<&Json>) -> &str {
    match json {
        Some(Json::String(s)) => s,
        other => panic!("not a string: {other:?}"),
    }
}

/// The vectors of every file of `area`, in file and vector order.
fn vectors(area: &str) -> Vec<Json> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/vectors")
        .join(area);
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    let limits = Limits::default().with_max_size(64 << 20);
    files
        .iter()
        .flat_map(|file| {
            let json = parse(&std::fs::read_to_string(file).unwrap(), &limits).unwrap();
            match get(&json, "vectors") {
                Some(Json::Array(items)) => items.clone(),
                _ => panic!("no vectors in {}", file.display()),
            }
        })
        .collect()
}

/// Every `audit` vector: the JSON report without messages and witnesses as
/// expected, each witness showing its finding, and the exit code of the
/// default `--fail-on error`.
#[test]
fn audit_vectors_through_the_command_line() {
    let mut count = 0;
    for vector in vectors("audit") {
        let id = text(get(&vector, "id")).to_owned();
        let given = get(&vector, "given").unwrap();
        let document = to_string(get(given, "document").unwrap());
        let mut args: Vec<String> = ["audit", "-", "--format", "json"]
            .map(String::from)
            .to_vec();
        if let Some(Json::Object(limits)) = get(given, "limits") {
            for (key, value) in limits {
                args.push(format!("--{}", key.replace('_', "-")));
                args.push(to_string(value));
            }
        }
        if get(given, "ruleset_digest") == Some(&Json::Bool(true)) {
            args.push("--ruleset-digest".into());
        }
        let run = cleanrules(&args, &document);
        // A document without rules: the error, as JSON, with the exit code 3.
        if let Some(error) = get(get(&vector, "expect").unwrap(), "error") {
            assert_eq!((run.code, run.stdout.as_str()), (3, ""), "{id}");
            let written = parse(run.stderr.trim_end(), &Limits::default()).unwrap();
            let code = get(get(&written, "error").unwrap(), "code");
            assert_eq!(code, get(error, "code"), "{id}");
            count += 1;
            continue;
        }
        assert!(run.stderr.is_empty(), "{id}: {}", run.stderr);
        let written = parse(run.stdout.trim_end(), &Limits::default()).unwrap();
        assert_eq!(
            get(&written, "version").map(to_string).as_deref(),
            Some("1"),
            "{id}"
        );
        let mut report = get(&written, "audit").unwrap().clone();
        // Each witness, evaluated by the rule set itself.
        let ruleset = cleanrules::format::read_document(&document, &Limits::default())
            .unwrap()
            .into_ruleset()
            .unwrap();
        let mut errors = false;
        if let Json::Object(members) = &mut report {
            for (key, value) in members.iter_mut() {
                let (Json::Array(items), "diagnostics") = (value, key.as_str()) else {
                    continue;
                };
                for item in items {
                    let code = text(get(item, "code")).to_owned();
                    let severity = text(get(item, "severity")).to_owned();
                    errors |= severity == "error";
                    let rules: Vec<String> = match get(item, "rules") {
                        Some(Json::Array(names)) => {
                            names.iter().map(|n| text(Some(n)).to_owned()).collect()
                        }
                        _ => panic!("{id}: no rules"),
                    };
                    if let Some(witness) = get(item, "witness") {
                        let input = read_value(witness, "/witness", ValueContext::Data).unwrap();
                        let fired = |decision: &Decision<'_>, name: &str| {
                            decision
                                .results()
                                .iter()
                                .any(|r| r.fired && r.rule.name() == name)
                        };
                        let shown =
                            match (code.as_str(), severity.as_str(), ruleset.evaluate(&input)) {
                                ("overlap", "error", Err(e)) => {
                                    e.code().as_str() == "hit_policy_error"
                                        && rules.iter().all(|r| e.context().rules.contains(r))
                                }
                                ("overlap", _, Ok(decision)) => {
                                    rules.iter().all(|r| fired(&decision, r))
                                }
                                ("gap", _, Ok(decision)) => {
                                    decision.results().iter().all(|r| !r.fired)
                                }
                                _ => false,
                            };
                        assert!(shown, "{id}: the witness does not show {code}");
                    } else {
                        assert!(!matches!(code.as_str(), "overlap" | "gap"), "{id}");
                    }
                    if let Json::Object(fields) = item {
                        fields.retain(|(k, _)| k != "message" && k != "witness");
                    }
                }
            }
        }
        let wanted = get(get(&vector, "expect").unwrap(), "audit").unwrap();
        assert!(
            same(&report, wanted),
            "{id}\ngot  {}\nwant {}",
            to_string(&report),
            to_string(wanted)
        );
        assert_eq!(run.code, i32::from(errors), "{id}: exit code");
        count += 1;
    }
    assert_eq!(count, 35, "every audit vector runs");
}

/// Every `draw` vector without an input: the diagram, or the error's code
/// with the exit code 3. Those with an input draw a decision, which the
/// command line does not make (`spec/cli.md`, *Not yet*).
#[test]
fn draw_vectors_through_the_command_line() {
    let (mut count, mut skipped) = (0, 0);
    for vector in vectors("visual") {
        let id = text(get(&vector, "id")).to_owned();
        let given = get(&vector, "given").unwrap();
        if get(given, "input").is_some() {
            skipped += 1;
            continue;
        }
        let mut args: Vec<String> = vec![
            "draw".into(),
            "-".into(),
            "--format".into(),
            text(get(given, "format")).into(),
        ];
        if let Some(n) = get(given, "limits").and_then(|l| get(l, "max_diagram_size")) {
            args.push("--max-diagram-size".into());
            args.push(to_string(n));
        }
        let run = cleanrules(&args, &to_string(get(given, "document").unwrap()));
        let expect = get(&vector, "expect").unwrap();
        if let Some(error) = get(expect, "error") {
            assert_eq!(run.code, 3, "{id}");
            let code = text(get(error, "code"));
            assert!(
                run.stderr.starts_with(&format!("error[{code}]: ")),
                "{id}: {}",
                run.stderr
            );
            assert!(run.stdout.is_empty(), "{id}");
        } else {
            assert_eq!(run.code, 0, "{id}: {}", run.stderr);
            let diagram = text(get(expect, "diagram"));
            let printed = run.stdout.strip_suffix('\n').unwrap();
            assert!(
                printed == diagram || run.stdout == diagram,
                "{id}\ngot\n{}\nwant\n{diagram}",
                run.stdout
            );
        }
        count += 1;
    }
    assert_eq!((count, skipped), (30, 14), "draw vectors run and skipped");
}
