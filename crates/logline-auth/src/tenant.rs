//! Tenant derivation helpers.

use serde_json::Value;

/// Where a tenant decision came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantSource {
    /// Derived from the request host (subdomain).
    Host,
    /// Derived from a token claim.
    Claim,
    /// No tenant could be derived.
    None,
}

/// Tenant derivation configuration.
#[derive(Debug, Clone)]
pub struct TenantConfig {
    /// If set, a host like `acme.example.com` will derive tenant `acme`
    /// when `host_root` is `example.com`.
    pub host_root: Option<String>,

    /// If set, a claim like `{ "tenant_id": "acme" }` will be considered.
    pub claim_key: Option<String>,

    /// Prefer host-derived tenants over claim-derived tenants.
    pub prefer_host: bool,

    /// Optional allow-list of tenant ids.
    pub allow_list: Option<Vec<String>>,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            host_root: None,
            claim_key: Some("tenant_id".to_string()),
            prefer_host: true,
            allow_list: None,
        }
    }
}

/// Result of deriving a tenant.
#[derive(Debug, Clone)]
pub struct TenantDecision {
    /// Derived tenant id.
    pub tenant_id: Option<String>,
    /// Source used.
    pub source: TenantSource,
}

impl TenantDecision {
    /// True if a tenant was derived.
    pub fn is_some(&self) -> bool {
        self.tenant_id.is_some()
    }
}

/// Derive a tenant id from host and/or claims.
///
/// - If `cfg.prefer_host` is true, host is tried first.
/// - If `cfg.allow_list` is set, derived tenants must be in the list.
pub fn derive_tenant(host: Option<&str>, claims: &Value, cfg: &TenantConfig) -> TenantDecision {
    let host_tenant = host.and_then(|h| derive_from_host(h, cfg.host_root.as_deref()));
    let claim_tenant = derive_from_claims(claims, cfg.claim_key.as_deref());

    let (tenant_id, source) = if cfg.prefer_host {
        if host_tenant.is_some() {
            (host_tenant, TenantSource::Host)
        } else if claim_tenant.is_some() {
            (claim_tenant, TenantSource::Claim)
        } else {
            (None, TenantSource::None)
        }
    } else {
        if claim_tenant.is_some() {
            (claim_tenant, TenantSource::Claim)
        } else if host_tenant.is_some() {
            (host_tenant, TenantSource::Host)
        } else {
            (None, TenantSource::None)
        }
    };

    let tenant_id = match (tenant_id, &cfg.allow_list) {
        (Some(t), Some(allow)) if !allow.iter().any(|a| a == &t) => None,
        (t, _) => t,
    };

    let source = if tenant_id.is_some() {
        source
    } else {
        TenantSource::None
    };

    TenantDecision { tenant_id, source }
}

fn derive_from_host(host: &str, host_root: Option<&str>) -> Option<String> {
    let mut host = host.trim().to_lowercase();

    // Strip port, if any.
    if let Some((h, _port)) = host.split_once(':') {
        host = h.to_string();
    }

    let root = host_root?.trim().to_lowercase();
    let root = root.trim_start_matches('.');

    if host == root {
        return None;
    }

    if !host.ends_with(root) {
        return None;
    }

    // Remove root suffix. Example: acme.example.com -> acme.
    let prefix = host.trim_end_matches(root).trim_end_matches('.');
    if prefix.is_empty() {
        return None;
    }

    // Use the last label of the remaining prefix (supports nested subdomains).
    let tenant = prefix.split('.').last()?.to_string();

    // Basic sanity: [a-z0-9-]
    if tenant
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        Some(tenant)
    } else {
        None
    }
}

fn derive_from_claims(claims: &Value, claim_key: Option<&str>) -> Option<String> {
    let key = claim_key?;
    claims
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_from_host() {
        assert_eq!(
            derive_from_host("acme.example.com", Some("example.com")),
            Some("acme".to_string())
        );
        assert_eq!(
            derive_from_host("foo.acme.example.com", Some("example.com")),
            Some("acme".to_string())
        );
        assert_eq!(derive_from_host("example.com", Some("example.com")), None);
    }

    #[test]
    fn tenant_from_claims() {
        let claims: Value = serde_json::json!({"tenant_id":"acme"});
        let cfg = TenantConfig::default();
        let d = derive_tenant(None, &claims, &cfg);
        assert_eq!(d.tenant_id.as_deref(), Some("acme"));
        assert_eq!(d.source, TenantSource::Claim);
    }
}
