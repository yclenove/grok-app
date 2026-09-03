//! HTTPS transport that validates DNS at connection time and pins the socket
//! (or proxy tunnel) to the validated public address.
//!
//! `skin_net::check_hop` validates URL policy before a request. That alone is
//! not sufficient for attacker-controlled hosts: a second resolver lookup by
//! the HTTP stack can be rebound to loopback or a private network. This
//! connector performs the security lookup while opening each new connection.
//! HTTP CONNECT and SOCKS proxies receive the validated IP address, while TLS
//! SNI and the HTTP Host header retain the original hostname.

use std::fmt;
use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::body::Incoming;
use hyper::header::{HeaderMap, HeaderValue, ACCEPT_ENCODING, HOST, PROXY_AUTHORIZATION};
use hyper::{Request, StatusCode, Uri};
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use ipnet::IpNet;
use percent_encoding::percent_decode_str;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tower::Service;
use url::Url;

use crate::proxy::{self, ProxyDecision};
use crate::skin_net;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const PROXY_REPLY_LIMIT: usize = 16 * 1024;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ConnectorFuture =
    Pin<Box<dyn Future<Output = Result<TokioIo<SafeIo>, BoxError>> + Send + 'static>>;

trait AsyncIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> AsyncIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

struct SafeIo {
    inner: Box<dyn AsyncIo>,
}

impl SafeIo {
    fn new<T>(inner: T) -> Self
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        Self {
            inner: Box::new(inner),
        }
    }
}

impl fmt::Debug for SafeIo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SafeIo")
    }
}

impl AsyncRead for SafeIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for SafeIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut *self.inner).poll_write(context, buffer)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut *self.inner).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut *self.inner).poll_shutdown(context)
    }
}

impl Connection for SafeIo {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

#[derive(Clone)]
struct SafeHttpsConnector {
    route: RouteSnapshot,
}

impl fmt::Debug for SafeHttpsConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SafeHttpsConnector")
    }
}

impl Service<Uri> for SafeHttpsConnector {
    type Response = TokioIo<SafeIo>;
    type Error = BoxError;
    type Future = ConnectorFuture;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, destination: Uri) -> Self::Future {
        let route = self.route.clone();
        Box::pin(async move { connect_https(destination, route).await })
    }
}

#[derive(Clone)]
pub struct SafeHttpsClient {
    inner: Client<SafeHttpsConnector, Empty<Bytes>>,
}

impl Default for SafeHttpsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SafeHttpsClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SafeHttpsClient")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeHttpsErrorKind {
    Blocked,
    Timeout,
    Network,
}

#[derive(Debug)]
pub struct SafeHttpsError {
    kind: SafeHttpsErrorKind,
}

impl SafeHttpsError {
    fn new(kind: SafeHttpsErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> SafeHttpsErrorKind {
        self.kind
    }
}

impl fmt::Display for SafeHttpsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            SafeHttpsErrorKind::Blocked => "HTTPS destination blocked",
            SafeHttpsErrorKind::Timeout => "HTTPS request timed out",
            SafeHttpsErrorKind::Network => "HTTPS request failed",
        })
    }
}

impl std::error::Error for SafeHttpsError {}

pub struct SafeHttpsResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Incoming,
    deadline: Instant,
}

impl SafeHttpsResponse {
    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn content_length(&self) -> Option<u64> {
        self.headers
            .get(hyper::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
    }

    pub async fn chunk(&mut self) -> Result<Option<Bytes>, SafeHttpsError> {
        loop {
            let frame = tokio::time::timeout_at(self.deadline.into(), self.body.frame())
                .await
                .map_err(|_| SafeHttpsError::new(SafeHttpsErrorKind::Timeout))?
                .transpose()
                .map_err(|_| SafeHttpsError::new(SafeHttpsErrorKind::Network))?;
            let Some(frame) = frame else {
                return Ok(None);
            };
            if let Ok(data) = frame.into_data() {
                return Ok(Some(data));
            }
        }
    }
}

impl SafeHttpsClient {
    pub fn new() -> Self {
        Self::with_route(route_snapshot())
    }

    fn with_route(route: RouteSnapshot) -> Self {
        Self {
            inner: Client::builder(TokioExecutor::new()).build(SafeHttpsConnector { route }),
        }
    }

    pub async fn get(
        &self,
        url: &Url,
        mut headers: HeaderMap,
        timeout: Duration,
    ) -> Result<SafeHttpsResponse, SafeHttpsError> {
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
        {
            return Err(SafeHttpsError::new(SafeHttpsErrorKind::Blocked));
        }
        headers
            .entry(ACCEPT_ENCODING)
            .or_insert(HeaderValue::from_static("identity"));
        let uri = url
            .as_str()
            .parse::<Uri>()
            .map_err(|_| SafeHttpsError::new(SafeHttpsErrorKind::Blocked))?;
        let request = build_get_request(uri, headers)?;
        let deadline = Instant::now() + timeout;
        let response = tokio::time::timeout_at(deadline.into(), self.inner.request(request))
            .await
            .map_err(|_| SafeHttpsError::new(SafeHttpsErrorKind::Timeout))?
            .map_err(|error| {
                let kind = if error_chain_has_io_kind(&error, io::ErrorKind::PermissionDenied) {
                    SafeHttpsErrorKind::Blocked
                } else if error_chain_has_io_kind(&error, io::ErrorKind::TimedOut) {
                    SafeHttpsErrorKind::Timeout
                } else {
                    SafeHttpsErrorKind::Network
                };
                SafeHttpsError::new(kind)
            })?;
        let (parts, body) = response.into_parts();
        Ok(SafeHttpsResponse {
            status: parts.status,
            headers: parts.headers,
            body,
            deadline,
        })
    }
}

fn build_get_request(
    uri: Uri,
    mut headers: HeaderMap,
) -> Result<Request<Empty<Bytes>>, SafeHttpsError> {
    let authority = uri
        .authority()
        .ok_or_else(|| SafeHttpsError::new(SafeHttpsErrorKind::Blocked))?;
    let host = HeaderValue::from_str(authority.as_str())
        .map_err(|_| SafeHttpsError::new(SafeHttpsErrorKind::Blocked))?;
    headers.insert(HOST, host);
    let request = Request::get(uri)
        .body(Empty::new())
        .map_err(|_| SafeHttpsError::new(SafeHttpsErrorKind::Blocked))?;
    let (mut parts, body) = request.into_parts();
    parts.headers = headers;
    Ok(Request::from_parts(parts, body))
}

fn error_chain_has_io_kind(error: &(dyn std::error::Error + 'static), kind: io::ErrorKind) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == kind)
        {
            return true;
        }
        current = error.source();
    }
    false
}

static SHARED_CLIENT: LazyLock<parking_lot::Mutex<Option<(RouteSnapshot, SafeHttpsClient)>>> =
    LazyLock::new(|| parking_lot::Mutex::new(None));

pub fn shared() -> SafeHttpsClient {
    let route = route_snapshot();
    let mut cached = SHARED_CLIENT.lock();
    if let Some((cached_route, client)) = cached.as_ref() {
        if cached_route == &route {
            return client.clone();
        }
    }
    let client = SafeHttpsClient::with_route(route.clone());
    *cached = Some((route, client.clone()));
    client
}

static TLS_CONFIG: LazyLock<Arc<ClientConfig>> = LazyLock::new(|| {
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(config)
});

async fn connect_https(
    destination: Uri,
    route: RouteSnapshot,
) -> Result<TokioIo<SafeIo>, BoxError> {
    if destination.scheme_str() != Some("https") {
        return Err(io_error(io::ErrorKind::PermissionDenied, "HTTPS required"));
    }
    let host = destination
        .host()
        .ok_or_else(|| io_error(io::ErrorKind::PermissionDenied, "missing host"))?
        .to_string();
    let port = destination.port_u16().unwrap_or(443);
    let target_addrs = resolve_public_target(&host, port).await?;
    let proxy = proxy_for_target(&route, &host, port, &target_addrs)?;

    let mut last_error: Option<BoxError> = None;
    for plan in connection_plans(&host, target_addrs) {
        match connect_target_once(&plan.tls_host, plan.target, proxy.as_ref()).await {
            Ok(stream) => return Ok(TokioIo::new(stream)),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io_error(io::ErrorKind::NotConnected, "no address")))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConnectionPlan {
    tls_host: String,
    target: SocketAddr,
}

fn connection_plans(host: &str, addresses: Vec<SocketAddr>) -> Vec<ConnectionPlan> {
    addresses
        .into_iter()
        .map(|target| ConnectionPlan {
            tls_host: host.to_string(),
            target,
        })
        .collect()
}

async fn resolve_public_target(host: &str, port: u16) -> Result<Vec<SocketAddr>, BoxError> {
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| io_error(io::ErrorKind::NotConnected, "DNS failed"))?
        .collect::<Vec<_>>();
    validate_public_addresses(addresses)
}

fn validate_public_addresses(addresses: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, BoxError> {
    if addresses.is_empty() {
        return Err(io_error(io::ErrorKind::NotConnected, "DNS empty"));
    }
    if addresses
        .iter()
        .any(|address| skin_net::is_blocked_ip(address.ip()))
    {
        return Err(io_error(
            io::ErrorKind::PermissionDenied,
            "private destination blocked",
        ));
    }
    let mut unique = Vec::with_capacity(addresses.len());
    for address in addresses {
        if !unique.contains(&address) {
            unique.push(address);
        }
    }
    Ok(unique)
}

#[derive(Clone)]
struct ProxyRoute {
    url: Url,
}

#[derive(Clone, PartialEq, Eq)]
struct RouteSnapshot {
    proxy_url: Option<String>,
    no_proxy: Option<String>,
}

fn route_snapshot() -> RouteSnapshot {
    match proxy::resolve().decision {
        ProxyDecision::Direct => RouteSnapshot {
            proxy_url: None,
            no_proxy: None,
        },
        ProxyDecision::Use { url, no_proxy } => RouteSnapshot {
            proxy_url: Some(url),
            no_proxy,
        },
        ProxyDecision::Inherit => RouteSnapshot {
            proxy_url: first_env(&["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]),
            no_proxy: first_env(&["NO_PROXY", "no_proxy"]),
        },
    }
}

fn proxy_for_target(
    route: &RouteSnapshot,
    host: &str,
    port: u16,
    addresses: &[SocketAddr],
) -> Result<Option<ProxyRoute>, BoxError> {
    if route
        .no_proxy
        .as_deref()
        .is_some_and(|list| no_proxy_matches(host, port, addresses, list))
    {
        return Ok(None);
    }
    let Some(raw_proxy) = route.proxy_url.as_deref() else {
        return Ok(None);
    };
    if !proxy::is_valid_proxy_url(raw_proxy) {
        return Err(io_error(io::ErrorKind::InvalidInput, "invalid proxy"));
    }
    let url = Url::parse(raw_proxy)
        .map_err(|_| io_error(io::ErrorKind::InvalidInput, "invalid proxy"))?;
    Ok(Some(ProxyRoute { url }))
}

fn first_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn no_proxy_matches(host: &str, port: u16, addresses: &[SocketAddr], list: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    list.split(',').any(|raw| {
        let entry = raw.trim();
        if entry.is_empty() {
            return false;
        }
        if entry == "*" {
            return true;
        }
        let (entry, required_port) = if entry.starts_with('[') {
            let Some(end) = entry.find(']') else {
                return false;
            };
            let suffix = &entry[end + 1..];
            let required_port = suffix
                .strip_prefix(':')
                .and_then(|value| value.parse::<u16>().ok());
            (&entry[1..end], required_port)
        } else if entry.matches(':').count() == 1 {
            if let Some((name, port)) = entry.rsplit_once(':') {
                if let Ok(required_port) = port.parse::<u16>() {
                    (name, Some(required_port))
                } else {
                    (entry, None)
                }
            } else {
                (entry, None)
            }
        } else {
            (entry, None)
        };
        if required_port.is_some_and(|required| required != port) {
            return false;
        }
        if let Ok(network) = entry.parse::<IpNet>() {
            return addresses
                .iter()
                .any(|address| network.contains(&address.ip()));
        }
        if let Ok(ip) = entry.parse::<IpAddr>() {
            return addresses.iter().any(|address| address.ip() == ip);
        }
        let domain = entry
            .strip_prefix("*.")
            .unwrap_or(entry)
            .trim_start_matches('.')
            .to_ascii_lowercase();
        !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
    })
}

async fn connect_target_once(
    target_host: &str,
    target: SocketAddr,
    proxy: Option<&ProxyRoute>,
) -> Result<SafeIo, BoxError> {
    let transport = match proxy {
        None => SafeIo::new(connect_tcp(&[target]).await?),
        Some(route) => connect_proxy_tunnel(route, target).await?,
    };
    tls_wrap(transport, target_host).await
}

async fn connect_proxy_tunnel(route: &ProxyRoute, target: SocketAddr) -> Result<SafeIo, BoxError> {
    let scheme = route.url.scheme();
    let proxy_host = route
        .url
        .host_str()
        .ok_or_else(|| io_error(io::ErrorKind::InvalidInput, "proxy host missing"))?;
    let proxy_port = route.url.port_or_known_default().unwrap_or(match scheme {
        "socks5" | "socks5h" => 1080,
        "https" => 443,
        _ => 80,
    });
    let proxy_addrs = tokio::net::lookup_host((proxy_host, proxy_port))
        .await
        .map_err(|_| io_error(io::ErrorKind::NotConnected, "proxy DNS failed"))?
        .collect::<Vec<_>>();
    let mut stream = SafeIo::new(connect_tcp(&proxy_addrs).await?);
    if scheme == "https" {
        stream = tls_wrap(stream, proxy_host).await?;
    }
    match scheme {
        "http" | "https" => http_connect(stream, &route.url, target).await,
        "socks5" | "socks5h" => socks5_connect(stream, &route.url, target).await,
        _ => Err(io_error(io::ErrorKind::InvalidInput, "proxy scheme")),
    }
}

async fn connect_tcp(addresses: &[SocketAddr]) -> Result<TcpStream, BoxError> {
    if addresses.is_empty() {
        return Err(io_error(io::ErrorKind::NotConnected, "no socket address"));
    }
    let result = tokio::time::timeout(CONNECT_TIMEOUT, async {
        let mut last = None;
        for address in addresses {
            match TcpStream::connect(address).await {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    return Ok(stream);
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "connect")))
    })
    .await
    .map_err(|_| io_error(io::ErrorKind::TimedOut, "connect timeout"))?;
    result.map_err(|_| io_error(io::ErrorKind::NotConnected, "connect failed"))
}

async fn tls_wrap(stream: SafeIo, host: &str) -> Result<SafeIo, BoxError> {
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| io_error(io::ErrorKind::InvalidInput, "invalid TLS host"))?;
    let stream = tokio::time::timeout(
        CONNECT_TIMEOUT,
        TlsConnector::from(Arc::clone(&TLS_CONFIG)).connect(server_name, stream),
    )
    .await
    .map_err(|_| io_error(io::ErrorKind::TimedOut, "TLS timeout"))?
    .map_err(|_| io_error(io::ErrorKind::ConnectionAborted, "TLS failed"))?;
    Ok(SafeIo::new(stream))
}

async fn http_connect(
    mut stream: SafeIo,
    proxy_url: &Url,
    target: SocketAddr,
) -> Result<SafeIo, BoxError> {
    let request = http_connect_request(proxy_url, target)?;
    stream.write_all(&request).await?;
    stream.flush().await?;

    let mut reply = Vec::with_capacity(512);
    loop {
        if reply.len() == PROXY_REPLY_LIMIT {
            return Err(io_error(
                io::ErrorKind::InvalidData,
                "proxy reply too large",
            ));
        }
        let mut byte = [0_u8; 1];
        let read = stream.read(&mut byte).await?;
        if read == 0 {
            return Err(io_error(io::ErrorKind::UnexpectedEof, "proxy reply ended"));
        }
        reply.push(byte[0]);
        if reply.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let first_line = reply.split(|byte| *byte == b'\n').next().unwrap_or(&[]);
    let success = first_line.starts_with(b"HTTP/1.1 2") || first_line.starts_with(b"HTTP/1.0 2");
    if !success {
        return Err(io_error(
            io::ErrorKind::ConnectionRefused,
            "proxy CONNECT failed",
        ));
    }
    Ok(stream)
}

fn http_connect_request(proxy_url: &Url, target: SocketAddr) -> Result<Vec<u8>, BoxError> {
    let authority = socket_authority(target);
    let mut request = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: keep-alive\r\n"
    );
    if let Some(auth) = proxy_authorization(proxy_url)? {
        request.push_str("Proxy-Authorization: ");
        request.push_str(
            auth.to_str()
                .map_err(|_| io_error(io::ErrorKind::InvalidInput, "proxy authorization"))?,
        );
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    Ok(request.into_bytes())
}

async fn socks5_connect(
    mut stream: SafeIo,
    proxy_url: &Url,
    target: SocketAddr,
) -> Result<SafeIo, BoxError> {
    let username = decode_component(proxy_url.username());
    let password = decode_component(proxy_url.password().unwrap_or(""));
    let has_auth = !username.is_empty() || proxy_url.password().is_some();
    stream
        .write_all(if has_auth {
            &[5_u8, 2, 0, 2]
        } else {
            &[5_u8, 1, 0]
        })
        .await?;
    let mut choice = [0_u8; 2];
    stream.read_exact(&mut choice).await?;
    if choice[0] != 5 || choice[1] == 0xff {
        return Err(io_error(
            io::ErrorKind::PermissionDenied,
            "SOCKS auth method",
        ));
    }
    if choice[1] == 2 {
        if username.len() > 255 || password.len() > 255 {
            return Err(io_error(io::ErrorKind::InvalidInput, "SOCKS credentials"));
        }
        let mut auth = Vec::with_capacity(username.len() + password.len() + 3);
        auth.extend_from_slice(&[1, username.len() as u8]);
        auth.extend_from_slice(username.as_bytes());
        auth.push(password.len() as u8);
        auth.extend_from_slice(password.as_bytes());
        stream.write_all(&auth).await?;
        let mut result = [0_u8; 2];
        stream.read_exact(&mut result).await?;
        if result != [1, 0] {
            return Err(io_error(
                io::ErrorKind::PermissionDenied,
                "SOCKS auth failed",
            ));
        }
    } else if choice[1] != 0 {
        return Err(io_error(
            io::ErrorKind::PermissionDenied,
            "SOCKS auth unsupported",
        ));
    }

    let request = socks5_target_request(target);
    stream.write_all(&request).await?;

    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 5 || header[1] != 0 {
        return Err(io_error(
            io::ErrorKind::ConnectionRefused,
            "SOCKS connect failed",
        ));
    }
    let address_bytes = match header[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut length = [0_u8; 1];
            stream.read_exact(&mut length).await?;
            length[0] as usize
        }
        _ => return Err(io_error(io::ErrorKind::InvalidData, "SOCKS reply")),
    };
    let mut discard = vec![0_u8; address_bytes + 2];
    stream.read_exact(&mut discard).await?;
    Ok(stream)
}

fn socks5_target_request(target: SocketAddr) -> Vec<u8> {
    let mut request = vec![5_u8, 1, 0];
    match target.ip() {
        IpAddr::V4(ip) => {
            request.push(1);
            request.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            request.push(4);
            request.extend_from_slice(&ip.octets());
        }
    }
    request.extend_from_slice(&target.port().to_be_bytes());
    request
}

fn proxy_authorization(url: &Url) -> Result<Option<HeaderValue>, BoxError> {
    if url.username().is_empty() && url.password().is_none() {
        return Ok(None);
    }
    let value = B64.encode(format!(
        "{}:{}",
        decode_component(url.username()),
        decode_component(url.password().unwrap_or(""))
    ));
    let mut header = HeaderValue::from_str(&format!("Basic {value}"))
        .map_err(|_| io_error(io::ErrorKind::InvalidInput, "proxy authorization"))?;
    header.set_sensitive(true);
    debug_assert_eq!(PROXY_AUTHORIZATION.as_str(), "proxy-authorization");
    Ok(Some(header))
}

fn decode_component(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

fn socket_authority(address: SocketAddr) -> String {
    match address.ip() {
        IpAddr::V4(ip) => format!("{ip}:{}", address.port()),
        IpAddr::V6(ip) => format!("[{ip}]:{}", address.port()),
    }
}

fn io_error(kind: io::ErrorKind, message: &'static str) -> BoxError {
    Box::new(io::Error::new(kind, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn destination_validation_rejects_any_private_dns_answer() {
        let public = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 443);
        let private = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 443);
        assert!(validate_public_addresses(vec![public, private]).is_err());
        assert_eq!(
            validate_public_addresses(vec![public, public]).unwrap(),
            [public]
        );
    }

    #[test]
    fn proxy_destinations_use_ip_authorities_but_keep_original_tls_name_separate() {
        let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)), 443);
        let plans = connection_plans("cdn.example", vec![target]);
        assert_eq!(
            plans,
            [ConnectionPlan {
                tls_host: "cdn.example".into(),
                target,
            }]
        );

        let proxy = Url::parse("http://proxy.example:8080").unwrap();
        let connect = String::from_utf8(http_connect_request(&proxy, target).unwrap()).unwrap();
        assert!(
            connect.starts_with("CONNECT 203.0.113.8:443 HTTP/1.1\r\nHost: 203.0.113.8:443\r\n")
        );
        assert!(!connect.contains("cdn.example"));

        let socks = socks5_target_request(target);
        assert_eq!(socks, [5, 1, 0, 1, 203, 0, 113, 8, 1, 187]);
        assert!(!socks
            .windows("cdn.example".len())
            .any(|window| window == b"cdn.example"));

        assert_eq!(
            socket_authority(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 443)),
            "[::1]:443"
        );
    }

    #[test]
    fn no_proxy_matches_domains_ips_cidr_and_optional_ports() {
        let addresses = [SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)),
            443,
        )];
        assert!(no_proxy_matches(
            "cdn.example.com",
            443,
            &addresses,
            ".example.com"
        ));
        assert!(no_proxy_matches(
            "cdn.example.com",
            443,
            &addresses,
            "*.example.com"
        ));
        assert!(!no_proxy_matches(
            "notexample.com",
            443,
            &addresses,
            "example.com"
        ));
        assert!(no_proxy_matches(
            "cdn.example.com",
            443,
            &addresses,
            "203.0.113.0/24"
        ));
        assert!(no_proxy_matches(
            "cdn.example.com",
            443,
            &addresses,
            "cdn.example.com:443"
        ));
        assert!(!no_proxy_matches(
            "cdn.example.com",
            8443,
            &addresses,
            "cdn.example.com:443"
        ));
    }

    #[test]
    fn request_keeps_the_original_http_host() {
        let uri = "https://cdn.example:8443/image.jpg".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("forged.example"));
        let request = build_get_request(uri, headers).unwrap();
        assert_eq!(request.headers().get(HOST).unwrap(), "cdn.example:8443");
    }
}
