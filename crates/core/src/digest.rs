//! Canonical bytes and digests of documents (`spec/json-format.md`,
//! *Canonical bytes* and *Digests*).
//!
//! The canonical bytes of a document are the UTF-8 JSON text of its canonical
//! form with every object's members sorted by key, by Unicode code point, no
//! insignificant whitespace, minimal string escapes and one layout for each
//! number. Its digest is the SHA-256 hash of those bytes, written
//! `sha256:<64 lowercase hex digits>`.
//!
//! A digest identifies one concrete canonical artifact. It does not say that
//! two rule sets decide alike: `x > 5` and `5 < x` have different digests.
//!
//! ```
//! use cleanrules::format::{Document, read_document};
//! use cleanrules::json::Limits;
//!
//! let spaced = r#"{ "version": 1, "condition": { "op": "gt", "left": { "op": "field", "path": ["x"] }, "right": { "op": "const", "value": 1e2 } } }"#;
//! let document = read_document(spaced, &Limits::default())?;
//! assert_eq!(
//!     document.canonical_bytes(),
//!     br#"{"condition":{"left":{"op":"field","path":["x"]},"op":"gt","right":{"op":"const","value":100.0}},"version":1}"#
//! );
//! assert!(document.digest().to_string().starts_with("sha256:"));
//! # Ok::<(), cleanrules::Error>(())
//! ```

use std::fmt;
use std::str::FromStr;

use sha2::{Digest as _, Sha256};

use crate::FORMAT_VERSION;
use crate::error::{Error, ErrorCode};
use crate::format::{Document, write_document, write_ruleset};
use crate::json::{Json, JsonNumber, to_sorted_string};
use crate::rules::{Rule, RuleSet};

/// The prefix of a SHA-256 digest, which names the hash function.
const PREFIX: &str = "sha256:";

/// The SHA-256 digest of a document's canonical bytes (`spec/json-format.md`,
/// *Digests*).
///
/// It is written, and parsed, as `sha256:` followed by 64 lowercase
/// hexadecimal digits, the form every language gives.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// The SHA-256 digest of `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// The 32 bytes of the hash.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(PREFIX)?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

impl FromStr for Digest {
    type Err = Error;

    /// Parse `sha256:` and 64 lowercase hexadecimal digits.
    ///
    /// # Errors
    ///
    /// `format_error` for any other text: another prefix, uppercase digits, or
    /// another length.
    fn from_str(text: &str) -> Result<Self, Error> {
        let malformed = || {
            let shown: String = text.chars().take(80).collect();
            let more = if shown.len() < text.len() { "…" } else { "" };
            Error::new(
                ErrorCode::FormatError,
                format!("{shown:?}{more} is not a digest"),
            )
            .with_hint("a digest is sha256: followed by 64 lowercase hexadecimal digits")
        };
        let hex = text.strip_prefix(PREFIX).ok_or_else(malformed)?;
        if hex.len() != 64 {
            return Err(malformed());
        }
        let mut bytes = [0; 32];
        let (pairs, _) = hex.as_bytes().as_chunks::<2>();
        for (byte, pair) in bytes.iter_mut().zip(pairs) {
            let digit = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                _ => None,
            };
            *byte = match (digit(pair[0]), digit(pair[1])) {
                (Some(high), Some(low)) => high << 4 | low,
                _ => return Err(malformed()),
            };
        }
        Ok(Self(bytes))
    }
}

/// A document's canonical bytes from its content key and written content.
fn canonical(key: &str, content: Json) -> Vec<u8> {
    let document = Json::Object(vec![
        (
            "version".into(),
            Json::Number(JsonNumber::from_literal(FORMAT_VERSION.to_string())),
        ),
        (key.into(), content),
    ]);
    to_sorted_string(&document).into_bytes()
}

impl Document {
    /// The document's canonical bytes (`spec/json-format.md`, *Canonical
    /// bytes*): the UTF-8 JSON text of its canonical form, with every object's
    /// members sorted by key, by Unicode code point.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        to_sorted_string(&write_document(self)).into_bytes()
    }

    /// The SHA-256 digest of the document's canonical bytes
    /// (`spec/json-format.md`, *Digests*).
    ///
    /// For a decision table, this is the digest of the table's own document;
    /// the rule set it compiles to has its own, [`RuleSet::digest`].
    #[must_use]
    pub fn digest(&self) -> Digest {
        Digest::of(&self.canonical_bytes())
    }
}

impl Rule {
    /// The canonical bytes of the rule's document (`{"rule": …, "version": 1}`).
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical("rule", crate::format::write_rule(self))
    }

    /// The digest of the rule's document.
    #[must_use]
    pub fn digest(&self) -> Digest {
        Digest::of(&self.canonical_bytes())
    }
}

impl RuleSet {
    /// The canonical bytes of the rule set's document
    /// (`{"ruleset": …, "version": 1}`).
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical("ruleset", write_ruleset(self))
    }

    /// The rule set digest (`spec/json-format.md`, *Rule set digests*): the
    /// digest of the rule set's document. Decisions carry it under
    /// `ruleset_digest` when asked ([`crate::rules::Decision::with_ruleset_digest`]),
    /// and readers verify it.
    #[must_use]
    pub fn digest(&self) -> Digest {
        *self
            .digest
            .get_or_init(|| Digest::of(&self.canonical_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::Digest;
    use crate::ErrorCode;
    use crate::build::{constant, field};
    use crate::format::{Document, read_document};
    use crate::json::Limits;
    use crate::rules::{Config, Rule, RuleSet};
    use crate::table::DecisionTable;

    #[test]
    fn digests_are_sha256_of_the_canonical_bytes() {
        // FIPS 180-4's example: SHA-256 of "abc".
        assert_eq!(
            Digest::of(b"abc").to_string(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let text = Digest::of(b"").to_string();
        assert_eq!(text.parse::<Digest>().unwrap(), Digest::of(b""));
        assert_eq!(format!("{:?}", Digest::of(b"")), format!("Digest({text})"));
        assert_eq!(Digest::of(b"abc").as_bytes()[0], 0xba);
    }

    #[test]
    fn only_one_form_parses() {
        let good = Digest::of(b"abc").to_string();
        for bad in [
            good.to_uppercase(),
            good.replace("sha256:", "SHA256:"),
            good.replace("sha256:", "sha512:"),
            good.trim_start_matches("sha256:").to_owned(),
            format!("{good}0"),
            good[..good.len() - 1].to_owned(),
            good.replace('a', "g"),
            good.replacen('b', "B", 1),
            format!(" {good}"),
        ] {
            let error = bad.parse::<Digest>().unwrap_err();
            assert_eq!(error.code(), ErrorCode::FormatError, "{bad}");
        }
    }

    #[test]
    fn rules_and_rule_sets_agree_with_their_documents() {
        let rule = Rule::new("vip", field("x").gt(constant(5)).build().unwrap()).unwrap();
        let ruleset = RuleSet::new(vec![rule.clone()], Config::default()).unwrap();
        assert_eq!(
            rule.canonical_bytes(),
            Document::Rule(rule.clone()).canonical_bytes()
        );
        assert_eq!(rule.digest(), Document::Rule(rule.clone()).digest());
        assert_eq!(
            ruleset.digest(),
            Document::RuleSet(ruleset.clone()).digest()
        );
        assert_eq!(
            String::from_utf8(ruleset.canonical_bytes()).unwrap(),
            r#"{"ruleset":{"hit_policy":"first","logic":"boolean","rules":[{"name":"vip","when":{"left":{"op":"field","path":["x"]},"op":"gt","right":{"op":"const","value":5}}}]},"version":1}"#
        );
        // A rule read as a rule set has the rule set's digest, not the rule's.
        assert_ne!(rule.digest(), ruleset.digest());
        // The same rule written the other way round is another artifact.
        let flipped = Rule::new("vip", constant(5).lt(field("x")).build().unwrap()).unwrap();
        assert_ne!(flipped.digest(), rule.digest());
    }

    #[test]
    fn a_table_has_its_own_digest_and_its_rule_set_digest() {
        let text = r#"{"version": 1, "decision_table": {"hit_policy": "first", "logic": "boolean", "inputs": ["x"], "outputs": [{"name": "y"}], "rows": [{"name": "big", "inputs": ["> 1"], "outputs": ["2"]}]}}"#;
        let document = read_document(text, &Limits::default()).unwrap();
        let Document::DecisionTable(table) = &document else {
            panic!("a table")
        };
        let table: &DecisionTable = table;
        assert_ne!(document.digest(), table.ruleset().digest());
        assert_eq!(
            document.clone().into_ruleset().unwrap().digest(),
            table.ruleset().digest()
        );
    }
}
