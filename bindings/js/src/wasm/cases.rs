//! The wasm-bindgen side of test cases and the case runner for the idiomatic
//! API, with the names, arguments and errors of the napi-rs build's
//! (`src/napi/cases.rs`). Internal: no entry point exports them.

use js_sys::{Function, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use super::classes::{Document, RuleSet};
use super::documents::NativeTable;
use super::{OrFail, fail};
use crate::common::cases::{self, CaseFileCore, RunCore, run_limits};
use crate::common::no_constructor;

/// A case file's content built from the JavaScript half's encoding, as in
/// the napi-rs build.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = casesNew)]
pub fn cases_new(text: &str) -> Result<Document, JsValue> {
    cases::cases_new(text)
        .map(|core| Document { core })
        .or_fail()
}

/// What the idiomatic `Cases` reads of a case file document, as in the
/// napi-rs build.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = casesData)]
pub fn cases_data(document: &Document) -> Result<String, JsValue> {
    cases::cases_data(&document.core).or_fail()
}

/// One case file given to a run.
#[wasm_bindgen]
pub struct NativeCaseFile {
    core: CaseFileCore,
}

#[wasm_bindgen]
impl NativeCaseFile {
    /// Not a constructor: case files come from `caseFileRead` and
    /// `caseFileOf`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<NativeCaseFile, JsValue> {
        Err(fail(no_constructor("NativeCaseFile")))
    }

    /// `[path, digest, error]`, as JSON.
    #[must_use]
    pub fn data(&self) -> String {
        self.core.data()
    }

    /// Its case file document, or `undefined` for a file that cannot be read.
    #[must_use]
    pub fn cases(&self) -> Option<Document> {
        self.core.cases().map(|core| Document { core })
    }
}

/// The case file whose bytes are `bytes`, as in the napi-rs build.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = caseFileRead)]
pub fn case_file_read(
    path: Option<String>,
    bytes: &[u8],
    format: Option<String>,
) -> Result<NativeCaseFile, JsValue> {
    CaseFileCore::read(path.as_deref(), bytes, format.as_deref())
        .map(|core| NativeCaseFile { core })
        .or_fail()
}

/// The case file of a case file document, as in the napi-rs build.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = caseFileOf)]
pub fn case_file_of(document: &Document, path: Option<String>) -> Result<NativeCaseFile, JsValue> {
    CaseFileCore::of(&document.core, path)
        .map(|core| NativeCaseFile { core })
        .or_fail()
}

/// A run in progress, over the rules and case files it owns.
#[wasm_bindgen]
pub struct NativeCaseRun {
    core: RunCore,
}

#[wasm_bindgen]
impl NativeCaseRun {
    /// Not a constructor: runs come from `runRuleset`, `runTable` and
    /// `runBundle`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<NativeCaseRun, JsValue> {
        Err(fail(no_constructor("NativeCaseRun")))
    }

    /// Give the run one more case file, before its first result.
    ///
    /// # Errors
    ///
    /// After the first result.
    pub fn add(&mut self, file: &NativeCaseFile) -> Result<(), JsValue> {
        self.core.add(&file.core).or_fail()
    }

    /// Give a bundle's run the bundle's case files, as in the napi-rs build.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = addBundleFiles)]
    pub fn add_bundle_files(&mut self, load: &Function) -> Result<(), JsValue> {
        // A bound is at most the size limit of a document, far below 2^53.
        #[allow(clippy::cast_precision_loss)]
        let outcome = self.core.add_bundle_files(|path, bound| {
            load.call2(
                &JsValue::NULL,
                &JsValue::from_str(path),
                &JsValue::from_f64(bound as f64),
            )
            .map(|bytes| Uint8Array::new(&bytes).to_vec())
        })?;
        outcome.or_fail()
    }

    /// Run the rest of the cases, as in the napi-rs build.
    pub fn finish(&mut self, cancel: &JsValue) {
        let cancel = cancel.dyn_ref::<Function>();
        self.core.finish(|| {
            cancel.is_some_and(|cancel| {
                cancel
                    .call0(&JsValue::NULL)
                    .map_or(true, |answer| answer.as_bool().unwrap_or(true))
            })
        });
    }

    /// The next result, as JSON, or `undefined` once the run is finished;
    /// `cancel` is asked as in the napi-rs build.
    pub fn next(&mut self, cancel: &JsValue) -> Option<String> {
        let cancel = cancel.dyn_ref::<Function>();
        self.core.next(|| {
            cancel.is_some_and(|cancel| {
                // A call that fails cannot be answered: the run stops.
                cancel
                    .call0(&JsValue::NULL)
                    .map_or(true, |answer| answer.as_bool().unwrap_or(true))
            })
        })
    }

    /// Stop the run.
    pub fn stop(&mut self) {
        self.core.stop();
    }

    /// The summary of the results given so far, as JSON.
    #[must_use]
    pub fn summary(&self) -> String {
        self.core.summary()
    }

    /// The results not given yet.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // A count, far below 2^53.
    pub fn remaining(&self) -> f64 {
        self.core.remaining() as f64
    }

    /// Whether the run gives no more results.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.core.is_finished()
    }

    /// The test run, made once, as JSON.
    #[wasm_bindgen(js_name = testRun)]
    pub fn test_run(&mut self) -> String {
        self.core.test_run()
    }
}

/// A run against a rule set, as in the napi-rs build.
#[wasm_bindgen(js_name = runRuleset)]
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

/// A run against a decision table, as in the napi-rs build.
#[wasm_bindgen(js_name = runTable)]
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

/// A run of a bundle's case files, as in the napi-rs build.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = runBundle)]
pub fn run_bundle(
    document: &Document,
    max_results: Option<f64>,
    max_steps: Option<f64>,
    max_explained: Option<f64>,
    ruleset_digest: bool,
) -> Result<NativeCaseRun, JsValue> {
    let limits = run_limits(max_results, max_steps, max_explained);
    RunCore::of_bundle(&document.core, limits, ruleset_digest)
        .map(|core| NativeCaseRun { core })
        .or_fail()
}
