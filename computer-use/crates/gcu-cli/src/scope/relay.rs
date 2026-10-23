//! Fixed data-only relay. It has no scope-control importer or parser.
use crate::exit::Failure;
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) async fn run(address: SocketAddr) -> Result<u8, Failure> {
    if address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
        return Err(Failure::arguments(
            "Scope relay requires an exact IPv4 loopback data socket",
        ));
    }
    let socket = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::TcpStream::connect(address),
    )
    .await
    .map_err(|_| Failure::internal("Scope data connection timed out"))?
    .map_err(|_| Failure::internal("Scope data connection refused"))?;
    let (mut read, mut write) = socket.into_split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    // Raw copying preserves MCP framing. Control-looking bytes can only reach
    // the MCP parser. Data EOF also cancels a pending stdin read immediately.
    tokio::select! {
        result = async {tokio::io::copy(&mut stdin,&mut write).await?;write.shutdown().await} => result,
        result = async {
            let mut bytes = [0;8192];
            loop {let n=read.read(&mut bytes).await?;if n==0 {return Ok(());}
                stdout.write_all(&bytes[..n]).await?;stdout.flush().await?;}
        } => result,
    }.map_err(|_:std::io::Error|Failure::internal("Scope data relay disconnected"))?;
    Ok(0)
}
