//! Process orchestration. Driver choice comes only from [`crate::assemble`].
use crate::{
    args::{self, Cli, Command, Transport},
    assemble::{self, Entry, Plan},
    exit::Failure,
    host::LocalHost,
};
use std::sync::Arc;

pub(crate) async fn run(cli: Cli) -> Result<u8, Failure> {
    assemble::reject_unused_scene(&cli.driver)?;
    if matches!(cli.command, Some(Command::Worker)) {
        return crate::worker::run().await;
    }
    if matches!(cli.command, Some(Command::ScriptWorker)) {
        return crate::script_worker::run().await;
    }
    if matches!(cli.command, Some(Command::Agent)) {
        return dispatch_agent(&cli).await;
    }
    if let Some(Command::TurnEnded { session, payload }) = cli.command.as_ref() {
        let mut args = Vec::new();
        if let Some(session) = session {
            args.push("--session".into());
            args.push(session.as_os_str().to_owned());
        }
        args.extend(payload.iter().cloned());
        if let Some(status) = crate::turn_ended::run(args).await {
            crate::private::unsafe_platform::exit_like(status);
        }
        return Ok(0);
    }
    let mode = args::approval_mode()?;
    match cli.command.as_ref() {
        Some(Command::NativeHost { origin, .. }) => return crate::native_host::run(origin).await,
        Some(Command::Install(args)) => {
            return crate::install_cmd::execute(args.clone(), false).await
        }
        Some(Command::Uninstall(args)) => {
            return crate::install_cmd::execute(args.clone(), true).await
        }
        Some(Command::Version) => {
            println!("gcu {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        Some(Command::Approvals(args)) => return crate::host::approvals(args.clone()).await,
        Some(Command::Resume { session }) => return crate::resume::run(session.clone()).await,
        Some(Command::Mcp {
            connect: Some(url),
            token_fd,
            transport,
            port,
            ..
        }) => {
            if matches!(transport, Transport::Http) || port.is_some() {
                return Err(Failure::arguments(
                    "--connect requires stdio proxy transport",
                ));
            }
            return crate::proxy::run(
                url,
                token_fd.ok_or_else(|| Failure::arguments("--connect requires --token-fd"))?,
            )
            .await;
        }
        Some(Command::Mcp {
            transport: Transport::Http,
            token_fd: None,
            ..
        }) => return Err(Failure::arguments("HTTP requires a secret --token-fd")),
        Some(Command::Mcp {
            transport: Transport::Stdio,
            connect: None,
            token_fd,
            port,
            ..
        }) if token_fd.is_some() || port.is_some() => {
            return Err(Failure::arguments(
                "--token-fd/--port require HTTP or --connect",
            ))
        }
        _ => (),
    }
    let entry = match &cli.command {
        None => Entry::Bare,
        Some(Command::Mcp {
            transport: Transport::Stdio,
            connect: None,
            token_fd: None,
            port: None,
            ..
        }) => Entry::McpStdio,
        _ => Entry::InProcess,
    };
    let plan = assemble::decide(assemble::production(&cli.driver, entry));
    let (scene, disclosure) = match plan {
        Plan::HelperProxy { .. } => {
            let Some(Command::Mcp {
                profile,
                turn_idle_secs,
                ..
            }) = &cli.command
            else {
                return Err(Failure::internal("Helper proxy requires mcp"));
            };
            return crate::helper::run_stdio_proxy((*profile).into(), *turn_idle_secs).await;
        }
        Plan::Onboarding { .. } => {
            #[cfg(target_os = "macos")]
            {
                crate::helper::launch_for_onboarding().await?;
                return Ok(0);
            }
            #[cfg(not(target_os = "macos"))]
            return Err(Failure::internal("Onboarding is only available on macOS"));
        }
        Plan::HelperAgent { .. } => {
            return Err(Failure::internal("Agent selection is handled earlier"))
        }
        Plan::Refuse { message } => return Err(Failure::arguments(message)),
        Plan::NativeX11 { disclosure } => {
            return dispatch_native_x11(cli, disclosure, mode).await;
        }
        Plan::Fake { scene, disclosure } => (scene, disclosure),
    };
    let driver = Arc::new(
        gcu_driver_fake::FakeDriver::from_scene(&scene)
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
    if let Some(Command::Engine {
        host_fd,
        host_socket,
        host_token_fd,
        stop_fd,
    }) = cli.command
    {
        return crate::engine::run(
            driver,
            disclosure.note(),
            host_fd,
            host_socket,
            host_token_fd,
            stop_fd,
        )
        .await;
    }
    let host = open_local_host(mode).await?;
    let oracle = driver.clone();
    dispatch_bound(cli, driver, disclosure, host, Some(oracle)).await
}

/// Load the local Host. Disk IO stays inside the five-second cap.
pub(crate) async fn open_local_host(mode: Option<String>) -> Result<LocalHost, Failure> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || LocalHost::new(crate::config::load()?, mode)),
    )
    .await
    .map_err(|_| Failure::internal("Host configuration IO timed out"))?
    .map_err(|_| Failure::internal("Host configuration failed"))?
}

/// Run doctor, call, or MCP on a driver whose Host latch is already bound.
pub(crate) async fn dispatch_bound(
    cli: Cli,
    driver: Arc<dyn gcu_core::driver::Driver>,
    disclosure: assemble::Disclosure,
    host: LocalHost,
    oracle: Option<Arc<gcu_driver_fake::FakeDriver>>,
) -> Result<u8, Failure> {
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
            if let Some(fake) = oracle.as_ref() {
                write_oracle(fake).await?;
            }
            result
        }
        Some(Command::Mcp {
            transport: Transport::Http,
            profile,
            token_fd,
            port,
            turn_idle_secs,
            ..
        }) => {
            let secret = gcu_host_protocol::auth::Secret::from_bytes(
                gcu_host_protocol::transport::token_fd(
                    token_fd.ok_or_else(|| Failure::arguments("Missing HTTP token fd"))?,
                )
                .await?,
            );
            let connection = gcu_mcp::Connection::open_host_only(
                driver.clone(),
                host.host.clone(),
                gcu_mcp::ServerConfig {
                    profile: profile.into(),
                    route_note: disclosure.note(),
                    script: crate::script_worker::runner(),
                },
            )
            .await?;
            let session = connection.session();
            let lifecycle = connection.lifecycle();
            lifecycle.set_idle_timeout(std::time::Duration::from_secs(turn_idle_secs));
            let control = crate::resume::Control::start_with_lifecycle(
                session.clone(),
                host.activity.clone(),
                lifecycle,
            )
            .await?;
            let mut endpoint = gcu_mcp::http::HttpEndpoint::start_with_secret(
                connection,
                port.unwrap_or(0),
                secret,
            )
            .await?;
            println!("{}", serde_json::json!({"mcp_url":endpoint.url()}));
            let _ = tokio::signal::ctrl_c().await;
            host.activity.latch();
            session.handle_stop().await;
            endpoint.close().await?;
            drop(control);
            host.save_summaries().await?;
            Ok(0)
        }
        Some(Command::Mcp {
            profile,
            turn_idle_secs,
            ..
        }) => {
            let connection = gcu_mcp::Connection::open_host_only(
                driver.clone(),
                host.host.clone(),
                gcu_mcp::ServerConfig {
                    profile: profile.into(),
                    route_note: disclosure.note(),
                    script: crate::script_worker::runner(),
                },
            )
            .await?;
            let session = connection.session();
            let lifecycle = connection.lifecycle();
            lifecycle.set_idle_timeout(std::time::Duration::from_secs(turn_idle_secs));
            // Register SIGINT/SIGTERM before the control directory appears: the directory is the
            // readiness signal for everything outside this process, so a Stop signal sent once it
            // exists must never hit the default action and kill the process without a latch.
            #[cfg(unix)]
            let (mut interrupt, mut term) = {
                use tokio::signal::unix::{signal, SignalKind};
                (
                    signal(SignalKind::interrupt()).ok(),
                    signal(SignalKind::terminate()).ok(),
                )
            };
            let control = crate::resume::Control::start_with_lifecycle(
                session.clone(),
                host.activity.clone(),
                lifecycle,
            )
            .await?;
            tracing::info!(directory = %control.directory().display(), "Resume control ready");
            // SIGINT/SIGTERM latch Stop and release input independently of stdio.
            let activity = host.activity.clone();
            let stop_task = tokio::spawn(async move {
                loop {
                    #[cfg(unix)]
                    tokio::select! {
                        _ = async {
                            if let Some(s) = &mut interrupt {
                                s.recv().await;
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => (),
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
            if let Some(fake) = oracle.as_ref() {
                write_oracle(fake).await?;
            }
            result.map(|_| 0).map_err(Failure::from)
        }
        Some(Command::Doctor { json }) => {
            crate::doctor::run(driver.as_ref(), &host, &disclosure, json).await
        }
        None => crate::doctor::run(driver.as_ref(), &host, &disclosure, false).await,
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

async fn dispatch_agent(cli: &Cli) -> Result<u8, Failure> {
    let plan = assemble::decide(assemble::production(&cli.driver, Entry::Agent));
    match plan {
        Plan::Fake { scene, .. } => crate::helper::run_agent(&scene, true).await,
        Plan::HelperAgent { .. } => crate::helper::run_agent("form", false).await,
        Plan::Refuse { message } => Err(Failure::arguments(message)),
        Plan::HelperProxy { .. } | Plan::Onboarding { .. } | Plan::NativeX11 { .. } => Err(
            Failure::arguments("Linux native X11 does not use the macOS helper agent entry"),
        ),
    }
}

async fn dispatch_native_x11(
    cli: Cli,
    disclosure: assemble::Disclosure,
    mode: Option<String>,
) -> Result<u8, Failure> {
    #[cfg(all(feature = "native-x11", target_os = "linux"))]
    {
        crate::native_x11::dispatch(cli, disclosure, mode).await
    }
    #[cfg(not(all(feature = "native-x11", target_os = "linux")))]
    {
        let _ = (cli, disclosure, mode);
        Err(Failure::internal(
            "native-x11 was selected without its compiled route",
        ))
    }
}
