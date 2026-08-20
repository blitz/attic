//! build-trace v2
//!
//! `GET`/`PUT` `/:cache/build-trace-v2/{drvName}.drv/{outputName}.doi`
//!
//! Reading requires "pull" permission, writing requires "push". Nix defines
//! the path layout and the JSON body, so the names below are fixed.
//!
//! Nix 2.35 introduced this layout. The manual covers the concept and the
//! entry, but not the URL, which only appears in the source.
//!
//! Concept: <https://releases.nixos.org/nix/nix-2.35.2/manual/store/build-trace.html>
//!
//! Entry: <https://releases.nixos.org/nix/nix-2.35.2/manual/protocols/json/build-trace-entry.html>
//!
//! Nix implementation of the URL: <https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/binary-cache-store.cc#L667-L670>
//!
//! Nix implementation of the prefix: <https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/binary-cache-store.hh#L118>
//!
//! The C++ says "realisation" throughout, so the endpoint does not turn up
//! under a search for "build trace".
//!
//! Nix writes the unkeyed half of the entry to the cache, since the path
//! carries the key, which is why `BuildTraceEntry` has no `key` field.

use serde::{Deserialize, Serialize};

/// Path component introducing a build trace lookup.
///
/// Nix raises this version while the feature is experimental and abandons the
/// entries at the old prefix.
pub const BUILD_TRACE_PREFIX: &str = "build-trace-v2";

/// Suffix on the output name in a build trace path.
pub const BUILD_TRACE_SUFFIX: &str = ".doi";

/// A build trace entry, as Nix reads and writes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildTraceEntry {
    /// Base name of the realized store path, without the store directory
    /// prefix.
    #[serde(rename = "outPath")]
    pub out_path: String,

    /// Signatures over the entry.
    ///
    /// Nix accepts an empty list.
    #[serde(default)]
    pub signatures: Vec<BuildTraceSignature>,
}

/// One signature over a build trace entry.
///
/// Neither Nix nor Celler verifies these when substituting, so both fields
/// stay unparsed strings and any entry Nix accepts round-trips through here.
///
/// Nix implementation: <https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libutil/include/nix/util/signature/local-keys.hh#L15-L22>
///
/// Nix issue on the missing check: <https://github.com/NixOS/nix/issues/11393>
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildTraceSignature {
    /// Name of the key that produced `sig`.
    ///
    /// `NixKeypair::sign` returns this and `sig` joined by a colon.
    #[serde(rename = "keyName")]
    pub key_name: String,

    /// Base64 of the raw signature bytes.
    pub sig: String,
}
