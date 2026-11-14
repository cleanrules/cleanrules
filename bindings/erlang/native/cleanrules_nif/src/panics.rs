//! Why a call failed, and panics caught as failures: the part of the boundary's
//! errors that needs no VM (`errors` turns a [`Failure`] into a term).
//!
//! Like `bignum` and `paths`, this module uses neither `rustler` nor the
//! `enif_*` functions, so that its unit tests can run outside the BEAM
//! (`tests/pure.rs`).

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

use cleanrules::Error;

/// Why a call failed.
#[derive(Debug)]
pub(crate) enum Failure {
    /// An error of the core: `{error, Map}`.
    Core(Error),
    /// An argument of the wrong kind: the `badarg` exception, as Erlang's own
    /// functions raise it.
    BadArg,
    /// A panic: `{error, #{code => internal_error, ...}}`.
    Internal(String),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

/// The result of a NIF's body.
pub(crate) type Outcome<T> = Result<T, Failure>;

/// The message of a panic, as the C ABI writes it.
pub(crate) fn panic_message(panic: &(dyn Any + Send)) -> String {
    let what = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic");
    format!("cleanrules failed ({what}); please report it")
}

/// Run `body`, catching a panic as [`Failure::Internal`].
pub(crate) fn catch<T>(body: impl FnOnce() -> Outcome<T>) -> Outcome<T> {
    catch_unwind(AssertUnwindSafe(body))
        .unwrap_or_else(|panic| Err(Failure::Internal(panic_message(panic.as_ref()))))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;

    #[test]
    fn a_panic_is_an_internal_failure_with_its_message() {
        let caught = catch::<()>(|| panic!("boom"));
        match caught {
            Err(Failure::Internal(message)) => {
                assert_eq!(message, "cleanrules failed (boom); please report it");
            }
            other => panic!("not an internal failure: {other:?}"),
        }
        let formatted = catch::<()>(|| panic!("{} {}", "two", "words"));
        assert!(matches!(formatted, Err(Failure::Internal(m)) if m.contains("two words")));
        let opaque = catch::<()>(|| std::panic::panic_any(7_u8));
        assert!(matches!(opaque, Err(Failure::Internal(m)) if m.contains("a panic")));
    }

    #[test]
    fn errors_and_results_pass_through() {
        assert!(matches!(catch(|| Ok(3)), Ok(3)));
        let error = Error::new(cleanrules::ErrorCode::FormatError, "bad");
        assert!(matches!(
            catch::<()>(|| Err(error.into())),
            Err(Failure::Core(e)) if e.message() == "bad"
        ));
        assert!(matches!(
            catch::<()>(|| Err(Failure::BadArg)),
            Err(Failure::BadArg)
        ));
    }
}
