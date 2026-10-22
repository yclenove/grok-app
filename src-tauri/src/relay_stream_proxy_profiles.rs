//! Private immutable routes. No credentials, route identities or endpoints in logs/UI.
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock, Weak};

use parking_lot::Mutex;
use tokio::sync::watch;

static ROUTES: OnceLock<Mutex<HashMap<String, Weak<Lease>>>> = OnceLock::new();

fn routes() -> &'static Mutex<HashMap<String, Weak<Lease>>> {
    ROUTES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One process generation owns its endpoints. HTTP readers borrow only the
/// cancellation receiver, so an outstanding stream cannot keep its owner alive.
struct Lease {
    identity: String,
    configuration: [u8; 32],
    upstreams: BTreeMap<String, String>,
    cancelled: watch::Sender<bool>,
}

#[derive(Clone)]
pub(crate) struct FrozenRoutes(Arc<Lease>);

impl PartialEq for FrozenRoutes {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FrozenRoutes {}

pub(crate) fn provider_component(provider: &str) -> String {
    const SAFE_COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'~');
    percent_encoding::utf8_percent_encode(provider, SAFE_COMPONENT).to_string()
}

pub(crate) fn base_url(owner: &str, generation: &str, provider: &str, port: u16) -> String {
    let provider = provider_component(provider);
    format!("http://127.0.0.1:{port}/p/{owner}/{generation}/{provider}/v1")
}

fn is_loopback_route(base: &str, prefix: &str) -> bool {
    reqwest::Url::parse(base).is_ok_and(|url| {
        let loopback = match url.host() {
            Some(url::Host::Domain(host)) => {
                host.trim_end_matches('.').eq_ignore_ascii_case("localhost")
            }
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        };
        loopback && url.path().starts_with(prefix)
    })
}

pub(crate) fn is_profile_url(base: &str) -> bool {
    is_loopback_route(base, "/p/")
}

pub(crate) fn is_legacy_proxy_url(base: &str) -> bool {
    is_loopback_route(base, "/r/")
}

impl FrozenRoutes {
    pub(crate) fn register(
        owner: &str,
        generation: &str,
        configuration: [u8; 32],
        upstreams: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        for id in [owner, generation] {
            if !uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id) {
                return Err("execution relay identity is invalid".into());
            }
        }
        for upstream in upstreams.values() {
            let url =
                reqwest::Url::parse(upstream).map_err(|_| "execution relay upstream is invalid")?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || is_profile_url(upstream)
                || is_legacy_proxy_url(upstream)
            {
                return Err("execution relay upstream is invalid".into());
            }
        }
        let identity = format!("{owner}/{generation}");
        let mut registry = routes().lock();
        if let Some(existing) = registry.get(&identity).and_then(Weak::upgrade) {
            // A last upgraded owner may be dropped on rejection. Release the
            // registry lock first: its destructor also removes this generation.
            drop(registry);
            let cancelled = *existing.cancelled.borrow();
            if existing.configuration != configuration
                || existing.upstreams != upstreams
                || cancelled
            {
                return Err("execution relay generation is changed or inactive".into());
            }
            return Ok(Self(existing));
        }
        let (cancelled, _) = watch::channel(false);
        let lease = Arc::new(Lease {
            identity: identity.clone(),
            configuration,
            upstreams,
            cancelled,
        });
        registry.insert(identity, Arc::downgrade(&lease));
        Ok(Self(lease))
    }

    pub(crate) fn cancel(&self) {
        self.0.cancelled.send_replace(true);
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.cancelled.send_replace(true);
        let mut registry = routes().lock();
        if registry
            .get(&self.identity)
            .is_some_and(|lease| std::ptr::eq(lease.as_ptr(), self))
        {
            registry.remove(&self.identity);
        }
    }
}

pub(super) struct RequestRoute {
    pub upstream: String,
    pub cancellation: watch::Receiver<bool>,
}

pub(super) fn lookup(path: &str) -> Option<(RequestRoute, &str)> {
    let mut parts = path.strip_prefix("/p/")?.splitn(4, '/');
    let identity = format!("{}/{}", parts.next()?, parts.next()?);
    let provider = parts.next()?;
    let suffix = parts.next()?;
    if suffix != "v1" && !suffix.starts_with("v1/") {
        return None;
    }
    // Never drop the last upgraded lease while holding the registry lock.
    let lease = { routes().lock().get(&identity)?.upgrade()? };
    let cancelled = *lease.cancelled.borrow();
    if cancelled {
        return None;
    }
    Some((
        RequestRoute {
            upstream: lease.upstreams.get(provider)?.clone(),
            cancellation: lease.cancelled.subscribe(),
        },
        suffix,
    ))
}

pub(super) async fn cancelled(receiver: &mut Option<watch::Receiver<bool>>) {
    match receiver {
        Some(receiver) => {
            let _ = receiver.wait_for(|cancelled| *cancelled).await;
        }
        None => std::future::pending::<()>().await,
    }
}
