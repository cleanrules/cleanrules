//! Reading documents (`spec/cli.md`, *Reading documents*): the core's
//! readers, as the agents' `validate` tool reads a document.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use cleanrules::format::{Document, DocumentFormat, read_document_as, size_limit, too_large};
use cleanrules::json::Limits;

use crate::args::{Source, Syntax};
use crate::failure::Failure;

/// The bytes of a document, before they are read as one.
pub(crate) struct Loaded {
    /// The bytes, at most one more than the size limit of their syntax.
    pub(crate) bytes: Vec<u8>,
    /// The file's size, when it is a file larger than the size limit.
    pub(crate) size: Option<u64>,
    /// Their syntax.
    pub(crate) format: DocumentFormat,
    /// The file as it was given (`-` for standard input).
    pub(crate) file: String,
}

impl Loaded {
    /// The `limit_exceeded` of bytes beyond the size limit of their syntax,
    /// which were not loaded whole: with the file's size when it is known.
    pub(crate) fn too_large(&self, limits: &Limits) -> Option<cleanrules::Error> {
        (self.bytes.len() > size_limit(self.format, limits))
            .then(|| too_large(self.format, self.size, limits))
    }

    /// Read the bytes as a document of their syntax.
    pub(crate) fn document(&self, limits: &Limits) -> Result<Document, Failure> {
        let core = |error| Failure::Core {
            file: Some(self.file.clone()),
            error,
        };
        if let Some(error) = self.too_large(limits) {
            return Err(core(error));
        }
        read_document_as(&self.bytes, self.format, limits).map_err(core)
    }
}

/// Load the bytes of `source`'s document and tell their syntax.
pub(crate) fn load(source: &Source, limits: &Limits) -> Result<Loaded, Failure> {
    let file = source.file.to_string_lossy().into_owned();
    let stdin = is_stdin(&source.file);
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
    let (bytes, size) = bounded(&source.file, size_limit(format, limits)).map_err(|error| {
        let shown = if stdin { "standard input" } else { &file };
        io_failure(&file, &format!("cannot read {shown}"), &error)
    })?;
    Ok(Loaded {
        bytes,
        size,
        format,
        file,
    })
}

/// Whether `path` names standard input (`-`).
pub(crate) fn is_stdin(path: &Path) -> bool {
    path.as_os_str() == "-"
}

/// The bytes of the file at `path`, or of standard input for `-`: at most
/// one byte more than `largest`, so the reader refuses a larger file without
/// it being read whole; and, for a file larger than that, its size, which
/// standard input does not tell.
pub(crate) fn bounded(path: &Path, largest: usize) -> io::Result<(Vec<u8>, Option<u64>)> {
    let bound = u64::try_from(largest).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    if is_stdin(path) {
        io::stdin().lock().take(bound).read_to_end(&mut bytes)?;
        return Ok((bytes, None));
    }
    let file = File::open(path)?;
    // The size of a regular file only: a pipe or a device has none.
    let size = file
        .metadata()
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| metadata.len())
        .filter(|&size| size >= bound);
    file.take(bound).read_to_end(&mut bytes)?;
    Ok((bytes, size))
}

/// Load and read the document of `source` with `limits`.
pub(crate) fn read(source: &Source, limits: &Limits) -> Result<(Document, Loaded), Failure> {
    let loaded = load(source, limits)?;
    Ok((loaded.document(limits)?, loaded))
}

/// The syntax a file's suffix gives, compared without case.
pub(crate) fn syntax_of(path: &Path) -> Option<Syntax> {
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
