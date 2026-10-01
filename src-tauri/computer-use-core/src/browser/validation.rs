//! Navigate / profile / actionId validation. Never reaches the worker for blocked schemes.

use crate::error::BrokerError;

use super::types::ManagedPage;

pub(crate) fn validate_managed_page(page: &ManagedPage) -> Result<(), BrokerError> {
    if page.page_id.trim().is_empty() || page.page_generation == 0 {
        return Err(BrokerError::Schema("managed page identity required".into()));
    }
    Ok(())
}

pub(crate) fn validate_browser_action_id(action_id: &str) -> Result<(), BrokerError> {
    if action_id.trim().is_empty()
        || action_id.len() > 128
        || !action_id
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'_' | b':' | b'-'))
    {
        return Err(BrokerError::Schema(
            "valid browser actionId required".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_profile_name(name: &str) -> Result<String, BrokerError> {
    if name.is_empty()
        || name.len() > 64
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-')
    {
        return Err(BrokerError::Schema(
            "invalid managed browser profile name".into(),
        ));
    }
    Ok(name.to_string())
}

fn url_origin(url: &str) -> Result<String, BrokerError> {
    let url = url.trim();
    if url.eq_ignore_ascii_case("about:blank") {
        return Ok("about:blank".into());
    }
    let lower = url.to_ascii_lowercase();
    let Some((scheme, rest)) = lower.split_once(':') else {
        return Err(BrokerError::Schema("navigate url must be http(s)".into()));
    };
    let rest = rest
        .strip_prefix("//")
        .ok_or_else(|| BrokerError::Schema("navigate url must be http(s) with a host".into()))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    Ok(format!("{scheme}://{authority}"))
}

/// Same-origin redirects are allowed. The landed URL must still pass scheme/credential/metadata checks.
pub fn navigation_result_allowed(requested: &str, actual: &str) -> Result<(), BrokerError> {
    is_allowed_navigate_url(requested)?;
    is_allowed_navigate_url(actual)?;
    if url_origin(requested)? != url_origin(actual)? {
        return Err(BrokerError::IdentityMismatch("origin"));
    }
    Ok(())
}

/// Model navigate is http(s) or about:blank only. javascript/data/file never reach the worker.
pub fn is_allowed_navigate_url(url: &str) -> Result<(), BrokerError> {
    let url = url.trim();
    if url.is_empty() || url.len() > 2048 {
        return Err(BrokerError::Schema(
            "navigate url is empty or too long".into(),
        ));
    }
    if url.contains('\0') || url.contains('\n') || url.contains('\r') {
        return Err(BrokerError::Schema(
            "navigate url contains control characters".into(),
        ));
    }
    let lower = url.to_ascii_lowercase();
    if lower == "about:blank" {
        return Ok(());
    }
    let Some((scheme, rest)) = lower.split_once(':') else {
        return Err(BrokerError::Schema("navigate url must be http(s)".into()));
    };
    match scheme {
        "http" | "https" => {}
        "javascript" | "data" | "file" | "vbscript" | "blob" | "about" => {
            return Err(BrokerError::Schema(format!(
                "{scheme}: navigation is not allowed"
            )));
        }
        _ => {
            return Err(BrokerError::Schema("navigate url must be http(s)".into()));
        }
    }
    let rest = rest
        .strip_prefix("//")
        .ok_or_else(|| BrokerError::Schema("navigate url must be http(s) with a host".into()))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() || authority.contains('@') {
        return Err(BrokerError::Schema(
            "navigate url must not include credentials".into(),
        ));
    }
    let host = navigate_host(authority);
    if blocked_navigate_host(host) {
        return Err(BrokerError::Schema(
            "navigate url must not target instance metadata".into(),
        ));
    }
    Ok(())
}

fn navigate_host(authority: &str) -> &str {
    if let Some(inner) = authority.strip_prefix('[') {
        inner.split(']').next().unwrap_or(authority)
    } else if let Some((name, port)) = authority.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            name
        } else {
            authority
        }
    } else {
        authority
    }
}

fn blocked_navigate_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.');
    host == "169.254.169.254"
        || host == "100.100.100.200"
        || host == "metadata.google.internal"
        || host.ends_with(".metadata.google.internal")
        || host == "::ffff:169.254.169.254"
}
