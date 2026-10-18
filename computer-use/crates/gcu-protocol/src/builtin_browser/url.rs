//! Fail-closed URL classification. One parse, no IO, no repeated percent-decoding.
//!
//! Non-ASCII hosts are rejected because this crate cannot take an IDNA library.
//! ASCII punycode (`xn--`) stays classifiable. A public classification is not
//! permission to connect: the caller still needs a proven platform.

/// Maximum accepted URL size, in UTF-8 bytes.
pub const URL_MAX_BYTES: usize = 8192;

/// Where the URL would be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlRole {
    /// Tool, address bar, history, link, form, frame, popup, or redirect.
    Navigation,
    /// Image, style, fetch, XHR, WebSocket, or worker target.
    Subresource,
}

/// Classification. [`UrlClass::Public`] carries the canonical URL that would be dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlClass {
    /// Public http(s) navigation, or a public http(s)/ws(s) subresource.
    Public(String),
    /// Refused. Nothing may be handed to a WebView.
    Denied,
}

/// Classify `raw` for `role`. Denied results must not be connected.
#[must_use]
pub fn classify(raw: &str, role: UrlRole) -> UrlClass {
    if raw.len() > URL_MAX_BYTES || raw.is_empty() || forbidden_char(raw) {
        return UrlClass::Denied;
    }
    let Some((scheme_raw, rest)) = raw.split_once(':') else {
        return UrlClass::Denied;
    };
    if scheme_raw.is_empty()
        || !scheme_raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    {
        return UrlClass::Denied;
    }
    let scheme = scheme_raw.to_ascii_lowercase();
    let http = scheme == "http" || scheme == "https";
    let ws = scheme == "ws" || scheme == "wss";
    let allowed = match role {
        UrlRole::Navigation => http,
        UrlRole::Subresource => http || ws,
    };
    if !allowed || !rest.starts_with("//") {
        return UrlClass::Denied;
    }
    let rest = &rest[2..];
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains('%')
        || authority.contains('\\')
    {
        return UrlClass::Denied;
    }
    let Ok((host_raw, port)) = split_host_port(authority) else {
        return UrlClass::Denied;
    };
    if host_raw.is_empty() || host_raw.contains('%') {
        return UrlClass::Denied;
    }
    let host = if host_raw.ends_with("..") {
        return UrlClass::Denied;
    } else if let Some(stripped) = host_raw.strip_suffix('.') {
        if looks_numeric(stripped) || stripped.contains(':') {
            return UrlClass::Denied;
        }
        stripped.to_ascii_lowercase()
    } else {
        host_raw.to_ascii_lowercase()
    };
    if !host_public(&host) {
        return UrlClass::Denied;
    }
    UrlClass::Public(canonical(&scheme, &host, port, path))
}

fn forbidden_char(raw: &str) -> bool {
    raw.chars().any(|ch| {
        ch.is_control()
            || ch == '\\'
            || ch == ' '
            || ch == '\u{feff}'
            || ('\u{200b}'..='\u{200f}').contains(&ch)
            || ('\u{202a}'..='\u{202e}').contains(&ch)
            || ('\u{2066}'..='\u{2069}').contains(&ch)
            || !ch.is_ascii()
    })
}

fn split_host_port(authority: &str) -> Result<(String, Option<u16>), ()> {
    if let Some(host) = authority.strip_prefix('[') {
        let end = host.find(']').ok_or(())?;
        let inside = &host[..end];
        let after = &host[end + 1..];
        if inside.is_empty() {
            return Err(());
        }
        let port = if after.is_empty() {
            None
        } else {
            Some(parse_port(after.strip_prefix(':').ok_or(())?)?)
        };
        return Ok((inside.to_owned(), port));
    }
    match authority.matches(':').count() {
        0 => Ok((authority.to_owned(), None)),
        1 => {
            let (host, port) = authority.split_once(':').ok_or(())?;
            if host.is_empty() {
                return Err(());
            }
            Ok((host.to_owned(), Some(parse_port(port)?)))
        }
        _ => Err(()),
    }
}

fn parse_port(text: &str) -> Result<u16, ()> {
    if text.is_empty() || !text.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(());
    }
    let port: u16 = text.parse().map_err(|_| ())?;
    if port == 0 {
        Err(())
    } else {
        Ok(port)
    }
}

fn host_public(host: &str) -> bool {
    if host.contains(':') {
        return host.parse::<std::net::Ipv6Addr>().is_ok_and(ipv6_public);
    }
    if looks_numeric(host) {
        return assemble_ipv4(host).is_some_and(|ip| !ipv4_blocked(ip));
    }
    dns_public(host)
}

fn assemble_ipv4(host: &str) -> Option<u32> {
    let mut parts = [0u64; 4];
    let mut count = 0usize;
    for part in host.split('.') {
        if count == 4 {
            return None;
        }
        parts[count] = part_number(part)?;
        count += 1;
    }
    let value = match count {
        1 if parts[0] <= u64::from(u32::MAX) => parts[0],
        2 if parts[0] <= 255 && parts[1] <= 0x00FF_FFFF => (parts[0] << 24) | parts[1],
        3 if parts[0] <= 255 && parts[1] <= 255 && parts[2] <= 0xFFFF => {
            (parts[0] << 24) | (parts[1] << 16) | parts[2]
        }
        4 if parts.iter().all(|part| *part <= 255) => {
            (parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]
        }
        _ => return None,
    };
    u32::try_from(value).ok()
}

fn part_number(part: &str) -> Option<u64> {
    if let Some(hex) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
        if hex.is_empty() {
            return None;
        }
        return u64::from_str_radix(hex, 16).ok();
    }
    if part.len() > 1 && part.starts_with('0') {
        if !part.chars().all(|ch| matches!(ch, '0'..='7')) {
            return None;
        }
        return u64::from_str_radix(part, 8).ok();
    }
    part.parse().ok()
}

fn ipv4_blocked(ip: u32) -> bool {
    let high = ip >> 24;
    let next = (ip >> 16) & 0xff;
    if high == 0 || high == 10 || high == 127 {
        return true;
    }
    if high == 169 && next == 254 {
        return true;
    }
    if high == 172 && (16..=31).contains(&next) {
        return true;
    }
    if high == 192 && next == 168 {
        return true;
    }
    if high == 100 && (64..=127).contains(&next) {
        return true;
    }
    if ip & 0xFFFF_FF00 == 0xC000_0000
        || ip & 0xFFFF_FF00 == 0xC000_0200
        || ip & 0xFFFF_FF00 == 0xC633_6400
        || ip & 0xFFFF_FF00 == 0xCB00_7100
        || ip & 0xFFFF_FF00 == 0xC058_6300
        || ip & 0xFFFE_0000 == 0xC612_0000
    {
        return true;
    }
    ip >= 0xE000_0000
}

fn ipv6_public(addr: std::net::Ipv6Addr) -> bool {
    let seg = addr.segments();
    (0x2000..=0x3fff).contains(&seg[0]) && !(seg[0] == 0x2001 && seg[1] == 0x0db8)
}

fn dns_public(host: &str) -> bool {
    if host.is_empty()
        || host == "localhost"
        || host == "metadata.google.internal"
        || !host.contains('.')
        || host.starts_with('.')
        || host.ends_with('.')
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.contains("..")
    {
        return false;
    }
    host.split('.').all(|label| {
        (1..=63).contains(&label.len())
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

fn looks_numeric(host: &str) -> bool {
    !host.is_empty()
        && host.split('.').all(|part| {
            if let Some(hex) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
                !hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit())
            } else {
                !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit())
            }
        })
}

fn canonical(scheme: &str, host: &str, port: Option<u16>, path: &str) -> String {
    let default = match scheme {
        "http" | "ws" => 80,
        _ => 443,
    };
    let mut out = format!("{scheme}://{host}");
    if let Some(port) = port {
        if port != default {
            out.push(':');
            out.push_str(&port.to_string());
        }
    }
    if path.is_empty() {
        out.push('/');
    } else {
        out.push_str(path);
    }
    out
}
