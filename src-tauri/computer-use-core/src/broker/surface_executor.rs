//! Broker-owned adapter registry. A missing product surface fails closed and
//! never resolves to Desktop.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::adapter::{ComputerUseAdapter, SurfaceKind};
use crate::error::BrokerError;

#[derive(Clone)]
pub(crate) struct SurfaceExecutor {
    pub(crate) generation: u64,
    pub(crate) adapter: Arc<dyn ComputerUseAdapter>,
}

struct RegistryState {
    next_generation: u64,
    entries: HashMap<SurfaceKind, SurfaceExecutor>,
}

pub(crate) struct SurfaceExecutorRegistry {
    inner: RwLock<RegistryState>,
}

impl SurfaceExecutorRegistry {
    pub(crate) fn new(desktop: Arc<dyn ComputerUseAdapter>) -> Self {
        let mut entries = HashMap::new();
        entries.insert(
            SurfaceKind::Desktop,
            SurfaceExecutor {
                generation: 1,
                adapter: desktop,
            },
        );
        Self {
            inner: RwLock::new(RegistryState {
                next_generation: 2,
                entries,
            }),
        }
    }

    pub(crate) fn register(
        &self,
        surface: SurfaceKind,
        adapter: Arc<dyn ComputerUseAdapter>,
    ) -> Result<u64, BrokerError> {
        let mut state = self.inner.write();
        if let Some(existing) = state.entries.get(&surface) {
            if Arc::ptr_eq(&existing.adapter, &adapter) {
                return Ok(existing.generation);
            }
            return Err(BrokerError::Schema(format!(
                "surface executor already registered for {}",
                surface.as_wire()
            )));
        }
        let generation = state.next_generation;
        state.next_generation = state.next_generation.saturating_add(1);
        state.entries.insert(
            surface,
            SurfaceExecutor {
                generation,
                adapter,
            },
        );
        Ok(generation)
    }

    pub(crate) fn resolve(&self, surface: SurfaceKind) -> Result<SurfaceExecutor, BrokerError> {
        self.inner
            .read()
            .entries
            .get(&surface)
            .cloned()
            .ok_or(BrokerError::SurfaceUnavailable { surface })
    }
}
