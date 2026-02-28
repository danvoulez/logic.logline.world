//! JWT verification using JWKS.

use crate::{Error, Result};

use base64::Engine;
use jsonwebtoken::{Algorithm, DecodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(feature = "cache")]
use dashmap::DashMap;
#[cfg(feature = "cache")]
use once_cell::sync::Lazy;

/// A JWKS (JSON Web Key Set).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JwksSet {
    /// Keys.
    pub keys: Vec<Jwk>,
}

/// Minimal JWK structure for RSA/EC/OKP.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Jwk {
    /// Key type ("RSA", "EC", "OKP").
    pub kty: String,

    /// Key id.
    pub kid: Option<String>,

    /// Public key use.
    #[serde(rename = "use")]
    pub use_: Option<String>,

    /// Algorithm (optional).
    pub alg: Option<String>,

    // RSA
    /// RSA modulus.
    pub n: Option<String>,
    /// RSA exponent.
    pub e: Option<String>,

    // EC
    /// Curve name.
    pub crv: Option<String>,
    /// EC x coordinate.
    pub x: Option<String>,
    /// EC y coordinate.
    pub y: Option<String>,

    // Symmetric (not supported)
    /// Symmetric key.
    pub k: Option<String>,
}

/// Source of JWKS data.
#[derive(Debug, Clone)]
pub enum JwksSource {
    /// Fetch from this URL.
    Url(String),
    /// Parse this JSON string.
    Json(String),
    /// Use this parsed key set.
    Set(JwksSet),
}

/// Options for token verification.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// URL of the JWKS.
    pub jwks_url: String,

    /// Expected issuer (`iss`).
    pub issuer: Option<String>,

    /// Expected audience (`aud`).
    pub audience: Option<String>,

    /// Allowed algorithms.
    pub allowed_algs: Vec<Algorithm>,

    /// Clock skew/leeway in seconds.
    pub leeway_seconds: u64,

    /// Max cache age for JWKS (seconds). Only used when the `cache` feature is enabled.
    pub max_jwks_age_seconds: u64,

    /// If true, reject tokens without a `kid` header.
    pub require_kid: bool,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            jwks_url: String::new(),
            issuer: None,
            audience: None,
            allowed_algs: vec![Algorithm::RS256, Algorithm::ES256, Algorithm::EdDSA],
            leeway_seconds: 60,
            max_jwks_age_seconds: 300,
            require_kid: false,
        }
    }
}

/// A verified JWT (header + claims).
#[derive(Debug, Clone)]
pub struct VerifiedJwt {
    /// The parsed header.
    pub header: Header,
    /// The decoded claims as JSON.
    pub claims: Value,
}

impl VerifiedJwt {
    /// Get a claim by key.
    pub fn claim(&self, key: &str) -> Option<&Value> {
        self.claims.get(key)
    }

    /// Convenience accessor for `sub`.
    pub fn sub(&self) -> Option<&str> {
        self.claim("sub").and_then(|v| v.as_str())
    }

    /// Convenience accessor for `iss`.
    pub fn iss(&self) -> Option<&str> {
        self.claim("iss").and_then(|v| v.as_str())
    }

    /// Convenience accessor for `aud`.
    pub fn aud(&self) -> Option<&Value> {
        self.claim("aud")
    }

    /// Convenience accessor for `exp`.
    pub fn exp(&self) -> Option<i64> {
        self.claim("exp").and_then(|v| v.as_i64())
    }
}

#[cfg(feature = "cache")]
#[derive(Debug, Clone)]
struct CachedJwks {
    exp_at_ms: u128,
    jwks: JwksSet,
}

#[cfg(feature = "cache")]
static JWKS_CACHE: Lazy<DashMap<String, CachedJwks>> = Lazy::new(DashMap::new);

/// Verifies JWTs against a JWKS.
#[derive(Debug, Clone, Default)]
pub struct JwtVerifier {
    _priv: (),
}

impl JwtVerifier {
    /// Verify a token using the configured JWKS URL.
    ///
    /// Requires the `fetch-reqwest` feature.
    pub async fn verify_with_jwks_url(
        &self,
        token: &str,
        opts: VerifyOptions,
    ) -> Result<VerifiedJwt> {
        self.verify_with_source(token, JwksSource::Url(opts.jwks_url.clone()), opts)
            .await
    }

    /// Verify a token using a JWKS source (URL, JSON, or parsed set).
    pub async fn verify_with_source(
        &self,
        token: &str,
        source: JwksSource,
        opts: VerifyOptions,
    ) -> Result<VerifiedJwt> {
        let header = jsonwebtoken::decode_header(token)
            .map_err(|e| Error::InvalidJwt(format!("failed to decode header: {e}")))?;

        if !opts.allowed_algs.contains(&header.alg) {
            return Err(Error::UnsupportedAlg(header.alg));
        }

        if opts.require_kid && header.kid.as_deref().unwrap_or("").is_empty() {
            return Err(Error::InvalidJwt("missing kid".to_string()));
        }

        let jwks = self.load_jwks(&source, &opts).await?;
        verify_against_jwks(token, &header, &jwks, &opts)
    }

    async fn load_jwks(&self, source: &JwksSource, opts: &VerifyOptions) -> Result<JwksSet> {
        match source {
            JwksSource::Set(set) => Ok(set.clone()),
            JwksSource::Json(json) => Ok(serde_json::from_str(json)?),
            JwksSource::Url(url) => {
                #[cfg(feature = "cache")]
                {
                    let now_ms = now_epoch_ms();
                    if let Some(cached) = JWKS_CACHE.get(url) {
                        if cached.exp_at_ms > now_ms {
                            return Ok(cached.jwks.clone());
                        }
                    }

                    let (set, max_age_seconds) = fetch_jwks_url(url).await?;
                    let ttl = std::cmp::min(max_age_seconds, opts.max_jwks_age_seconds);
                    let exp_at_ms = now_ms + (ttl as u128 * 1000);
                    JWKS_CACHE.insert(
                        url.clone(),
                        CachedJwks {
                            exp_at_ms,
                            jwks: set.clone(),
                        },
                    );
                    return Ok(set);
                }

                #[cfg(not(feature = "cache"))]
                {
                    let (set, _max_age_seconds) = fetch_jwks_url(url).await?;
                    Ok(set)
                }
            }
        }
    }

    /// Resolve an OIDC issuer to its `jwks_uri` via discovery.
    ///
    /// This is a helper so callers can do:
    /// 1) `jwks_url = resolve_oidc_jwks_url(issuer)`
    /// 2) `verify_with_jwks_url(token, VerifyOptions { jwks_url, issuer: Some(issuer), .. })`
    ///
    /// Requires the `fetch-reqwest` feature.
    pub async fn resolve_oidc_jwks_url(
        &self,
        issuer: &str,
        max_age_seconds: u64,
    ) -> Result<String> {
        let issuer = issuer.trim_end_matches('/');
        let discovery = format!("{issuer}/.well-known/openid-configuration");

        #[cfg(feature = "cache")]
        {
            static DISCOVERY_CACHE: Lazy<DashMap<String, (u128, String)>> = Lazy::new(DashMap::new);
            let now_ms = now_epoch_ms();
            if let Some(cached) = DISCOVERY_CACHE.get(&discovery) {
                if cached.value().0 > now_ms {
                    return Ok(cached.value().1.clone());
                }
            }
            let (doc, max_age) = fetch_json_with_cache_control(&discovery).await?;
            let jwks_uri = doc
                .get("jwks_uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Jwks("OIDC discovery missing jwks_uri".to_string()))?;
            let ttl = std::cmp::min(max_age, max_age_seconds);
            DISCOVERY_CACHE.insert(
                discovery,
                (now_ms + (ttl as u128 * 1000), jwks_uri.to_string()),
            );
            return Ok(jwks_uri.to_string());
        }

        #[cfg(not(feature = "cache"))]
        {
            let (doc, _max_age) = fetch_json_with_cache_control(&discovery).await?;
            let jwks_uri = doc
                .get("jwks_uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Jwks("OIDC discovery missing jwks_uri".to_string()))?;
            Ok(jwks_uri.to_string())
        }
    }
}

fn verify_against_jwks(
    token: &str,
    header: &Header,
    jwks: &JwksSet,
    opts: &VerifyOptions,
) -> Result<VerifiedJwt> {
    let mut validation = Validation::new(header.alg);
    validation.leeway = opts.leeway_seconds;
    validation.validate_exp = true;
    validation.validate_nbf = true;
    // We validate `iss`/`aud` manually for maximum compatibility across jsonwebtoken versions.
    validation.validate_aud = false;

    // Prefer kid match when present.
    let mut candidates: Vec<&Jwk> = Vec::new();
    if let Some(kid) = header.kid.as_deref() {
        for k in &jwks.keys {
            if k.kid.as_deref() == Some(kid) {
                candidates.push(k);
            }
        }
    }
    if candidates.is_empty() {
        candidates = jwks.keys.iter().collect();
    }

    let mut last_err: Option<jsonwebtoken::errors::Error> = None;

    for jwk in candidates {
        if let Ok(key) = decoding_key_from_jwk(jwk) {
            match jsonwebtoken::decode::<Value>(token, &key, &validation) {
                Ok(data) => {
                    let verified = VerifiedJwt {
                        header: data.header,
                        claims: data.claims,
                    };
                    validate_issuer_audience(&verified, opts)?;
                    return Ok(verified);
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }
    }

    if let Some(e) = last_err {
        // If we got here, we at least tried keys.
        // Map signature/claim errors to a clearer message.
        return Err(Error::Validation(format!("{e}")));
    }

    Err(Error::NoMatchingKey)
}

fn decoding_key_from_jwk(jwk: &Jwk) -> Result<DecodingKey> {
    match jwk.kty.as_str() {
        "RSA" => {
            let n = jwk
                .n
                .as_deref()
                .ok_or_else(|| Error::Jwks("RSA JWK missing n".to_string()))?;
            let e = jwk
                .e
                .as_deref()
                .ok_or_else(|| Error::Jwks("RSA JWK missing e".to_string()))?;
            Ok(DecodingKey::from_rsa_components(n, e)?)
        }
        "EC" => {
            let x = jwk
                .x
                .as_deref()
                .ok_or_else(|| Error::Jwks("EC JWK missing x".to_string()))?;
            let y = jwk
                .y
                .as_deref()
                .ok_or_else(|| Error::Jwks("EC JWK missing y".to_string()))?;
            Ok(DecodingKey::from_ec_components(x, y)?)
        }
        "OKP" => {
            // jsonwebtoken supports EdDSA in recent versions. It expects a DER-encoded key.
            // Many providers publish Ed25519 as OKP + x (public key bytes base64url).
            let crv = jwk.crv.as_deref().unwrap_or("");
            if crv != "Ed25519" {
                return Err(Error::Jwks(format!("unsupported OKP curve: {crv}")));
            }
            let x = jwk
                .x
                .as_deref()
                .ok_or_else(|| Error::Jwks("OKP JWK missing x".to_string()))?;
            let pubkey = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(x)
                .map_err(|e| Error::Jwks(format!("invalid okp x: {e}")))?;
            Ok(DecodingKey::from_ed_der(&ed25519_spki_der(&pubkey)))
        }
        other => Err(Error::Jwks(format!("unsupported kty: {other}"))),
    }
}

fn validate_issuer_audience(verified: &VerifiedJwt, opts: &VerifyOptions) -> Result<()> {
    if let Some(expected_iss) = &opts.issuer {
        let iss = verified
            .iss()
            .ok_or_else(|| Error::Validation("missing iss".to_string()))?;
        if iss != expected_iss {
            return Err(Error::Validation(format!(
                "issuer mismatch: expected {expected_iss}, got {iss}"
            )));
        }
    }

    if let Some(expected_aud) = &opts.audience {
        let aud = verified
            .aud()
            .ok_or_else(|| Error::Validation("missing aud".to_string()))?;
        let ok = match aud {
            Value::String(s) => s == expected_aud,
            Value::Array(arr) => arr
                .iter()
                .any(|v| v.as_str() == Some(expected_aud.as_str())),
            _ => false,
        };
        if !ok {
            return Err(Error::Validation(format!(
                "audience mismatch: expected {expected_aud}"
            )));
        }
    }

    Ok(())
}

fn ed25519_spki_der(pubkey32: &[u8]) -> Vec<u8> {
    // SubjectPublicKeyInfo for Ed25519:
    // SEQUENCE {
    //   SEQUENCE { OID 1.3.101.112 }
    //   BIT STRING (pubkey)
    // }
    // This is a tiny DER builder just for this structure.

    // OID 1.3.101.112 DER: 06 03 2B 65 70
    let mut alg_id: Vec<u8> = vec![0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, 0x70];

    // BIT STRING: 03 21 00 <32 bytes>
    let mut bit_string: Vec<u8> = vec![0x03, 0x21, 0x00];
    bit_string.extend_from_slice(&pubkey32[..32]);

    // Outer SEQUENCE length = alg_id + bit_string
    let len = alg_id.len() + bit_string.len();
    let mut out: Vec<u8> = vec![0x30, len as u8];
    out.append(&mut alg_id);
    out.append(&mut bit_string);
    out
}

fn now_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis()
}

#[cfg(feature = "fetch-reqwest")]
async fn fetch_jwks_url(url: &str) -> Result<(JwksSet, u64)> {
    let (json, max_age) = fetch_json_string_with_cache_control(url).await?;
    let set: JwksSet = serde_json::from_str(&json)?;
    Ok((set, max_age))
}

#[cfg(not(feature = "fetch-reqwest"))]
async fn fetch_jwks_url(_url: &str) -> Result<(JwksSet, u64)> {
    Err(Error::Jwks(
        "JwksSource::Url requires the fetch-reqwest feature (or provide JwksSource::Json/Set)"
            .to_string(),
    ))
}

#[cfg(feature = "fetch-reqwest")]
async fn fetch_json_with_cache_control(url: &str) -> Result<(Value, u64)> {
    let (json, max_age) = fetch_json_string_with_cache_control(url).await?;
    Ok((serde_json::from_str(&json)?, max_age))
}

#[cfg(not(feature = "fetch-reqwest"))]
async fn fetch_json_with_cache_control(_url: &str) -> Result<(Value, u64)> {
    Err(Error::Jwks(
        "fetching discovery requires the fetch-reqwest feature".to_string(),
    ))
}

#[cfg(feature = "fetch-reqwest")]
async fn fetch_json_string_with_cache_control(url: &str) -> Result<(String, u64)> {
    use reqwest::header;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Jwks(format!("fetch failed: {}", resp.status())));
    }

    let max_age = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|h| h.to_str().ok())
        .and_then(parse_cache_control_max_age)
        .unwrap_or(300);

    let text = resp.text().await?;
    Ok((text, max_age))
}

fn parse_cache_control_max_age(cc: &str) -> Option<u64> {
    // Very small parser: look for max-age=NNN
    for part in cc.split(',') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("max-age=") {
            if let Ok(n) = rest.trim().parse::<u64>() {
                return Some(n);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_control_parser() {
        assert_eq!(parse_cache_control_max_age("public, max-age=60"), Some(60));
        assert_eq!(parse_cache_control_max_age("max-age=0"), Some(0));
        assert_eq!(parse_cache_control_max_age("no-store"), None);
    }
}
