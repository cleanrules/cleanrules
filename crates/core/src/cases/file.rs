//! One case file to run (`spec/tests.md`, *Running cases*).
//!
//! Three names, the same in every language: a [`CaseFileRef`] is a bundle's
//! reference to a case file (its path and the digest of its bytes); a
//! [`Cases`] is the content of a case file document; a [`CaseFile`] is one
//! file given to a run: its path, when it has one, and its cases, or the
//! error that keeps it from being read.

use std::sync::Arc;

use super::{Case, Cases};
use crate::bundle::{Bundle, CaseFileRef};
use crate::digest::Digest;
use crate::error::{Error, ErrorCode, Result};
use crate::format::{Document, DocumentFormat, read_document_as, size_limit, too_large};
use crate::json::Limits;

/// What a caller loaded of a case file, up to the bound
/// [`CaseFile::bundle_files_bounded`] gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaseFileBytes {
    /// Every byte of the file.
    Whole(Vec<u8>),
    /// The file has more bytes than the bound, and was not loaded whole: its
    /// size in bytes, when the caller knows it.
    TooLarge {
        /// The file's size, in bytes, when the caller knows it.
        size: Option<u64>,
    },
}

/// One case file given to a run: its path, when it has one, the digest of
/// the bytes it was read from, when it was, and its cases or the error of
/// reading it, which running it gives as its one result.
#[derive(Debug)]
pub struct CaseFile {
    path: Option<String>,
    digest: Option<Digest>,
    /// Shared, so that a copy of the file is cheap.
    content: Result<Arc<Cases>>,
}

/// A copy of the case file, the error of one that cannot be read included
/// (as [`crate::Error`] is not `Clone`, the copy keeps its cause as a
/// message): a binding keeps its case files and gives them to more than one
/// run.
impl Clone for CaseFile {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            digest: self.digest,
            content: match &self.content {
                Ok(cases) => Ok(Arc::clone(cases)),
                Err(error) => Err(error.duplicate()),
            },
        }
    }
}

impl CaseFile {
    /// A case file already read, without a path.
    #[must_use]
    pub fn new(cases: Cases) -> Self {
        Self {
            path: None,
            digest: None,
            content: Ok(Arc::new(cases)),
        }
    }

    /// This case file, at `path`.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The case file at `path` whose bytes are `bytes`: read as YAML when the
    /// path ends in `.yaml` or `.yml` (in any ASCII case), and as JSON
    /// otherwise (`spec/tests.md`, *Running cases*). An error reading it is
    /// kept, and becomes the file's result when it is run.
    #[must_use]
    pub fn read(path: &str, bytes: &[u8], limits: &Limits) -> Self {
        Self::read_as(Some(path), bytes, format_of(path), limits)
    }

    /// The case file whose bytes are `bytes`, in `format` (JSON or YAML),
    /// with a path or without one.
    #[must_use]
    pub fn read_as(
        path: Option<&str>,
        bytes: &[u8],
        format: DocumentFormat,
        limits: &Limits,
    ) -> Self {
        let content = read_document_as(bytes, format, limits).and_then(|document| match document {
            Document::Cases(cases) => Ok(Arc::new(cases)),
            other => Err(not_cases(&other)),
        });
        Self {
            path: path.map(str::to_owned),
            digest: Some(Digest::of(bytes)),
            content,
        }
    }

    /// A case file larger than the size limit of `format`, which its caller
    /// did not load whole (`spec/cli.md`, *`test`*): its result is the
    /// reader's `limit_exceeded`, with the file's `size` when the caller
    /// knows it, and it has no digest, since its bytes were not all read.
    #[must_use]
    pub fn too_large(
        path: Option<&str>,
        format: DocumentFormat,
        size: Option<u64>,
        limits: &Limits,
    ) -> Self {
        Self::unreadable(path, too_large(format, size, limits))
    }

    /// A case file that could not be read, with the error that says why: the
    /// caller's own, such as a command line that goes on when it cannot open
    /// a file. Running it gives that error as its one result.
    #[must_use]
    pub fn unreadable(path: Option<&str>, error: Error) -> Self {
        Self {
            path: path.map(str::to_owned),
            digest: None,
            content: Err(error),
        }
    }

    /// The case files of a bundle, in its order (`spec/tests.md`, *Case files
    /// of a bundle*), read under the limits the bundle records: `read` gives
    /// the bytes of each, since the core never opens files, and a file whose
    /// bytes are not the ones the bundle records keeps a `decision_mismatch`.
    ///
    /// # Errors
    ///
    /// The first error `read` returns: a file the caller cannot open is the
    /// caller's error, not a result.
    pub fn bundle_files<E>(
        bundle: &Bundle,
        mut read: impl FnMut(&CaseFileRef) -> Result<Vec<u8>, E>,
    ) -> Result<Vec<CaseFile>, E> {
        let limits = bundle.engine().limits().to_limits();
        bundle
            .cases()
            .iter()
            .enumerate()
            .map(|(index, case_ref)| Ok(Self::of_ref(case_ref, index, &read(case_ref)?, &limits)))
            .collect()
    }

    /// The case files of a bundle, as [`CaseFile::bundle_files`] gives them,
    /// from a caller that loads each file up to a bound (`spec/cli.md`,
    /// *`test`*): `load` is given the reference and the most bytes to load,
    /// the larger of the size limit the bundle records for the file's syntax
    /// and the default one, so that a file the bundle's limit refuses is
    /// still checked against its digest first. A file with more bytes than
    /// the bound gives `limit_exceeded` ([`CaseFile::too_large`]) without its
    /// digest being checked; every other file is checked and read as
    /// [`CaseFile::bundle_files`] does.
    ///
    /// # Errors
    ///
    /// The first error `load` returns: a file the caller cannot open is the
    /// caller's error, not a result.
    pub fn bundle_files_bounded<E>(
        bundle: &Bundle,
        mut load: impl FnMut(&CaseFileRef, usize) -> Result<CaseFileBytes, E>,
    ) -> Result<Vec<CaseFile>, E> {
        let limits = bundle.engine().limits().to_limits();
        let defaults = Limits::default();
        bundle
            .cases()
            .iter()
            .enumerate()
            .map(|(index, case_ref)| {
                let format = format_of(case_ref.path());
                let bound = size_limit(format, &limits).max(size_limit(format, &defaults));
                Ok(match load(case_ref, bound)? {
                    CaseFileBytes::Whole(bytes) if bytes.len() <= bound => {
                        Self::of_ref(case_ref, index, &bytes, &limits)
                    }
                    CaseFileBytes::Whole(bytes) => Self::too_large(
                        Some(case_ref.path()),
                        format,
                        u64::try_from(bytes.len()).ok(),
                        &limits,
                    ),
                    CaseFileBytes::TooLarge { size } => {
                        Self::too_large(Some(case_ref.path()), format, size, &limits)
                    }
                })
            })
            .collect()
    }

    /// The case file a bundle names at `path`, from its bytes, read under the
    /// limits the bundle records; `None` when the bundle names no such file.
    #[must_use]
    pub fn bundle_file(bundle: &Bundle, path: &str, bytes: &[u8]) -> Option<Self> {
        let limits = bundle.engine().limits().to_limits();
        bundle
            .cases()
            .iter()
            .enumerate()
            .find(|(_, case_ref)| case_ref.path() == path)
            .map(|(index, case_ref)| Self::of_ref(case_ref, index, bytes, &limits))
    }

    /// The case file of `case_ref`, at `index` in its bundle's `cases`.
    fn of_ref(case_ref: &CaseFileRef, index: usize, bytes: &[u8], limits: &Limits) -> Self {
        if case_ref.digest_matches(bytes) {
            return Self::read(case_ref.path(), bytes, limits);
        }
        let digest = Digest::of(bytes);
        let error = Error::new(
            ErrorCode::DecisionMismatch,
            format!(
                "the case file {} is not the one the bundle names: its digest is {digest}, not {}",
                case_ref.path(),
                case_ref.digest()
            ),
        )
        .with_hint("run the cases the bundle was made with, or make a new bundle")
        .at_pointer(format!("/bundle/cases/{index}/digest"));
        Self {
            path: Some(case_ref.path().to_owned()),
            digest: Some(digest),
            content: Err(error),
        }
    }

    /// The path, when the file has one.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The SHA-256 digest of the bytes it was read from, when it was.
    #[must_use]
    pub fn digest(&self) -> Option<Digest> {
        self.digest
    }

    /// The cases, or the error of reading the file.
    ///
    /// # Errors
    ///
    /// The error of reading the file, which running it gives as its result.
    pub fn cases(&self) -> std::result::Result<&Cases, &Error> {
        self.content.as_ref().map(|cases| &**cases)
    }

    /// The case with the id `id`, when the file was read and has it: with
    /// its path, a result names it (`spec/tests.md`, *Identity*).
    #[must_use]
    pub fn case(&self, id: &str) -> Option<&Case> {
        self.content.as_ref().ok()?.get(id)
    }

    /// The results running it alone gives: one per case, or one for a file
    /// that cannot be read.
    pub(crate) fn results(&self) -> usize {
        self.content.as_ref().map_or(1, |cases| cases.len())
    }
}

/// The syntax of a case file at `path`: YAML when its last segment ends in
/// `.yaml` or `.yml`, in any ASCII case, and JSON otherwise.
fn format_of(path: &str) -> DocumentFormat {
    let name = path.rsplit('/').next().unwrap_or(path);
    let yaml = name.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
    });
    if yaml {
        DocumentFormat::Yaml
    } else {
        DocumentFormat::Json
    }
}

/// The error of a document that is not a case file.
fn not_cases(document: &Document) -> Error {
    let what = match document {
        Document::Condition(_) => "a condition",
        Document::Rule(_) => "a rule",
        Document::RuleSet(_) => "a rule set",
        Document::DecisionTable(_) => "a decision table",
        Document::Bundle(_) => "a bundle",
        Document::Cases(_) => "test cases",
    };
    Error::new(
        ErrorCode::FormatError,
        format!("this document holds {what}, not test cases"),
    )
    .with_hint("give a case file: {\"version\": 1, \"cases\": {\"items\": [...]}}")
    .at_pointer("")
}
