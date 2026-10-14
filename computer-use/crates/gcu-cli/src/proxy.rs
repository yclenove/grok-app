//! NDJSON stdio to authenticated loopback streamable-HTTP transport bridge.
use crate::exit::Failure;
use gcu_mcp::codec::{Decoded, NdjsonCodec};
use rmcp::{
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
        Transport,
    },
    RoleClient,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) async fn run(url: &str, fd: usize) -> Result<u8, Failure> {
    // Require literal loopback and an explicit port. No DNS, userinfo, fragments,
    // redirects, remote hosts, or environmental proxy route is permitted.
    let uri = reqwest::Url::parse(url).map_err(|_| Failure::arguments("Invalid engine URL"))?;
    if uri.scheme() != "http"
        || uri.host_str() != Some("127.0.0.1")
        || uri.port().is_none()
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.path() != "/mcp"
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        return Err(Failure::arguments(
            "Engine URL must be exact loopback MCP endpoint",
        ));
    }
    let bytes = gcu_host_protocol::transport::token_fd(fd).await?;
    let secret = gcu_host_protocol::auth::Secret::from_bytes(bytes);
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Failure::internal("Cannot create loopback HTTP client"))?;
    let config = StreamableHttpClientTransportConfig::with_uri(url.to_owned())
        .auth_header(secret.hex())
        .max_concurrent_requests(1)
        .reinit_on_expired_session(false);
    let mut transport = StreamableHttpClientTransport::with_client(client, config);
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let mut codec = NdjsonCodec::default();
    let mut buffer = [0; 8192];
    loop {
        tokio::select! {
            read=input.read(&mut buffer)=>{
                let n=read.map_err(|_|Failure::internal("Proxy stdin failed"))?;
                if n==0 {let _=Transport::<RoleClient>::close(&mut transport).await;return if codec.finish().is_none(){Ok(0)}else{Err(Failure::arguments("Truncated NDJSON"))};}
                for message in codec.push(&buffer[..n]) {
                    let value=match message {Decoded::Message(v)=>v,Decoded::Error(_)=>return Err(Failure::arguments("Invalid NDJSON"))};
                    let message=serde_json::from_value(value).map_err(|_|Failure::arguments("Invalid MCP envelope"))?;
                    Transport::<RoleClient>::send(&mut transport,message).await.map_err(|_|Failure::internal("Engine HTTP transport failed"))?;
                }
            }
            message=Transport::<RoleClient>::receive(&mut transport)=>{
                let Some(message)=message else {return Err(Failure::internal("Engine endpoint disconnected"));};
                let mut bytes=serde_json::to_vec(&message).map_err(|_|Failure::internal("Cannot encode MCP response"))?;bytes.push(b'\n');
                output.write_all(&bytes).await.map_err(|_|Failure::internal("Proxy stdout failed"))?;output.flush().await.map_err(|_|Failure::internal("Proxy stdout failed"))?;
            }
        }
    }
}
