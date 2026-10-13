//! Original directory and ancestor handles travel with the journal lease.

use super::*;

pub(in crate::computer_use::webview::download::windows) struct PrivateBuffer {
    pub(super) directory: PathBuf,
    pub(super) directory_pin: Option<File>,
    pub(super) pins: Vec<File>,
    pub(super) transaction: String,
    pub(super) retain: bool,
}

impl PrivateBuffer {
    pub(in crate::computer_use::webview::download::windows) fn create() -> Result<Self, String> {
        Self::in_parent(&std::env::temp_dir())
    }

    pub(super) fn in_parent(parent: &Path) -> Result<Self, String> {
        let pins = safety::parents(parent, false)?;
        let transaction = uuid::Uuid::new_v4().simple().to_string();
        let directory = parent.join(format!("grok-cu-download-{transaction}"));
        std::fs::create_dir(&directory).map_err(safety::error)?;
        let directory_pin = Some(safety::directory(&directory, true)?);
        Ok(Self {
            directory,
            directory_pin,
            pins,
            transaction,
            retain: false,
        })
    }

    pub(in crate::computer_use::webview::download::windows) fn path(&self) -> PathBuf {
        self.directory.join("download.bin")
    }

    pub(in crate::computer_use::webview::download::windows) fn cleanup(
        &mut self,
    ) -> Result<(), String> {
        if self.retain || self.directory_pin.is_none() {
            return Ok(());
        }
        let data = self.path();
        if data.try_exists().map_err(safety::error)? {
            let mut original = Some(safety::file(&data, true)?);
            safety::delete(&mut original)?;
        }
        safety::delete(&mut self.directory_pin)
    }
}

impl Drop for PrivateBuffer {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
