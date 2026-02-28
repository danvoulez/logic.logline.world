//! Cookie helpers.

use crate::{Error, Result};
use httpdate::fmt_http_date;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// SameSite attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    /// SameSite=Strict
    Strict,
    /// SameSite=Lax
    Lax,
    /// SameSite=None
    None,
}

impl SameSite {
    fn as_str(self) -> &'static str {
        match self {
            SameSite::Strict => "Strict",
            SameSite::Lax => "Lax",
            SameSite::None => "None",
        }
    }
}

/// Options used to build a session cookie.
#[derive(Debug, Clone)]
pub struct CookieOptions {
    /// Cookie name (without any prefix).
    pub name: String,

    /// Cookie path.
    pub path: String,

    /// Optional cookie domain.
    pub domain: Option<String>,

    /// Send on HTTPS only.
    pub secure: bool,

    /// Not accessible to JS.
    pub http_only: bool,

    /// SameSite attribute.
    pub same_site: SameSite,

    /// Max-Age in seconds.
    pub max_age_seconds: Option<u64>,

    /// If true and `domain` is None, the cookie name will be prefixed with `__Host-`
    /// and the function will enforce `path=/` and `secure=true`.
    pub use_host_prefix: bool,
}

impl Default for CookieOptions {
    fn default() -> Self {
        Self {
            name: "logline_session".to_string(),
            path: "/".to_string(),
            domain: None,
            secure: true,
            http_only: true,
            same_site: SameSite::Lax,
            max_age_seconds: None,
            use_host_prefix: true,
        }
    }
}

fn cookie_name(opts: &CookieOptions) -> Result<String> {
    if opts.use_host_prefix && opts.domain.is_none() {
        // Enforce __Host- cookie requirements.
        if opts.path != "/" {
            return Err(Error::Validation(
                "__Host- cookies must have Path=/".to_string(),
            ));
        }
        if !opts.secure {
            return Err(Error::Validation(
                "__Host- cookies must be Secure".to_string(),
            ));
        }
        Ok(format!(
            "__Host-{}",
            opts.name.trim_start_matches("__Host-")
        ))
    } else {
        Ok(opts.name.clone())
    }
}

/// Build a `Set-Cookie` header value.
pub fn build_set_cookie(value: &str, opts: &CookieOptions) -> Result<String> {
    let name = cookie_name(opts)?;

    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{name}={value}"));
    parts.push(format!("Path={}", opts.path));

    if let Some(domain) = &opts.domain {
        parts.push(format!("Domain={domain}"));
    }

    if opts.secure {
        parts.push("Secure".to_string());
    }
    if opts.http_only {
        parts.push("HttpOnly".to_string());
    }

    parts.push(format!("SameSite={}", opts.same_site.as_str()));

    if let Some(max_age) = opts.max_age_seconds {
        parts.push(format!("Max-Age={max_age}"));
        // Expires for older clients.
        let expires = SystemTime::now() + Duration::from_secs(max_age);
        parts.push(format!("Expires={}", fmt_http_date(expires)));
    }

    Ok(parts.join("; "))
}

/// Build a `Set-Cookie` header value that clears the cookie.
pub fn build_clear_cookie(opts: &CookieOptions) -> Result<String> {
    let name = cookie_name(opts)?;

    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{name}="));
    parts.push(format!("Path={}", opts.path));

    if let Some(domain) = &opts.domain {
        parts.push(format!("Domain={domain}"));
    }

    if opts.secure {
        parts.push("Secure".to_string());
    }
    if opts.http_only {
        parts.push("HttpOnly".to_string());
    }

    parts.push(format!("SameSite={}", opts.same_site.as_str()));
    parts.push("Max-Age=0".to_string());
    parts.push(format!("Expires={}", fmt_http_date(UNIX_EPOCH)));

    Ok(parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_cookie_name() {
        let opts = CookieOptions::default();
        let sc = build_set_cookie("abc", &opts).unwrap();
        assert!(sc.starts_with("__Host-logline_session=abc"));
        assert!(sc.contains("Path=/"));
    }

    #[test]
    fn clear_cookie_has_max_age_zero() {
        let opts = CookieOptions::default();
        let sc = build_clear_cookie(&opts).unwrap();
        assert!(sc.contains("Max-Age=0"));
    }
}
