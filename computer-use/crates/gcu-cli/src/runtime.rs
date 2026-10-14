//! Process orchestration. No real driver is silently substituted with fake.
use crate::{
    args::{self, Cli, Command, Transport},
    exit::Failure,
    host::LocalHost,
};
use std::sync::Arc;

pub(crate) async fn run(cli: Cli) -> Result<u8, Failure> {
    let mode = args::approval_mode()?;
    match cli.command.as_ref() {
        Some(Command::Version) => {
            println!("gcu {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        Some(Command::Resume { session }) => return crate::resume::run(session.clone()).await,
        Some(Command::Mcp {
            transport,
            connect,
            token_fd,
            port,
            ..
        }) if matches!(transport, Transport::Http)
            || connect.is_some()
            || token_fd.is_some()
            || port.is_some() =>
        {
            return Err(Failure::arguments(
                "HTTP/--connect/--token-fd/--port are not implemented; P0-12",
            ));
        }
        _ => (),
    }
    args::require_fake(&cli.driver)?;
    let driver = Arc::new(
        gcu_driver_fake::FakeDriver::from_scene(&cli.driver.scene)
            .and_then(|d| {
                d.with_image_digest([0x47; 32]).map_err(|_| {
                    gcu_driver_fake::scene::SceneError::Invalid("fake identity unavailable")
                })
            })
            .map_err(|_| Failure::arguments("Unknown or invalid fake scene"))?,
    );
    #[cfg(all(feature = "eval-autoapprove", debug_assertions))]
    if let Ok(code) = std::env::var("GCU_EVAL_READ_ERROR") {
        use gcu_core::driver::DriverError;
        let error = match code.as_str() {
            "permission_missing" => DriverError::PermissionMissing {
                permission: "fake fixture".into(),
            },
            "ambiguous_app" => DriverError::AmbiguousApp,
            _ => return Err(Failure::arguments("Unknown evaluation read fault")),
        };
        driver
            .inject(gcu_driver_fake::faults::Fault::Error(error))
            .await
            .map_err(|_| Failure::internal("Cannot inject evaluation fault"))?;
    }
    let host = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || LocalHost::new(crate::config::load()?, mode)),
    )
    .await
    .map_err(|_| Failure::internal("Host configuration IO timed out"))?
    .map_err(|_| Failure::internal("Host configuration failed"))??;
    crate::private::work(|| {
        crate::call::cleanup_artifacts();
        Ok(())
    })
    .await?;
    match cli.command {
        Some(Command::Call { tool, args, json }) => {
            let connection = gcu_mcp::Connection::open_host_only(
                driver.clone(),
                host.host.clone(),
                Default::default(),
            )
            .await?;
            let result = crate::call::run(connection, &tool, &args, json).await;
            host.save_summaries().await?;
            write_oracle(&driver).await?;
            result
        }
        Some(Command::Mcp { profile, .. }) => {
            let connection = gcu_mcp::Connection::open_host_only(
                driver.clone(),
                host.host.clone(),
                gcu_mcp::ServerConfig {
                    profile: profile.into(),
                },
            )
            .await?;
            let session = connection.session();
            let control =
                crate::resume::Control::start(session.clone(), host.activity.clone()).await?;
            tracing::info!(directory = %control.directory().display(), "Resume control ready");
            // SIGINT/SIGTERM latch Stop and release input independently of stdio.
            let activity = host.activity.clone();
            let stop_task = tokio::spawn(async move {
                #[cfg(unix)]
                let mut term = {
                    use tokio::signal::unix::{signal, SignalKind};
                    signal(SignalKind::terminate()).ok()
                };
                loop {
                    #[cfg(unix)]
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => (),
                        _ = async {
                            if let Some(s) = &mut term {
                                s.recv().await;
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => ()
                    }
                    #[cfg(windows)]
                    if tokio::signal::ctrl_c().await.is_err() {
                        return;
                    }
                    activity.latch();
                    let persist_activity = activity.clone();
                    let persist =
                        tokio::task::spawn_blocking(move || persist_activity.persist_stop());
                    session.handle_stop().await;
                    if !matches!(persist.await, Ok(Ok(()))) {
                        tracing::error!("Stop persistence failed; memory latch remains set");
                    }
                }
            });
            let result = connection.serve_stdio().await;
            stop_task.abort();
            drop(control);
            host.save_summaries().await?;
            write_oracle(&driver).await?;
            result.map(|_| 0).map_err(Failure::from)
        }
        Some(Command::Doctor { json }) => crate::doctor::run(driver.as_ref(), &host, json).await,
        None => crate::doctor::run(driver.as_ref(), &host, false).await,
        _ => Err(Failure::internal("Invalid CLI dispatch")),
    }
}
async fn write_oracle(driver: &gcu_driver_fake::FakeDriver) -> Result<(), Failure> {
    #[cfg(all(feature = "eval-autoapprove", debug_assertions))]
    if let Some(path) = std::env::var_os("GCU_EVAL_ORACLE_FILE") {
        let o = driver
            .oracle()
            .await
            .map_err(|_| Failure::internal("Fake oracle failed"))?;
        let value = serde_json::json!({"presses":o.presses,"text":o.text,"held_input":o.held_input,"foreground":o.foreground});
        crate::private::work(move || {
            crate::private::write_secret(std::path::Path::new(&path), value.to_string().as_bytes())
                .map_err(|_| Failure::internal("Cannot write evaluation oracle"))
        })
        .await?;
    }
    let _ = driver;
    Ok(())
}
