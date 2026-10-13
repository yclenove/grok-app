//! Native-compatible download paths; the Broker destination remains authoritative.

use super::{journal, windows_path, Mutex, Path, PathBuf};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;

pub(super) struct NativeDestination {
    path: PathBuf,
    buffered: bool,
    private: Mutex<Option<BufferOwnership>>,
}

enum BufferOwnership {
    Unpublished(journal::PrivateBuffer),
    Published(Box<journal::Lease>),
}

impl NativeDestination {
    pub(super) fn new(destination: &Path) -> Result<Self, String> {
        let legacy = PathBuf::from(windows_path(destination));
        // WebView2 cannot create extended-namespace paths. Long paths and
        // names with different DOS semantics need a private native buffer;
        // shortening the Broker root or changing its filename is not safe.
        if legacy.as_os_str().to_string_lossy().encode_utf16().count() < 260
            && legacy.parent().is_none_or(|parent| {
                parent.as_os_str().to_string_lossy().encode_utf16().count() < 248
            })
            && legacy.components().all(|component| {
                !component
                    .as_os_str()
                    .to_string_lossy()
                    .ends_with(['.', ' '])
            })
        {
            return Ok(Self {
                path: legacy,
                buffered: false,
                private: Mutex::new(None),
            });
        }
        let buffer = journal::PrivateBuffer::create()?;
        let native = Self {
            path: buffer.path(),
            buffered: true,
            private: Mutex::new(Some(BufferOwnership::Unpublished(buffer))),
        };
        if native
            .path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .count()
            >= 260
        {
            return Err("WebView private download path is too long".into());
        }
        Ok(native)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn buffered(&self) -> bool {
        self.buffered
    }

    pub(super) fn bind(
        &mut self,
        owners: &journal::Owners,
        instance: u64,
        id: u64,
    ) -> Result<(), String> {
        let Some(ownership) = self.private.get_mut().take() else {
            return Ok(());
        };
        match ownership {
            BufferOwnership::Unpublished(buffer) => {
                let lease = journal::Lease::publish(owners, instance, id, buffer)?;
                *self.private.get_mut() = Some(BufferOwnership::Published(Box::new(lease)));
                Ok(())
            }
            BufferOwnership::Published(lease) => {
                *self.private.get_mut() = Some(BufferOwnership::Published(lease));
                Err("native download buffer already published".into())
            }
        }
    }

    pub(super) fn copy_into(
        &self,
        destination: &Path,
        check: impl Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        check()?;
        if !self.buffered() {
            return Ok(());
        }
        let metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|_| "WebView private download file unavailable")?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
            return Err("WebView private download file changed".into());
        }
        let limit = grok_computer_use_core::staging::MAX_DOWNLOAD_BYTES;
        if metadata.len() == 0 || metadata.len() > limit {
            return Err("WebView downloaded file size is not allowed".into());
        }
        let mut input =
            File::open(&self.path).map_err(|_| "WebView private download file unavailable")?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|_| "WebView Broker staging destination unavailable")?;
        // Only a file created by this transfer is removed on failure. The
        // source remains owned until the original callbacks/caller retire.
        let mut rollback = CopyOutput {
            path: destination.to_path_buf(),
            keep: false,
        };
        let copied = (|| {
            let mut buffer = [0u8; 64 * 1024];
            let mut bytes = 0u64;
            loop {
                check()?;
                let read = input
                    .read(&mut buffer)
                    .map_err(|_| "WebView download transfer failed")?;
                if read == 0 {
                    break;
                }
                bytes += read as u64;
                if bytes > limit {
                    return Err("WebView downloaded file size is not allowed".to_string());
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(|_| "WebView download transfer failed")?;
            }
            output
                .flush()
                .map_err(|_| "WebView download transfer failed")?;
            if bytes != metadata.len() {
                return Err("WebView private download file changed".to_string());
            }
            check()
        })();
        drop(output);
        copied?;
        rollback.keep = true;
        Ok(())
    }

    pub(super) fn cleanup(&self) {
        let mut private = self.private.lock();
        let result = match private.as_mut() {
            Some(BufferOwnership::Unpublished(buffer)) => buffer.cleanup(),
            Some(BufferOwnership::Published(lease)) => lease.cleanup(),
            None => Ok(()),
        };
        if let Err(error) = result {
            // Preserve the immutable journal. This is not a business-success
            // result; explicit Host maintenance can retry after owner retirement.
            eprintln!("native_download_cleanup_retained: {error}");
        }
    }
}

impl Drop for NativeDestination {
    fn drop(&mut self) {
        self.cleanup();
    }
}

struct CopyOutput {
    path: PathBuf,
    keep: bool,
}

impl Drop for CopyOutput {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests;
