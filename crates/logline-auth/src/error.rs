//! Error types.

use thiserror::Error;

/// Crate result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by this crate.
#[derive(Debug, Error)]
pub enum Error {
    /// JWT is malformed or missing required fields.
    #[error("invalid JWT: {0}")]
    InvalidJwt(String),

    /// Algorithm is not in the allow-list.
    #[error("unsupported JWT algorithm: {0:?}")]
    UnsupportedAlg(jsonwebtoken::Algorithm),

    /// Unable to fetch, parse, or use a JWKS.
    #[error("JWKS error: {0}")]
    Jwks(String),

    /// The JWKS does not contain a usable key for the token.
    #[error("no suitable key found in JWKS")]
    NoMatchingKey,

    /// Token claims failed validation.
    #[error("token validation failed: {0}")]
    Validation(String),

    /// An error occurred while performing HTTP requests.
    #[cfg(feature = "fetch-reqwest")]
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    /// JSON parsing error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// jsonwebtoken error.
    #[error(transparent)]
    Jwt(#[from] jsonwebtoken::errors::Error),
}
