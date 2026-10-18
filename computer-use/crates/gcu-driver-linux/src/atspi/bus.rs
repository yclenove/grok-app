//! Real live AT-SPI D-Bus source implementation.

use super::source::AtspiSource;
use async_trait::async_trait;
use gcu_core::{
    driver::DriverError,
    model::{NativeHandle, NodeAction, NodeValue, Rect, TextExtras},
};
use smol_str::SmolStr;
use std::{collections::HashMap, sync::Arc, time::Duration};

/// Per D-Bus call timeout to prevent hanging on unresponsive applications.
pub const DBUS_CALL_TIMEOUT: Duration = Duration::from_millis(500);

/// D-Bus addressable AT-SPI node reference.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AtspiNodeRef {
    /// D-Bus service name.
    pub destination: String,
    /// D-Bus object path.
    pub path: String,
}

/// Real AT-SPI D-Bus source backed by `atspi` and `zbus`.
pub struct AtspiBus {
    connection: zbus::Connection,
}

fn bus_dest(dest: &str) -> Result<zbus::names::OwnedBusName, DriverError> {
    dest.to_string()
        .try_into()
        .map_err(|e: zbus::names::Error| DriverError::Native {
            message: e.to_string(),
            maybe_executed: false,
        })
}

fn obj_path(path: &str) -> Result<zbus::zvariant::OwnedObjectPath, DriverError> {
    path.try_into()
        .map_err(|e: zbus::zvariant::Error| DriverError::Native {
            message: format!("invalid object path: {e}"),
            maybe_executed: false,
        })
}

impl AtspiBus {
    /// Create a new `AtspiBus` from an active D-Bus connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    /// Access the underlying D-Bus connection.
    pub fn connection(&self) -> &zbus::Connection {
        &self.connection
    }

    /// Process id published as `org.a11y.atspi.Application.Id`, when positive.
    ///
    /// Failures and a missing interface return `None`. Callers must not invent a pid.
    pub async fn application_pid(&self, node: &AtspiNodeRef) -> Option<u32> {
        let dest = bus_dest(&node.destination).ok()?;
        let path = obj_path(&node.path).ok()?;
        let built = atspi::proxy::application::ApplicationProxy::builder(&self.connection)
            .destination(dest)
            .ok()?
            .path(path)
            .ok()?
            .build();
        let proxy = tokio::time::timeout(DBUS_CALL_TIMEOUT, built)
            .await
            .ok()?
            .ok()?;
        let id = tokio::time::timeout(DBUS_CALL_TIMEOUT, proxy.id())
            .await
            .ok()?
            .ok()?;
        u32::try_from(id).ok().filter(|pid| *pid > 0)
    }

    async fn accessible_proxy(
        &self,
        node: &AtspiNodeRef,
    ) -> Result<atspi::proxy::accessible::AccessibleProxy<'_>, DriverError> {
        let dest = bus_dest(&node.destination)?;
        let path = obj_path(&node.path)?;
        atspi::proxy::accessible::AccessibleProxy::builder(&self.connection)
            .destination(dest)
            .map_err(|e| DriverError::Native {
                message: e.to_string(),
                maybe_executed: false,
            })?
            .path(path)
            .map_err(|e| DriverError::Native {
                message: e.to_string(),
                maybe_executed: false,
            })?
            .build()
            .await
            .map_err(|e| DriverError::Native {
                message: e.to_string(),
                maybe_executed: false,
            })
    }
}

#[async_trait]
impl AtspiSource for AtspiBus {
    type Node = AtspiNodeRef;

    async fn children(&self, node: &Self::Node) -> Result<Vec<Self::Node>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let proxy = self.accessible_proxy(node).await?;
            let children = proxy
                .get_children()
                .await
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?;
            Ok(children
                .into_iter()
                .map(|c| AtspiNodeRef {
                    destination: c
                        .name()
                        .map_or_else(|| node.destination.clone(), |n| n.as_str().to_string()),
                    path: c.path().as_str().to_string(),
                })
                .collect())
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn role(&self, node: &Self::Node) -> Result<atspi::Role, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let proxy = self.accessible_proxy(node).await?;
            proxy.get_role().await.map_err(|e| DriverError::Native {
                message: e.to_string(),
                maybe_executed: false,
            })
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn name(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let proxy = self.accessible_proxy(node).await?;
            proxy
                .name()
                .await
                .map(Some)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn description(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let proxy = self.accessible_proxy(node).await?;
            proxy
                .description()
                .await
                .map(Some)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn help(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let proxy = self.accessible_proxy(node).await?;
            proxy
                .help_text()
                .await
                .map(Some)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn identifier(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let proxy = self.accessible_proxy(node).await?;
            proxy
                .accessible_id()
                .await
                .map(Some)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn attributes(&self, node: &Self::Node) -> Result<HashMap<String, String>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let proxy = self.accessible_proxy(node).await?;
            proxy
                .get_attributes()
                .await
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn states(&self, node: &Self::Node) -> Result<atspi::StateSet, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let proxy = self.accessible_proxy(node).await?;
            proxy.get_state().await.map_err(|e| DriverError::Native {
                message: e.to_string(),
                maybe_executed: false,
            })
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn value(&self, node: &Self::Node) -> Result<Option<NodeValue>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let dest = bus_dest(&node.destination)?;
            let path = obj_path(&node.path)?;
            let proxy = atspi::proxy::value::ValueProxy::builder(&self.connection)
                .destination(dest)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?
                .path(path)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?
                .build()
                .await;

            if let Ok(v_proxy) = proxy {
                if let Ok(curr) = v_proxy.current_value().await {
                    let min = v_proxy.minimum_value().await.unwrap_or(0.0);
                    let max = v_proxy.maximum_value().await.unwrap_or(100.0);
                    return Ok(Some(NodeValue::Range {
                        value: curr,
                        min,
                        max,
                    }));
                }
            }
            Ok(None)
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn extents(&self, node: &Self::Node) -> Result<Option<Rect>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let dest = bus_dest(&node.destination)?;
            let path = obj_path(&node.path)?;
            let proxy = atspi::proxy::component::ComponentProxy::builder(&self.connection)
                .destination(dest)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?
                .path(path)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?
                .build()
                .await;

            if let Ok(comp_proxy) = proxy {
                if let Ok((x, y, w, h)) = comp_proxy.get_extents(atspi::CoordType::Window).await {
                    return Ok(Some(Rect {
                        x: f64::from(x),
                        y: f64::from(y),
                        w: f64::from(w),
                        h: f64::from(h),
                    }));
                }
            }
            Ok(None)
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn actions(&self, node: &Self::Node) -> Result<Vec<NodeAction>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let dest = bus_dest(&node.destination)?;
            let path = obj_path(&node.path)?;
            let proxy = atspi::proxy::action::ActionProxy::builder(&self.connection)
                .destination(dest)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?
                .path(path)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?
                .build()
                .await;

            if let Ok(act_proxy) = proxy {
                if let Ok(acts) = act_proxy.get_actions().await {
                    let mut list = Vec::new();
                    for a in acts {
                        let lower = a.name.to_ascii_lowercase();
                        let mapped = match lower.as_str() {
                            "click" | "press" | "activate" => NodeAction::Press,
                            "open" | "showmenu" | "menu" => NodeAction::ShowMenu,
                            "select" => NodeAction::Select,
                            "toggle" => NodeAction::Toggle,
                            _ => NodeAction::Native(SmolStr::new(a.name)),
                        };
                        list.push(mapped);
                    }
                    return Ok(list);
                }
            }
            Ok(Vec::new())
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn text_extras(
        &self,
        node: &Self::Node,
        text_limit: Option<usize>,
    ) -> Result<Option<TextExtras>, DriverError> {
        tokio::time::timeout(DBUS_CALL_TIMEOUT, async {
            let dest = bus_dest(&node.destination)?;
            let path = obj_path(&node.path)?;
            let proxy = atspi::proxy::text::TextProxy::builder(&self.connection)
                .destination(dest)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?
                .path(path)
                .map_err(|e| DriverError::Native {
                    message: e.to_string(),
                    maybe_executed: false,
                })?
                .build()
                .await;

            if let Ok(text_proxy) = proxy {
                if let Ok(full_text) = text_proxy.get_text(0, -1).await {
                    let bounded = match text_limit {
                        Some(lim) => full_text.chars().take(lim).collect(),
                        None => full_text,
                    };
                    return Ok(Some(TextExtras {
                        selected: None,
                        visible: None,
                        text: Some(bounded),
                    }));
                }
            }
            Ok(None)
        })
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
    }

    async fn url(&self, _node: &Self::Node) -> Result<Option<String>, DriverError> {
        Ok(None)
    }

    fn handle(&self, node: &Self::Node) -> NativeHandle {
        NativeHandle::new(Arc::new(node.clone()))
    }
}
