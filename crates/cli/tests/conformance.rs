//! The `audit`, `draw` and `test_run` conformance vectors
//! (`conformance/vectors/audit`, `conformance/vectors/visual`,
//! `conformance/vectors/tests`) through the command line: each document on
//! standard input or in a file, its report or diagram read from standard
//! output, and the exit code checked against what the vector expects.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cleanrules::digest::Digest;
use cleanrules::format::{ValueContext, read_value};
use cleanrules::json::{Json, Limits, parse, same, to_string};
use cleanrules::rules::Decision;

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn cleanrules(args: &[String], stdin: &str) -> Run {
    cleanrules_in(Path::new("."), args, stdin)
}

fn cleanrules_in(dir: &Path, args: &[String], stdin: &str) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cleanrules"))
        .args(args)
        .current_dir(dir)
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

/// The compared fields of an error (`spec/errors.md`, *Comparing errors*),
/// as the core's runner keeps them.
fn compared_error(json: &mut Json) {
    const COMPARED: [&str; 11] = [
        "code", "pointer", "line", "column", "rule", "rules", "path", "types", "row", "name",
        "executed",
    ];
    if let Json::Object(members) = json {
        members.retain(|(k, _)| COMPARED.contains(&k.as_str()));
    }
}

/// A member of an object, to change.
fn member<'a>(json: &'a mut Json, key: &str) -> Option<&'a mut Json> {
    match json {
        Json::Object(members) => members.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// The rules of a `test_run` vector in `rules.json` and its case files at
/// their paths, under a folder of its own; and the arguments and standard
/// input that run them: the case files at their paths, a bundle's under its
/// folder, or `cases` on standard input.
fn test_run_command(
    root: &Path,
    id: &str,
    given: &Json,
    files: &[(String, String)],
) -> (PathBuf, Vec<String>, String) {
    let dir = root.join(id.replace('/', "_"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rules.json"),
        to_string(get(given, "document").unwrap()),
    )
    .unwrap();
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let bundle = get(get(given, "document").unwrap(), "bundle").is_some();
    let mut args: Vec<String> = ["test", "rules.json", "--format", "json"]
        .map(String::from)
        .to_vec();
    let mut stdin = String::new();
    if let Some(cases) = get(given, "cases") {
        args.push("-".into());
        stdin = to_string(cases);
    } else if !bundle {
        args.extend(files.iter().map(|(path, _)| path.clone()));
    }
    if let Some(Json::Object(limits)) = get(given, "limits") {
        for (key, value) in limits {
            args.push(format!("--{}", key.replace('_', "-")));
            args.push(to_string(value));
        }
    }
    if get(given, "ruleset_digest") == Some(&Json::Bool(true)) {
        args.push("--ruleset-digest".into());
    }
    (dir, args, stdin)
}

/// Every error of a written test run reduced to its compared fields: a
/// result's `error`, and the `actual` of a mismatch of the key `error`.
fn compared_errors(run: &mut Json) {
    let Some(Json::Array(results)) = member(run, "results") else {
        return;
    };
    for result in results {
        if let Some(error) = member(result, "error") {
            compared_error(error);
        }
        if let Some(Json::Array(mismatches)) = member(result, "mismatches") {
            for mismatch in mismatches {
                if get(mismatch, "key") == Some(&Json::String("error".into()))
                    && let Some(error) = member(mismatch, "actual")
                {
                    compared_error(error);
                }
            }
        }
    }
}

/// Every `test_run` vector but those that cancel the run (the command has no
/// callback) and those that give two different files the same path (one path
/// names one file), through `test_run_command`, the test run read from
/// standard output with its errors reduced to their compared fields
/// (`conformance/README.md`, `test_run`). The exit code is 0 exactly when
/// the run is complete, has results and every one passed. The `statistics`
/// vectors of the same folder are skipped and counted: the command does not
/// gather statistics yet (`spec/cli.md`, *Not yet*).
#[test]
fn test_run_vectors_through_the_command_line() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("test-run-vectors");
    let (mut count, mut skipped, mut statistics) = (0, 0, 0);
    for vector in vectors("tests") {
        let id = text(get(&vector, "id")).to_owned();
        let given = get(&vector, "given").unwrap();
        if text(get(given, "kind")) == "statistics" {
            statistics += 1;
            continue;
        }
        let files: Vec<(String, String)> = match get(given, "files") {
            Some(Json::Array(items)) => items
                .iter()
                .map(|f| {
                    (
                        text(get(f, "path")).to_owned(),
                        text(get(f, "text")).to_owned(),
                    )
                })
                .collect(),
            _ => Vec::new(),
        };
        let repeats = files
            .iter()
            .enumerate()
            .any(|(i, (path, text))| files[..i].iter().any(|(p, t)| p == path && t != text));
        if get(given, "cancel_after").is_some() || repeats {
            skipped += 1;
            continue;
        }
        let (dir, args, stdin) = test_run_command(&root, &id, given, &files);
        let run = cleanrules_in(&dir, &args, &stdin);
        assert!(run.stderr.is_empty(), "{id}: {}", run.stderr);
        let written = parse(run.stdout.trim_end(), &Limits::default()).unwrap();
        assert_eq!(
            get(&written, "version").map(to_string).as_deref(),
            Some("1"),
            "{id}"
        );
        let mut actual = get(&written, "test_run").unwrap().clone();
        compared_errors(&mut actual);
        let mut expect = get(get(&vector, "expect").unwrap(), "test_run")
            .unwrap()
            .clone();
        // This engine's version; and a case file read from standard input,
        // which the core's runner gives without bytes, has their digest.
        *member(member(&mut expect, "engine").unwrap(), "version").unwrap() =
            Json::String(cleanrules::bundle::ENGINE_VERSION.to_owned());
        if !stdin.is_empty()
            && let Some(Json::Array(entries)) = member(&mut expect, "files")
            && let Some(Json::Object(entry)) = entries.first_mut()
        {
            entry.push((
                "digest".into(),
                Json::String(Digest::of(stdin.as_bytes()).to_string()),
            ));
        }
        assert!(
            same(&actual, &expect),
            "{id}\ngot  {}\nwant {}",
            to_string(&actual),
            to_string(&expect)
        );
        let summary = get(&expect, "summary").unwrap();
        let passes = get(summary, "partial") == Some(&Json::Bool(false))
            && get(summary, "total") == get(summary, "passed")
            && get(summary, "total").map(to_string).as_deref() != Some("0");
        assert_eq!(run.code, i32::from(!passes), "{id}: exit code");
        count += 1;
    }
    assert_eq!(
        (count, skipped, statistics),
        (25, 3, 9),
        "test_run vectors run and skipped, and statistics vectors skipped"
    );
}
