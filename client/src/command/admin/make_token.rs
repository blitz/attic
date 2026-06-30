use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chrono::{Duration as ChronoDuration, Utc};
use clap::Parser;
use humantime::Duration;

use attic::cache::CacheNamePattern;
use attic_token::{HS256Key, RS256KeyPair, SignatureType, Token};

/// Generate a new token.
///
/// For example, to generate a token for Alice with read-write access
/// to any cache starting with `dev-` and read-only access to `prod`,
/// expiring in 2 years:
///
/// $ atticadm make-token --sub "alice" --validity "2y" --pull "dev-*" --push "dev-*" --pull "prod"
#[derive(Debug, Parser)]
pub struct MakeToken {
    /// The signing key to use for the token.
    ///
    /// For RS256 specify the path of a PEM file containing the secret key.
    ///
    /// For HS256 specify a file that contains the raw secret key (not BASE64-encoded).
    #[clap(long = "signing-key", value_name = "SECRET_KEY")]
    signing_key: PathBuf,

    /// The signing algorithm to use for the token.
    #[clap(long, default_value = "rs256")]
    signing_algorithm: SigningAlgorithm,

    /// The subject of the JWT token.
    #[clap(long)]
    sub: String,

    /// The `iss` claim of the JWT.
    #[clap(long = "issuer", value_name = "ISSUER")]
    pub issuer: Option<String>,

    /// The `aud` claim of the JWT.
    ///
    /// Can be specified multiple times to allow multiple audiences.
    #[clap(long = "audience", value_name = "AUDIENCE")]
    pub audience: Vec<String>,

    /// The validity period of the JWT token.
    ///
    /// You can use expressions like "2 years", "3 months"
    /// and "1y".
    #[clap(long)]
    validity: Duration,

    /// Dump the claims without signing and encoding it.
    #[clap(long)]
    dump_claims: bool,

    /// A cache that the token may pull from.
    ///
    /// The value may contain wildcards. Specify this flag multiple
    /// times to allow multiple patterns.
    #[clap(long = "pull", value_name = "PATTERN")]
    pull_patterns: Vec<CacheNamePattern>,

    /// A cache that the token may push to.
    ///
    /// The value may contain wildcards. Specify this flag multiple
    /// times to allow multiple patterns.
    #[clap(long = "push", value_name = "PATTERN")]
    push_patterns: Vec<CacheNamePattern>,

    /// A cache that the token may delete store paths from.
    ///
    /// The value may contain wildcards. Specify this flag multiple
    /// times to allow multiple patterns.
    #[clap(long = "delete", value_name = "PATTERN")]
    delete_patterns: Vec<CacheNamePattern>,

    /// A cache that the token may create.
    ///
    /// The value may contain wildcards. Specify this flag multiple
    /// times to allow multiple patterns.
    #[clap(long = "create-cache", value_name = "PATTERN")]
    create_cache_patterns: Vec<CacheNamePattern>,

    /// A cache that the token may configure.
    ///
    /// The value may contain wildcards. Specify this flag multiple
    /// times to allow multiple patterns.
    #[clap(long = "configure-cache", value_name = "PATTERN")]
    configure_cache_patterns: Vec<CacheNamePattern>,

    /// A cache that the token may configure retention/quota for.
    ///
    /// The value may contain wildcards. Specify this flag multiple
    /// times to allow multiple patterns.
    #[clap(long = "configure-cache-retention", value_name = "PATTERN")]
    configure_cache_retention_patterns: Vec<CacheNamePattern>,

    /// A cache that the token may destroy.
    ///
    /// The value may contain wildcards. Specify this flag multiple
    /// times to allow multiple patterns.
    #[clap(long = "destroy-cache", value_name = "PATTERN")]
    destroy_cache_patterns: Vec<CacheNamePattern>,
}

/// The supported signing algorithms for the token.
#[derive(Debug, Clone, clap::ValueEnum)]
enum SigningAlgorithm {
    RS256,
    HS256,
}

macro_rules! grant_permissions {
    ($token:ident, $list:expr, $perm:ident) => {
        for pattern in $list {
            let perm = $token.get_or_insert_permission_mut(pattern.to_owned());
            perm.$perm = true;
        }
    };
}

pub async fn run(sub: &MakeToken) -> Result<()> {
    let duration = ChronoDuration::from_std(sub.validity.into())?;
    let exp = Utc::now()
        .checked_add_signed(duration)
        .ok_or_else(|| anyhow!("Expiry timestamp overflowed"))?;

    let mut token = Token::new(sub.sub.to_owned(), &exp);

    grant_permissions!(token, &sub.pull_patterns, pull);
    grant_permissions!(token, &sub.push_patterns, push);
    grant_permissions!(token, &sub.delete_patterns, delete);
    grant_permissions!(token, &sub.create_cache_patterns, create_cache);
    grant_permissions!(token, &sub.configure_cache_patterns, configure_cache);
    grant_permissions!(
        token,
        &sub.configure_cache_retention_patterns,
        configure_cache_retention
    );
    grant_permissions!(token, &sub.destroy_cache_patterns, destroy_cache);

    if sub.dump_claims {
        println!("{}", serde_json::to_string(token.opaque_claims())?);
    } else {
        let secret_key = std::fs::read(&sub.signing_key)
            .map_err(|e| anyhow!("Failed to read signing key: {}", e))?;

        let signature_type = match sub.signing_algorithm {
            SigningAlgorithm::HS256 => {
                SignatureType::HS256(HS256Key::from_bytes(&secret_key))
            }
            SigningAlgorithm::RS256 => {
                let secret_key = String::from_utf8(secret_key)
                    .map_err(|e| anyhow!("Cannot decode signing key: {}", e))?;
                SignatureType::RS256(RS256KeyPair::from_pem(&secret_key)?)
            }
        };

        let encoded_token = token.encode(
            &signature_type,
            &sub.issuer,
            &(!sub.audience.is_empty()).then(|| sub.audience.iter().cloned().collect()),
        )?;
        println!("{}", encoded_token);
    }

    Ok(())
}
