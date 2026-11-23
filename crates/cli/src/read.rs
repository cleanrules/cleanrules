//! Reading documents (`spec/cli.md`, *Reading documents*): the core's
//! readers, as the agents' `validate` tool reads a document.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use cleanrules::format::{Document, DocumentFormat, read_document_as};
use cleanrules::json::Limits;

use crate::args::{Source, Syntax};
use crate::failure::Failure;

/// The bytes of a document, before they are read as one.
pub(crate) struct Loaded {
    /// The bytes, at most one more than the size limit of their syntax.
    pub(crate) bytes: Vec<u8>,
    /// Their syntax.
    pub(crate) format: DocumentFormat,
    /// The file as it was given (`-` for standard input).
    pub(crate) file: String,
}

impl Loaded {
    /// Read the bytes as a document of their syntax.
    pub(crate) fn document(&self, limits: &Limits) -> Result<Document, Failure> {
        read_document_as(&self.bytes, self.format, limits).map_err(|error| Failure::Core {
            file: Some(self.file.clone()),
            error,
        })
    }
}

/// Load the bytes of `source`'s document and tell their syntax.
pub(crate) fn load(source: &Source, limits: &Limits) -> Result<Loaded, Failure> {
    let file = source.file.to_string_lossy().into_owned();
    let stdin = file == "-";
    let format = match source.syntax {
        Some(syntax) => syntax.format(),
        None if stdin => DocumentFormat::Json,
        None => syntax_of(&source.file)
            .ok_or_else(|| Failure::Usage {
                message: format!("cannot tell the syntax of {file} from its suffix"),
                hint: "name it .json, .yaml, .yml, .rules or .csv, or give --syntax json, \
                       yaml, text or csv"
                    .to_owned(),
            })?
            .format(),
    };
    // One byte more than the limit, so the reader refuses a larger file
    // without it being read whole.
    let largest = match format {
        DocumentFormat::Yaml => limits.max_yaml_size,
        DocumentFormat::Text | DocumentFormat::Csv => limits.max_text_size,
        _ => limits.max_size,
    };
    let bound = u64::try_from(largest).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    let read = if stdin {
        io::stdin().lock().take(bound).read_to_end(&mut bytes)
    } else {
        File::open(&source.file).and_then(|f| f.take(bound).read_to_end(&mut bytes))
    };
    read.map_err(|error| {
        let shown = if stdin { "standard input" } else { &file };
        io_failure(&file, &format!("cannot read {shown}"), &error)
    })?;
    Ok(Loaded {
        bytes,
        format,
        file,
    })
}

/// Load and read the document of `source` with `limits`.
pub(crate) fn read(source: &Source, limits: &Limits) -> Result<(Document, Loaded), Failure> {
    let loaded = load(source, limits)?;
    Ok((loaded.document(limits)?, loaded))
}

/// The syntax a file's suffix gives, compared without case.
fn syntax_of(path: &Path) -> Option<Syntax> {
    let suffix = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match suffix.as_str() {
        "json" => Syntax::Json,
        "yaml" | "yml" => Syntax::Yaml,
        "rules" => Syntax::Text,
        "csv" => Syntax::Csv,
        _ => return None,
    })
}

/// An `io_error` about `file`: `what` failed with `error`.
pub(crate) fn io_failure(file: &str, what: &str, error: &io::Error) -> Failure {
    let hint = match error.kind() {
        io::ErrorKind::NotFound => Some("check the path, relative to the current directory".into()),
        io::ErrorKind::PermissionDenied => Some("check the file's permissions".into()),
        _ => None,
    };
    Failure::Io {
        file: file.to_owned(),
        message: format!("{what}: {error}"),
        hint,
    }
}
