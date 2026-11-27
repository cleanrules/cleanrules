//! The `cleanrules` command, run as a user runs it (`spec/cli.md`): exit
//! codes, text and JSON output, and errors.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cleanrules::bundle::{Bundle, BundleId, CaseFileRef, Revision};
use cleanrules::format::{DocumentFormat, write_document_as};
use cleanrules::json::{Json, Limits, parse};

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples")
}

/// Run the command in `tests/data`, with `stdin` as standard input.
fn run_in(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cleanrules"))
        .args(args)
        .current_dir(dir)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(bytes) = stdin {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    Run {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn run(args: &[&str]) -> Run {
    run_in(&data(), args, None)
}

fn json(text: &str) -> Json {
    parse(text.trim_end(), &Limits::default()).unwrap()
}

fn get<'a>(json: &'a Json, key: &str) -> Option<&'a Json> {
    match json {
        Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn string(json: Option<&Json>) -> &str {
    match json {
        Some(Json::String(s)) => s,
        other => panic!("not a string: {other:?}"),
    }
}

/// A scratch file under the target directory.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

// Exit codes by severity (`spec/cli.md`, *Exit codes*).

#[test]
fn audit_exits_by_severity() {
    // (file, the most serious severity it gives)
    let files = [
        ("clean.rules", None),
        ("info.rules", Some("info")),
        ("warning.rules", Some("warning")),
        ("unique.rules", Some("error")),
    ];
    let rank = |s: &str| ["error", "warning", "info"].iter().position(|x| *x == s);
    for (file, found) in files {
        for fail_on in ["error", "warning", "info", "never"] {
            let result = run(&["audit", file, "--fail-on", fail_on]);
            let fails = match (found, rank(fail_on)) {
                (Some(found), Some(threshold)) => rank(found).unwrap() <= threshold,
                _ => false,
            };
            assert_eq!(
                result.code,
                i32::from(fails),
                "{file} --fail-on {fail_on}: {}",
                result.stderr
            );
            assert_eq!(result.stderr, "");
        }
    }
    // The default is error.
    assert_eq!(run(&["audit", "unique.rules"]).code, 1);
    assert_eq!(run(&["audit", "warning.rules"]).code, 0);
}

#[test]
fn audit_text_reports() {
    for file in [
        "clean.rules",
        "info.rules",
        "warning.rules",
        "unique.rules",
        "rule.rules",
        "table.csv",
    ] {
        let result = run(&["audit", file, "--fail-on", "never"]);
        assert_eq!(result.code, 0, "{file}");
        insta::assert_snapshot!(format!("audit_text_{file}"), result.stdout);
    }
}

#[test]
fn audit_json_reports() {
    let result = run(&["audit", "unique.rules", "--format", "json"]);
    assert_eq!(result.code, 1);
    assert!(result.stdout.ends_with("}\n") && result.stdout.lines().count() == 1);
    insta::assert_snapshot!("audit_json_unique", result.stdout);
    let report = json(&result.stdout);
    assert!(get(get(&report, "audit").unwrap(), "ruleset_digest").is_none());
    // The rule set digest, and the limits, on request.
    let result = run(&[
        "audit",
        "unique.rules",
        "--format=json",
        "--ruleset-digest",
        "--max-terms",
        "64",
        "--max-steps",
        "0",
    ]);
    // With no steps, the overlap is not found.
    assert_eq!(result.code, 0);
    let audit = get(&json(&result.stdout), "audit").unwrap().clone();
    assert!(string(get(&audit, "ruleset_digest")).starts_with("sha256:"));
    let limits = cleanrules::json::to_string(get(&audit, "limits").unwrap());
    assert_eq!(limits, r#"{"max_terms":64,"max_steps":0}"#);
}

#[test]
fn audit_reads_every_syntax() {
    let examples = examples();
    for file in [
        "discounts.json",
        "discounts.yaml",
        "discounts.rules",
        "risk.csv",
    ] {
        let result = run_in(&examples, &["audit", file], None);
        assert_eq!(result.code, 0, "{file}: {}", result.stderr);
        assert!(result.stdout.starts_with("Audit of a "), "{file}");
    }
    // The same rules give the same report, whatever their syntax.
    let text = |file| run_in(&examples, &["audit", file, "--format", "json"], None).stdout;
    assert_eq!(text("discounts.json"), text("discounts.yaml"));
    assert_eq!(text("discounts.json"), text("discounts.rules"));
    // Standard input: JSON by default, or the syntax given.
    let rules = std::fs::read(data().join("unique.rules")).unwrap();
    let piped = run_in(&data(), &["audit", "-", "--syntax", "text"], Some(&rules));
    assert_eq!(piped.code, 1);
    assert_eq!(piped.stdout, run(&["audit", "unique.rules"]).stdout);
    let document = std::fs::read(examples.join("discounts.json")).unwrap();
    let piped = run_in(&data(), &["audit", "-"], Some(&document));
    assert_eq!(piped.code, 0, "{}", piped.stderr);
    // A suffix is compared without case.
    let upper = scratch("UNIQUE.RULES");
    std::fs::write(&upper, &rules).unwrap();
    let result = run(&["audit", upper.to_str().unwrap()]);
    assert_eq!(result.code, 1, "{}", result.stderr);
}

#[test]
fn audit_locates_bundles_and_rules() {
    let limits = Limits::default();
    let ruleset = cleanrules::text::parse(
        "HIT POLICY first\nRULE odd IF x > TRUE\nRULE rest IF TRUE",
        &limits,
    )
    .unwrap()
    .into_ruleset()
    .unwrap();
    let bundle = Bundle::new(
        ruleset,
        BundleId::new("acme/cli").unwrap(),
        Revision::new("1").unwrap(),
    );
    let path = scratch("bundle.json");
    let written = write_document_as(&bundle.into_document(), DocumentFormat::Json).unwrap();
    std::fs::write(&path, written).unwrap();
    let result = run(&["audit", path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let report = json(&result.stdout);
    let Some(Json::Array(diagnostics)) = get(get(&report, "audit").unwrap(), "diagnostics") else {
        panic!("no diagnostics");
    };
    assert_eq!(
        string(get(&diagnostics[0], "pointer")),
        "/bundle/ruleset/rules/0/when"
    );
    // A rule document: pointers under /rule.
    let result = run(&["audit", "rule.rules"]);
    assert!(result.stdout.contains("at /rule/when/operands/0"));
}

#[test]
fn validate_says_what_a_document_holds() {
    let result = run(&["validate", "unique.rules"]);
    assert_eq!(result.code, 0);
    insta::assert_snapshot!("validate_text", result.stdout);
    for (file, content) in [
        ("condition.rules", "condition"),
        ("rule.rules", "rule"),
        ("table.csv", "decision_table"),
    ] {
        let result = run(&["validate", file]);
        assert_eq!(result.code, 0);
        assert!(
            result
                .stdout
                .starts_with(&format!("{file}: valid {content}\n")),
            "{}",
            result.stdout
        );
    }
    let cases = run_in(&examples(), &["validate", "cases/discounts.json"], None);
    assert_eq!(cases.stdout, "cases/discounts.json: valid cases\n");
    // JSON: the agents' validate result.
    let result = run(&["validate", "table.csv", "--format", "json"]);
    assert_eq!(result.code, 0);
    let validated = json(&result.stdout);
    assert_eq!(get(&validated, "valid"), Some(&Json::Bool(true)));
    assert_eq!(string(get(&validated, "content")), "decision_table");
    assert!(get(&validated, "summary").is_some() && get(&validated, "input_schema").is_some());
    let piped = run_in(
        &data(),
        &["validate", "-"],
        Some(br#"{"version": 1, "condition": {"op": "const", "value": true}}"#),
    );
    assert_eq!(piped.stdout, "<stdin>: valid condition\n");
}

#[test]
fn draw_writes_diagrams() {
    let result = run(&["draw", "unique.rules"]);
    assert_eq!(result.code, 0);
    assert!(result.stdout.ends_with('\n'));
    insta::assert_snapshot!("draw_mermaid", result.stdout);
    let result = run(&["draw", "condition.rules", "--format", "dot"]);
    assert_eq!(result.code, 0);
    insta::assert_snapshot!("draw_dot", result.stdout);
    // A decision table: HTML by default.
    let result = run(&["draw", "table.csv"]);
    assert_eq!(result.code, 0);
    assert!(result.stdout.starts_with("<!DOCTYPE html>"));
    let fragment = run(&["draw", "table.csv", "--format", "html_fragment"]);
    assert!(!fragment.stdout.starts_with("<!DOCTYPE html>"));
    // To a file.
    let path = scratch("rules.mmd");
    let result = run(&["draw", "unique.rules", "--output", path.to_str().unwrap()]);
    assert_eq!((result.code, result.stdout.as_str()), (0, ""));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        run(&["draw", "unique.rules"]).stdout
    );
}

#[test]
fn draw_errors() {
    let result = run(&["draw", "table.csv", "--format", "mermaid"]);
    assert_eq!(result.code, 3);
    assert_eq!(result.stdout, "");
    insta::assert_snapshot!("draw_table_as_mermaid", result.stderr);
    let result = run(&["draw", "unique.rules", "--max-diagram-size", "3"]);
    assert_eq!(result.code, 3);
    assert!(result.stderr.starts_with("error[limit_exceeded]: "));
    let cases = run_in(&examples(), &["draw", "cases/discounts.json"], None);
    assert_eq!(cases.code, 3);
    assert!(cases.stderr.starts_with("error[unsupported]: "));
    let missing = scratch("no-such-dir").join("rules.mmd");
    let result = run(&[
        "draw",
        "unique.rules",
        "--output",
        missing.to_str().unwrap(),
    ]);
    assert_eq!(result.code, 3);
    assert!(result.stderr.starts_with("error[io_error]: cannot write "));
}

#[test]
fn errors_have_codes_and_places() {
    for (name, args) in [
        ("format_error", &["audit", "broken.json"][..]),
        ("rule_syntax_error", &["audit", "broken.rules"][..]),
        ("unsupported", &["audit", "condition.rules"][..]),
        ("usage_error", &["audit", "notes.txt"][..]),
    ] {
        let result = run(args);
        assert_eq!(result.code, if name == "usage_error" { 2 } else { 3 });
        assert_eq!(result.stdout, "");
        insta::assert_snapshot!(format!("error_text_{name}"), result.stderr);
        let mut json_args = args.to_vec();
        json_args.extend(["--format", "json"]);
        let result = run(&json_args);
        assert_eq!(result.stderr.lines().count(), 1);
        insta::assert_snapshot!(format!("error_json_{name}"), result.stderr);
    }
    // From standard input, at a line and a column.
    let result = run_in(
        &data(),
        &["audit", "-", "--syntax", "text"],
        Some(b"RULE a IF x >\n"),
    );
    assert_eq!(result.code, 3);
    assert!(result.stderr.starts_with("error[rule_syntax_error]: "));
    assert!(result.stderr.contains("  --> <stdin>:2:1\n"));
    let result = run_in(
        &data(),
        &["draw", "-", "--format", "dot"],
        Some(b"{\"version\": 2}"),
    );
    assert_eq!(result.code, 3);
    assert!(result.stderr.starts_with("error[unsupported_version]: "));
    let result = run_in(
        &data(),
        &["audit", "-", "--format", "json"],
        Some(b"{\"version\": 2}"),
    );
    let error = json(&result.stderr);
    assert_eq!(string(get(&error, "file")), "-");
    assert_eq!(
        string(get(get(&error, "error").unwrap(), "code")),
        "unsupported_version"
    );
    // A file that is not there; the operating system's message varies.
    for format in ["text", "json"] {
        let result = run(&["audit", "missing.json", "--format", format]);
        assert_eq!(result.code, 3);
        assert!(result.stderr.contains("io_error"));
        assert!(result.stderr.contains("missing.json"));
    }
    // Errors of the core without a place: a graded logic is not audited.
    let result = run_in(
        &data(),
        &["audit", "-", "--syntax", "yaml"],
        Some(b"version: 1\nruleset:\n  logic: prod\n  rules: []\n"),
    );
    assert_eq!(result.code, 3, "{}", result.stderr);
    assert!(
        result.stderr.starts_with("error[unsupported]: "),
        "{}",
        result.stderr
    );
}

#[test]
fn larger_documents_are_refused_unread() {
    let big = vec![b' '; 16 * 1024 * 1024 + 10];
    let result = run_in(&data(), &["audit", "-"], Some(&big));
    assert_eq!(result.code, 3);
    assert!(
        result.stderr.starts_with("error[limit_exceeded]: "),
        "{}",
        result.stderr
    );
}

#[test]
fn usage_errors_exit_with_2() {
    for args in [
        &["frob"][..],
        &["audit"][..],
        &["audit", "clean.rules", "--fail-on", "fatal"][..],
        &["audit", "clean.rules", "--format", "yaml"][..],
        &["draw", "clean.rules", "--format", "png"][..],
        &["audit", "clean.rules", "--max-steps", "-1"][..],
        &[][..],
    ] {
        let result = run(args);
        assert_eq!(result.code, 2, "{args:?}");
        assert_eq!(result.stdout, "", "{args:?}");
        assert!(
            result.stderr.starts_with("error[usage_error]: "),
            "{args:?}: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("\n  hint: see cleanrules "),
            "{args:?}"
        );
        // As JSON when the arguments ask for it, even when they do not parse.
        let mut with_json = args.to_vec();
        with_json.extend(["--format", "json"]);
        let result = run(&with_json);
        assert_eq!(result.code, 2, "{with_json:?}");
        let error = json(&result.stderr);
        assert_eq!(
            string(get(get(&error, "error").unwrap(), "code")),
            "usage_error"
        );
        assert!(get(&error, "file").is_none());
    }
    insta::assert_snapshot!(
        "usage_error_text",
        run(&["audit", "x.rules", "--frob"]).stderr
    );
    insta::assert_snapshot!(
        "usage_error_json",
        run(&["audit", "x.rules", "--fail-on=fatal", "--format=json"]).stderr
    );
    assert!(
        run(&[])
            .stderr
            .starts_with("error[usage_error]: no command given")
    );
}

/// A document that does not read is what `validate` reports: exit 1, the
/// error on standard output, as the agents' tool gives it in JSON.
#[test]
fn validate_reports_documents_that_do_not_read() {
    let result = run(&["validate", "broken.rules"]);
    assert_eq!(result.code, 1);
    assert_eq!(result.stderr, "");
    insta::assert_snapshot!("validate_invalid_text", result.stdout);
    for (file, syntax) in [
        ("broken.json", "json"),
        ("broken.rules", "text"),
        ("table.csv", "csv"),
        ("unique.rules", "text"),
    ] {
        let result = run(&["validate", file, "--format", "json"]);
        let text = std::fs::read_to_string(data().join(file)).unwrap();
        let call = Json::Object(vec![
            ("document".into(), Json::String(text)),
            ("format".into(), Json::String(syntax.into())),
        ]);
        let tool = cleanrules::agents::Catalog::new()
            .call("validate", &call)
            .content;
        assert_eq!(
            result.stdout,
            format!("{}\n", cleanrules::json::to_string(&tool)),
            "{file}"
        );
        let valid = get(&tool, "valid") == Some(&Json::Bool(true));
        assert_eq!(result.code, i32::from(!valid), "{file}");
    }
    // Bytes that are not UTF-8, which no tool call can carry.
    let result = run_in(
        &data(),
        &["validate", "-", "--format", "json"],
        Some(b"{\"a\xff\": 1}"),
    );
    assert_eq!(result.code, 1);
    let answer = json(&result.stdout);
    assert_eq!(get(&answer, "valid"), Some(&Json::Bool(false)));
    let error = get(&answer, "error").unwrap();
    assert_eq!(string(get(error, "code")), "format_error");
    assert!(get(error, "hint").is_some());
}

/// A rule document has no hit policy of its own: no gap of the default one.
#[test]
fn a_rule_is_audited_without_the_checks_between_rules() {
    let result = run_in(
        &data(),
        &["audit", "-", "--syntax", "text", "--fail-on", "warning"],
        Some(b"RULE a IF x > 1"),
    );
    assert_eq!(result.code, 0, "{}", result.stdout);
    assert_eq!(
        result.stdout,
        "Audit of a rule, logic boolean\nNo findings.\nChecks: types complete, fields not \
         applicable, unsatisfiable complete, shadowed not applicable, overlap not applicable, gap \
         not applicable\n"
    );
}

#[test]
fn help_and_version() {
    let help = run(&["--help"]);
    assert_eq!(help.code, 0);
    insta::assert_snapshot!("help", help.stdout);
    for command in ["validate", "audit", "test", "draw"] {
        let result = run(&[command, "--help"]);
        assert_eq!(result.code, 0);
        insta::assert_snapshot!(format!("help_{command}"), result.stdout);
    }
    let version = run(&["--version"]);
    assert_eq!(version.code, 0);
    assert_eq!(
        version.stdout,
        format!("cleanrules {}\n", env!("CARGO_PKG_VERSION"))
    );
}

// `test` (`spec/cli.md`, *`test`*).

/// The JSON of a test run with the engine's version, which changes with
/// every release, written as `<version>`.
fn without_version(stdout: &str) -> String {
    stdout.replace(
        &format!(r#""version":"{}""#, env!("CARGO_PKG_VERSION")),
        r#""version":"<version>""#,
    )
}

fn test_run(stdout: &str) -> Json {
    let written = json(stdout);
    assert_eq!(
        get(&written, "version")
            .map(cleanrules::json::to_string)
            .as_deref(),
        Some("1")
    );
    get(&written, "test_run").unwrap().clone()
}

/// A member of a test run's summary.
fn count(run: &Json, key: &str) -> String {
    cleanrules::json::to_string(get(get(run, "summary").unwrap(), key).unwrap())
}

/// The example cases pass against the example rules, in every syntax.
#[test]
fn test_runs_the_example_cases() {
    let examples = examples();
    let text = run_in(
        &examples,
        &["test", "discounts.json", "cases/discounts.json"],
        None,
    );
    assert_eq!(text.code, 0, "{}{}", text.stdout, text.stderr);
    assert_eq!(text.stderr, "");
    insta::assert_snapshot!("test_text_examples", text.stdout);
    for rules in ["discounts.yaml", "discounts.rules"] {
        let other = run_in(&examples, &["test", rules, "cases/discounts.json"], None);
        assert_eq!((other.code, &other.stdout), (0, &text.stdout), "{rules}");
    }
    let result = run_in(
        &examples,
        &[
            "test",
            "discounts.json",
            "cases/discounts.json",
            "--format",
            "json",
            "--ruleset-digest",
        ],
        None,
    );
    assert_eq!(result.code, 0);
    assert!(result.stdout.ends_with("}\n") && result.stdout.lines().count() == 1);
    let run = test_run(&result.stdout);
    assert_eq!(count(&run, "total"), "4");
    assert_eq!(count(&run, "passed"), "4");
    let Some(Json::Array(files)) = get(&run, "files") else {
        panic!("no files");
    };
    assert_eq!(string(get(&files[0], "path")), "cases/discounts.json");
    let bytes = std::fs::read(examples.join("cases/discounts.json")).unwrap();
    assert_eq!(
        string(get(&files[0], "digest")),
        cleanrules::digest::Digest::of(&bytes).to_string()
    );
    // The rule set digest of the rules tested, as `audit` carries it.
    let audit = run_in(
        &examples,
        &[
            "audit",
            "discounts.json",
            "--format",
            "json",
            "--ruleset-digest",
        ],
        None,
    );
    assert_eq!(
        get(&run, "ruleset_digest"),
        get(
            get(&json(&audit.stdout), "audit").unwrap(),
            "ruleset_digest"
        )
    );
    // The default limits the case files were read with.
    let engine = get(&run, "engine").unwrap();
    assert_eq!(string(get(engine, "name")), "cleanrules");
    let d = cleanrules::bundle::EngineLimits::default();
    assert_eq!(
        cleanrules::json::to_string(get(engine, "limits").unwrap()),
        format!(
            r#"{{"max_size":{},"max_nesting":{},"max_depth":{},"max_path_segments":{},"max_table_size":{},"max_values":{}}}"#,
            d.max_size,
            d.max_nesting,
            d.max_depth,
            d.max_path_segments,
            d.max_table_size,
            d.max_values
        )
    );
}

/// Failed and erring cases, and files that do not read: exit 1, with the
/// run written whole.
#[test]
fn test_reports_failures_and_errors() {
    let args = [
        "test",
        "unique.rules",
        "cases/ages.json",
        "cases/passing.yaml",
        "cases/broken.json",
    ];
    let result = run(&args);
    assert_eq!(result.code, 1);
    assert_eq!(result.stderr, "");
    insta::assert_snapshot!("test_text_failures", result.stdout);
    let mut json_args = args.to_vec();
    json_args.extend(["--format", "json"]);
    let result = run(&json_args);
    assert_eq!(result.code, 1);
    insta::assert_snapshot!("test_json_failures", without_version(&result.stdout));
    let run_json = test_run(&result.stdout);
    assert_eq!(
        (
            count(&run_json, "passed"),
            count(&run_json, "failed"),
            count(&run_json, "error")
        ),
        ("4".into(), "1".into(), "2".into())
    );
    // No explanations.
    let quiet = run(&[
        "test",
        "unique.rules",
        "cases/ages.json",
        "--max-explained",
        "0",
    ]);
    assert_eq!(quiet.code, 1);
    assert!(!quiet.stdout.contains("explanation:"), "{}", quiet.stdout);
    // Cases that all pass, in YAML, against a rule document read from
    // standard input.
    let yaml = run(&["test", "unique.rules", "cases/passing.yaml"]);
    assert_eq!(yaml.code, 0, "{}", yaml.stdout);
    let rule = run_in(
        &data(),
        &["test", "-", "cases/passing.yaml", "--syntax", "text"],
        Some(b"RULE adult IF age >= 18 THEN band = 'adult'"),
    );
    assert_eq!(rule.code, 0, "{}{}", rule.stdout, rule.stderr);
    assert!(
        rule.stdout
            .starts_with("Test run of a rule set: 1 rule, hit policy first")
    );
}

/// A partial run, and a run without cases, exit with 1.
#[test]
fn test_partial_and_empty_runs_do_not_pass() {
    let examples = examples();
    for (limit, reason) in [
        ("--max-results", "max_results"),
        ("--max-steps", "max_steps"),
    ] {
        let bound = if reason == "max_steps" { "3" } else { "1" };
        let result = run_in(
            &examples,
            &[
                "test",
                "discounts.json",
                "cases/discounts.json",
                limit,
                bound,
            ],
            None,
        );
        assert_eq!(result.code, 1, "{limit}");
        assert!(
            result.stdout.ends_with(&format!(
                "1 result: 1 passed, 0 failed, 0 errors\nPartial run: interrupted by {reason}, 3 not run\n"
            )),
            "{}",
            result.stdout
        );
    }
    // Limits that the run does not reach: complete.
    let result = run_in(
        &examples,
        &[
            "test",
            "discounts.json",
            "cases/discounts.json",
            "--max-results",
            "4",
            "--max-steps",
            "12",
        ],
        None,
    );
    assert_eq!(result.code, 0, "{}", result.stdout);
    let result = run(&["test", "unique.rules", "cases/empty.json"]);
    assert_eq!(result.code, 1);
    assert_eq!(
        result.stdout,
        "Test run of a rule set: 2 rules, hit policy unique, logic boolean\n0 results: 0 \
         passed, 0 failed, 0 errors\n"
    );
    let result = run(&[
        "test",
        "unique.rules",
        "cases/empty.json",
        "--format",
        "json",
    ]);
    assert_eq!(result.code, 1);
    assert_eq!(count(&test_run(&result.stdout), "total"), "0");
}

/// Case files from standard input, given twice, and read from a decision
/// table's rules.
#[test]
fn test_reads_case_files_as_given() {
    let cases = std::fs::read(data().join("cases/passing.yaml")).unwrap();
    let as_json = br#"{"version": 1, "cases": {"items": [{"id": "a", "input": {"age": 20}, "expect": {"selected": ["adult"]}}]}}"#;
    let piped = run_in(
        &data(),
        &["test", "unique.rules", "-", "--format", "json"],
        Some(as_json),
    );
    assert_eq!(piped.code, 0, "{}{}", piped.stdout, piped.stderr);
    let run_json = test_run(&piped.stdout);
    let Some(Json::Array(results)) = get(&run_json, "results") else {
        panic!("no results");
    };
    // No path: standard input has none.
    assert!(get(&results[0], "path").is_none());
    let Some(Json::Array(files)) = get(&run_json, "files") else {
        panic!("no files");
    };
    assert_eq!(
        string(get(&files[0], "digest")),
        cleanrules::digest::Digest::of(as_json).to_string()
    );
    // YAML on standard input is not read as a case file: it is JSON.
    let piped = run_in(&data(), &["test", "unique.rules", "-"], Some(&cases));
    assert_eq!(piped.code, 1);
    assert!(
        piped
            .stdout
            .contains("ERROR   (a case file without a path)"),
        "{}",
        piped.stdout
    );
    // The same file twice: the second is an error, and none of its cases runs.
    let twice = run(&[
        "test",
        "unique.rules",
        "cases/passing.yaml",
        "cases/passing.yaml",
    ]);
    assert_eq!(twice.code, 1);
    assert!(
        twice
            .stdout
            .ends_with("3 results: 2 passed, 0 failed, 1 error\n"),
        "{}",
        twice.stdout
    );
    // A decision table's rules.
    let table = run_in(
        &examples(),
        &["test", "risk.csv", "-", "--format", "json"],
        Some(br#"{"version": 1, "cases": {"items": [{"id": "x", "input": {}}]}}"#),
    );
    assert_eq!(
        string(get(&test_run(&table.stdout), "source")),
        "decision_table"
    );
    // A suffix is compared without case.
    let upper = scratch("PASSING.YML");
    std::fs::write(&upper, &cases).unwrap();
    let result = run(&["test", "unique.rules", upper.to_str().unwrap()]);
    assert_eq!(result.code, 0, "{}", result.stdout);
}

#[test]
fn test_case_files_larger_than_the_limit_are_results() {
    let big = scratch("big.json");
    std::fs::write(&big, vec![b' '; 16 * 1024 * 1024 + 10]).unwrap();
    let result = run(&[
        "test",
        "unique.rules",
        big.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(result.code, 1, "{}", result.stderr);
    let run_json = test_run(&result.stdout);
    let Some(Json::Array(results)) = get(&run_json, "results") else {
        panic!("no results");
    };
    assert_eq!(
        string(get(get(&results[0], "error").unwrap(), "code")),
        "limit_exceeded"
    );
    let Some(Json::Array(files)) = get(&run_json, "files") else {
        panic!("no files");
    };
    assert!(get(&files[0], "digest").is_none());
}

/// What stops `test`: usage errors (2), and files it cannot open or rules
/// that do not read (3), with nothing on standard output.
#[test]
fn test_errors() {
    for (args, code, error) in [
        (&["test", "unique.rules"][..], 2, "usage_error"),
        (&["test", "unique.rules", "notes.txt"][..], 2, "usage_error"),
        (
            &["test", "unique.rules", "clean.rules"][..],
            2,
            "usage_error",
        ),
        (&["test", "-", "-"][..], 2, "usage_error"),
        (
            &["test", "unique.rules", "cases/missing.json"][..],
            3,
            "io_error",
        ),
        (
            &["test", "missing.rules", "cases/ages.json"][..],
            3,
            "io_error",
        ),
        (
            &["test", "broken.rules", "cases/ages.json"][..],
            3,
            "rule_syntax_error",
        ),
        (
            &["test", "condition.rules", "cases/ages.json"][..],
            3,
            "format_error",
        ),
        (
            &["test", "cases/ages.json", "unique.rules"][..],
            2,
            "usage_error",
        ),
        (
            &["test", "cases/empty.json", "cases/ages.json"][..],
            3,
            "format_error",
        ),
        (
            &[
                "test",
                "unique.rules",
                "cases/ages.json",
                "--max-results",
                "x",
            ][..],
            2,
            "usage_error",
        ),
    ] {
        let result = run(args);
        assert_eq!(result.code, code, "{args:?}: {}", result.stderr);
        assert_eq!(result.stdout, "", "{args:?}");
        assert!(
            result.stderr.starts_with(&format!("error[{error}]: ")),
            "{args:?}: {}",
            result.stderr
        );
        let mut json_args = args.to_vec();
        json_args.extend(["--format", "json"]);
        let result = run(&json_args);
        assert_eq!(result.code, code, "{json_args:?}");
        assert_eq!(
            string(get(get(&json(&result.stderr), "error").unwrap(), "code")),
            error
        );
    }
    insta::assert_snapshot!("test_error_no_cases", run(&["test", "unique.rules"]).stderr);
    insta::assert_snapshot!(
        "test_error_missing_case_file",
        run(&[
            "test",
            "unique.rules",
            "cases/missing.json",
            "--format",
            "json"
        ])
        .stderr
        .replace("No such file or directory (os error 2)", "<os error>")
        .replace(
            "The system cannot find the file specified. (os error 2)",
            "<os error>"
        )
    );
}

/// A bundle's own case files, at their paths relative to the bundle's
/// folder and checked against their digests.
#[test]
fn test_runs_a_bundles_case_files() {
    let folder = scratch("bundle-test");
    std::fs::create_dir_all(folder.join("cases")).unwrap();
    let passing = std::fs::read(data().join("cases/passing.yaml")).unwrap();
    let ages = std::fs::read(data().join("cases/ages.json")).unwrap();
    let rules = cleanrules::text::parse(
        &std::fs::read_to_string(data().join("unique.rules")).unwrap(),
        &Limits::default(),
    )
    .unwrap()
    .into_ruleset()
    .unwrap();
    let make = |cases: &[(&str, &[u8])]| {
        Bundle::new(
            rules.clone(),
            BundleId::new("acme/ages").unwrap(),
            Revision::new("7").unwrap(),
        )
        .with_cases(
            cases
                .iter()
                .map(|(path, bytes)| CaseFileRef::new(path, bytes).unwrap()),
        )
        .unwrap()
        .into_document()
    };
    let write = |name: &str, cases: &[(&str, &[u8])]| {
        let path = folder.join(name);
        std::fs::write(
            &path,
            write_document_as(&make(cases), DocumentFormat::Json).unwrap(),
        )
        .unwrap();
        path
    };
    std::fs::write(folder.join("cases/passing.yaml"), &passing).unwrap();
    let bundle = write("bundle.json", &[("cases/passing.yaml", &passing)]);
    // From another folder: the paths are the bundle's.
    let result = run(&["test", bundle.to_str().unwrap(), "--format", "json"]);
    assert_eq!(result.code, 0, "{}{}", result.stdout, result.stderr);
    let run_json = test_run(&result.stdout);
    assert_eq!(
        cleanrules::json::to_string(get(&run_json, "bundle").unwrap()),
        r#"{"id":"acme/ages","revision":"7"}"#
    );
    let Some(Json::Array(results)) = get(&run_json, "results") else {
        panic!("no results");
    };
    assert_eq!(string(get(&results[0], "path")), "cases/passing.yaml");
    // From standard input: relative to the current folder.
    let piped = run_in(
        &folder,
        &["test", "-"],
        Some(&std::fs::read(&bundle).unwrap()),
    );
    assert_eq!(piped.code, 0, "{}{}", piped.stdout, piped.stderr);
    // A case file that is not the one the bundle was made with.
    let changed = write("changed.json", &[("cases/passing.yaml", &ages)]);
    let result = run(&["test", changed.to_str().unwrap()]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stdout
            .contains("decision_mismatch at /bundle/cases/0/digest"),
        "{}",
        result.stdout
    );
    // A case file that is not there.
    let missing = write("missing.json", &[("cases/ages.json", &ages)]);
    let result = run(&["test", missing.to_str().unwrap()]);
    assert_eq!((result.code, result.stdout.as_str()), (3, ""));
    assert!(
        result.stderr.starts_with("error[io_error]: cannot read "),
        "{}",
        result.stderr
    );
    assert!(
        result.stderr.contains(&format!(
            "hint: the bundle {} lists it at /bundle/cases/0/path, relative to the bundle's \
             folder, or the current folder when the bundle comes from standard input",
            missing.display()
        )),
        "{}",
        result.stderr
    );
    // No case files: nothing tested, and a note says so.
    let empty = write("empty.json", &[]);
    let result = run(&["test", empty.to_str().unwrap()]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.starts_with("note[no_cases]: no cases ran"));
    assert!(
        result
            .stdout
            .ends_with("0 results: 0 passed, 0 failed, 0 errors\n")
    );
    // Other case files: the bundle's rules on them, without the bundle.
    let other = run(&[
        "test",
        missing.to_str().unwrap(),
        "cases/passing.yaml",
        "--format",
        "json",
    ]);
    assert_eq!(other.code, 0, "{}{}", other.stdout, other.stderr);
    assert!(get(&test_run(&other.stdout), "bundle").is_none());
}

/// Bytes beyond the size limit are refused with `limit_exceeded`, whatever
/// byte the cut lands on, with the file's real size, or "more bytes than"
/// the limit for standard input.
#[test]
fn larger_documents_are_refused_whatever_their_bytes() {
    // A YAML file of 1 MiB and some, whose cut lands inside an `é`.
    let limit = 1024 * 1024;
    let mut yaml = b"version: 1\nnote: '".to_vec();
    yaml.resize(limit - 1, b'a');
    yaml.extend("ééé'\n".as_bytes());
    let path = scratch("big.yaml");
    std::fs::write(&path, &yaml).unwrap();
    let result = run(&["audit", path.to_str().unwrap()]);
    assert_eq!(result.code, 3);
    assert!(
        result.stderr.starts_with(&format!(
            "error[limit_exceeded]: the YAML text has {} bytes, more than the limit of {limit}\n",
            yaml.len()
        )),
        "{}",
        result.stderr
    );
    let piped = run_in(&data(), &["audit", "-", "--syntax", "yaml"], Some(&yaml));
    assert!(
        piped.stderr.starts_with(&format!(
            "error[limit_exceeded]: the YAML text has more bytes than the limit of {limit}\n"
        )),
        "{}",
        piped.stderr
    );
    // validate in JSON gives the reader's error too.
    let result = run(&["validate", path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(result.code, 1);
    let answer = json(&result.stdout);
    assert_eq!(get(&answer, "valid"), Some(&Json::Bool(false)));
    assert_eq!(
        string(get(get(&answer, "error").unwrap(), "code")),
        "limit_exceeded"
    );
    // A case file: its result, with its size.
    let big = scratch("big-cases.yaml");
    std::fs::write(&big, &yaml).unwrap();
    let result = run(&["test", "unique.rules", big.to_str().unwrap()]);
    assert_eq!(result.code, 1);
    assert!(
        result.stdout.contains(&format!(
            "limit_exceeded: the YAML text has {} bytes, more than the limit of {limit}",
            yaml.len()
        )),
        "{}",
        result.stdout
    );
}

/// The path recorded for a case file has no leading `./`; an empty run
/// writes its note as JSON under `--format json`.
#[test]
fn test_records_paths_and_notes() {
    let result = run(&[
        "test",
        "unique.rules",
        "./cases/passing.yaml",
        "--format",
        "json",
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let run_json = test_run(&result.stdout);
    let Some(Json::Array(results)) = get(&run_json, "results") else {
        panic!("no results");
    };
    assert_eq!(string(get(&results[0], "path")), "cases/passing.yaml");
    let result = run(&[
        "test",
        "unique.rules",
        "cases/empty.json",
        "--format",
        "json",
    ]);
    assert_eq!(result.code, 1);
    insta::assert_snapshot!("test_note_json", result.stderr);
    let note = json(&result.stderr);
    assert_eq!(string(get(get(&note, "note").unwrap(), "code")), "no_cases");
}

/// The bundle of the example rules and cases that the example CI workflow
/// runs (`docs/examples/ci/cleanrules-audit.yml`), which finds bundles by
/// the compact JSON of `validate`.
#[test]
fn the_example_bundle_runs_its_cases() {
    let result = run(&["test", "bundle/discounts.json"]);
    assert_eq!(result.code, 0, "{}{}", result.stdout, result.stderr);
    assert!(
        result
            .stdout
            .ends_with("4 results: 4 passed, 0 failed, 0 errors\n")
    );
    let validated = run(&["validate", "bundle/discounts.json", "--format", "json"]);
    assert!(
        validated.stdout.contains(r#""content":"bundle""#),
        "{}",
        validated.stdout
    );
}
