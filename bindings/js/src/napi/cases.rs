//! The napi-rs side of test cases and the case runner for the idiomatic API
//! (`lib/api/cases.js`), over `common::cases`.
//!
//! Internal, like `build.rs`: the addon exports these functions and classes,
//! but no entry point does. The WebAssembly build has the same ones
//! (`src/wasm/cases.rs`), with the same checks and errors.

use napi::bindgen_prelude::{FnArgs, Function, Uint8Array};
use napi::{Env, Result};
use napi_derive::napi;

use super::classes::{Document, RuleSet};
use super::documents::NativeTable;
use super::{OrFail, fail};
use crate::common::cases::{self, CaseFileCore, RunCore, run_limits};
use crate::common::no_constructor;

/// A case file's content built from the JavaScript half's encoding, as a
/// case file document (`common::cases::cases_new`).
///
/// # Errors
///
/// `FormatError` for what a case file cannot hold, as the reader refuses it.
#[napi]
pub fn cases_new(env: &Env, text: String) -> Result<Document> {
    cases::cases_new(&text)
        .map(|core| Document { core })
        .or_fail(env)
}

/// What the idiomatic `Cases` reads of a case file document, as JSON.
///
/// # Errors
///
/// For a document of another kind.
#[napi]
pub fn cases_data(env: &Env, document: &Document) -> Result<String> {
    cases::cases_data(&document.core).or_fail(env)
}

/// One case file given to a run: its path and its cases, or the error of
/// reading it.
#[napi]
pub struct NativeCaseFile {
    core: CaseFileCore,
}

#[napi]
impl NativeCaseFile {
    /// Not a constructor: case files come from `caseFileRead` and
    /// `caseFileOf`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, no_constructor("NativeCaseFile")))
    }

    /// `[path, digest, error]`, as JSON.
    #[napi]
    #[must_use]
    pub fn data(&self) -> String {
        self.core.data()
    }

    /// Its case file document, or `null` for a file that cannot be read.
    #[napi]
    #[must_use]
    pub fn cases(&self) -> Option<Document> {
        self.core.cases().map(|core| Document { core })
    }
}

/// The case file whose bytes are `bytes`: as YAML or JSON by its path's
/// extension unless `format` says; the error of reading it is kept.
///
/// # Errors
///
/// A `RangeError` for a format other than `"json"` and `"yaml"`.
#[napi]
pub fn case_file_read(
    env: &Env,
    path: Option<String>,
    bytes: Uint8Array,
    format: Option<String>,
) -> Result<NativeCaseFile> {
    CaseFileCore::read(path.as_deref(), &bytes, format.as_deref())
        .map(|core| NativeCaseFile { core })
        .or_fail(env)
}

/// The case file of a case file document, with a path or without one.
///
/// # Errors
///
/// For a document of another kind.
#[napi]
pub fn case_file_of(
    env: &Env,
    document: &Document,
    path: Option<String>,
) -> Result<NativeCaseFile> {
    CaseFileCore::of(&document.core, path)
        .map(|core| NativeCaseFile { core })
        .or_fail(env)
}

/// A run in progress, over the rules and case files it owns.
#[napi]
pub struct NativeCaseRun {
    core: RunCore,
}

#[napi]
impl NativeCaseRun {
    /// Not a constructor: runs come from `runRuleset`, `runTable` and
    /// `runBundle`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, no_constructor("NativeCaseRun")))
    }

    /// Give the run one more case file, before its first result.
    ///
    /// # Errors
    ///
    /// After the first result.
    #[napi]
    pub fn add(&mut self, env: &Env, file: &NativeCaseFile) -> Result<()> {
        self.core.add(&file.core).or_fail(env)
    }

    /// Give a bundle's run the bundle's case files, in its order: `load`
    /// is given each file's path and the most bytes to load (the core's
    /// bound) and returns its bytes; the core checks each one against its
    /// digest. What `load` throws is thrown.
    ///
    /// # Errors
    ///
    /// What `load` throws; for a run that is not a bundle's.
    #[napi(ts_args_type = "load: (path: string, bound: number) => Uint8Array")]
    pub fn add_bundle_files(
        &mut self,
        env: &Env,
        load: Function<'_, FnArgs<(String, f64)>, Uint8Array>,
    ) -> Result<()> {
        // A bound is at most the size limit of a document, far below 2^53.
        #[allow(clippy::cast_precision_loss)]
        let outcome = self.core.add_bundle_files(|path, bound| {
            load.call(FnArgs::from((path.to_owned(), bound as f64)))
                .map(|bytes| bytes.to_vec())
        })?;
        outcome.or_fail(env)
    }

    /// Run the rest of the cases without giving the results one at a time;
    /// `cancel` is asked as `next` asks it.
    #[napi(ts_args_type = "cancel?: (() => boolean) | null")]
    pub fn finish(&mut self, cancel: Option<Function<'_, (), bool>>) {
        self.core.finish(|| {
            cancel
                .as_ref()
                .is_some_and(|cancel| cancel.call(()).unwrap_or(true))
        });
    }

    /// The next result, as JSON, or `null` once the run is finished.
    /// `cancel`, a function that never throws and returns a boolean (the
    /// JavaScript half wraps the caller's), is asked before the item, when
    /// the core asks.
    #[napi(ts_args_type = "cancel?: (() => boolean) | null")]
    pub fn next(&mut self, cancel: Option<Function<'_, (), bool>>) -> Option<String> {
        self.core.next(|| {
            cancel
                .as_ref()
                // A call that fails cannot be answered: the run stops.
                .is_some_and(|cancel| cancel.call(()).unwrap_or(true))
        })
    }

    /// Stop the run: it gives no more results, and is partial (cancelled)
    /// when a result remained.
    #[napi]
    pub fn stop(&mut self) {
        self.core.stop();
    }

    /// The summary of the results given so far, as JSON.
    #[napi]
    #[must_use]
    pub fn summary(&self) -> String {
        self.core.summary()
    }

    /// The results not given yet.
    #[napi]
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // A count, far below 2^53.
    pub fn remaining(&self) -> f64 {
        self.core.remaining() as f64
    }

    /// Whether the run gives no more results.
    #[napi]
    #[must_use]
    pub fn finished(&self) -> bool {
        self.core.is_finished()
    }

    /// The test run, made once, as JSON; a run that is not finished is
    /// stopped first.
    #[napi]
    pub fn test_run(&mut self) -> String {
        self.core.test_run()
    }
}

/// A run against a rule set, with limits the JavaScript half checked.
#[napi]
#[must_use]
pub fn run_ruleset(
    ruleset: &RuleSet,
    max_results: Option<f64>,
    max_steps: Option<f64>,
    max_explained: Option<f64>,
    ruleset_digest: bool,
) -> NativeCaseRun {
    let limits = run_limits(max_results, max_steps, max_explained);
    NativeCaseRun {
        core: RunCore::of_ruleset(&ruleset.core, limits, ruleset_digest),
    }
}

/// A run against a decision table.
#[napi]
#[must_use]
pub fn run_table(
    table: &NativeTable,
    max_results: Option<f64>,
    max_steps: Option<f64>,
    max_explained: Option<f64>,
    ruleset_digest: bool,
) -> NativeCaseRun {
    let limits = run_limits(max_results, max_steps, max_explained);
    NativeCaseRun {
        core: RunCore::of_table(&table.core, limits, ruleset_digest),
    }
}

/// A run of a bundle's case files (`NativeCaseRun.addBundleFiles`) against
/// its rules.
///
/// # Errors
///
/// For a document that is not a bundle.
#[napi]
pub fn run_bundle(
    env: &Env,
    document: &Document,
    max_results: Option<f64>,
    max_steps: Option<f64>,
    max_explained: Option<f64>,
    ruleset_digest: bool,
) -> Result<NativeCaseRun> {
    let limits = run_limits(max_results, max_steps, max_explained);
    RunCore::of_bundle(&document.core, limits, ruleset_digest)
        .map(|core| NativeCaseRun { core })
        .or_fail(env)
}
