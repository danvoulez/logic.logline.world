//! logline-auth
//!
//! Authentication helpers for Logline runtime services and clients.
//! It focuses on three recurring problems:
//!
//! - **Verifying JWTs using a JWKS** (kid selection, algorithm allow-list, iss/aud/leeway checks)
//! - **Deriving a tenant** from request host or token claims
//! - **Building secure cookies** (`__Host-` semantics, SameSite, etc.)
//!
//! The core API is `JwtVerifier`, which can verify a token against a JWKS URL (with optional
//! in-memory caching) or against a JWKS you provide directly.
//!
//! ## Quick start
//! ```no_run
//! use logline_auth::{JwtVerifier, VerifyOptions};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let token = "eyJ...";
//! let verifier = JwtVerifier::default();
//! let verified = verifier.verify_with_jwks_url(
//!     token,
//!     VerifyOptions {
//!         jwks_url: "https://issuer.example/.well-known/jwks.json".to_string(),
//!         issuer: Some("https://issuer.example/".to_string()),
//!         audience: Some("my-audience".to_string()),
//!         ..Default::default()
//!     },
//! ).await?;
//!
//! println!("sub={:?}", verified.sub());
//! # Ok(()) }
//! ```

#![forbid(unsafe_code)]

mod cookie;
mod error;
mod jwt;
mod tenant;

pub use cookie::{CookieOptions, SameSite, build_clear_cookie, build_set_cookie};
pub use error::{Error, Result};
pub use jwt::{JwksSource, JwtVerifier, VerifiedJwt, VerifyOptions};
pub use tenant::{TenantConfig, TenantDecision, TenantSource, derive_tenant};
