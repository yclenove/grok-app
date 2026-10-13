//! User choice runs on the existing owned control registry, not an IPC future.
use grok_computer_use_core::adapter::{HostSelectedUploadFile, MAX_UPLOAD_BYTES};
use grok_computer_use_core::broker::{
    HostUploadCancel, HostUploadSelection, WebViewUploadCompletion,
};
use std::fs::File;
use std::path::Path;

struct Waiter {
    cancel: HostUploadCancel,
    completed: bool,
}
impl Drop for Waiter {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel.fence();
        }
    }
}

pub(crate) async fn choose(
    selection: HostUploadSelection,
    dialog: rfd::FileDialog,
) -> Result<WebViewUploadCompletion, String> {
    let cancel = selection.cancel_handle();
    let mut waiter = Waiter {
        cancel: cancel.clone(),
        completed: false,
    };
    let result = super::task_controls::run_fenced(
        move || {
            pick(selection, dialog)
                .map(|result| super::task_controls::TaskControlOutput::Uploaded(Box::new(result)))
        },
        move || cancel.fence(),
    )
    .await?;
    waiter.completed = true;
    match result {
        super::task_controls::TaskControlOutput::Uploaded(result) => Ok(*result),
        _ => Err("Computer Use upload reply changed".into()),
    }
}

fn pick(
    selection: HostUploadSelection,
    dialog: rfd::FileDialog,
) -> Result<WebViewUploadCompletion, String> {
    selection
        .require_current()
        .map_err(|_| "Computer Use file choice retired")?;
    // rfd 0.15 has no public close/cancel handle. Keep this synchronous call's
    // real owner registered until the native dialog returns, including on Quit.
    // Never drop a dialog future and report that physical cleanup finished.
    let Some(path) = dialog.pick_file() else {
        return Ok(selection.cancel());
    };
    selection
        .require_current()
        .map_err(|_| "Computer Use file choice retired")?;
    let file = read_choice(&path)?;
    selection
        .assign(file)
        .map_err(|_| "Computer Use original file assignment retired".into())
}

fn read_choice(path: &Path) -> Result<HostSelectedUploadFile, String> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Computer Use selected filename is unsupported")?
        .to_string();
    let original =
        File::open(path).map_err(|_| "Computer Use selected file could not be opened")?;
    let metadata = original
        .metadata()
        .map_err(|_| "Computer Use selected file metadata unavailable")?;
    if !metadata.is_file() {
        return Err("Computer Use upload requires a regular file".into());
    }
    if metadata.len() > MAX_UPLOAD_BYTES as u64 {
        return Err("Computer Use selected file exceeds 8 MiB".into());
    }
    // The initial bound protects against normal oversize files; bounded reading
    // also catches a file growing after metadata without truncating its upload.
    // No content, original path, or OS error details enter result/trace/ledger.
    let mime = mime_guess::from_path(&filename).first_or_octet_stream();
    HostSelectedUploadFile::from_host_reader(filename, mime.essence_str().into(), original)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_content_read_is_regular_bounded_and_redacted() {
        let directory =
            std::env::temp_dir().join(format!("cu-host-upload-read-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("中文.bin");
        std::fs::write(&path, [0, 255, 17, 4]).unwrap();
        let content = read_choice(&path).unwrap();
        assert_eq!(content.filename(), "中文.bin");
        assert_eq!(content.mime(), "application/octet-stream");
        assert_eq!(content.bytes(), [0, 255, 17, 4]);
        assert!(!format!("{content:?}").contains("中文"));
        let image_path = directory.join("photo.PNG");
        std::fs::write(&image_path, [137, 80, 78, 71]).unwrap();
        assert_eq!(read_choice(&image_path).unwrap().mime(), "image/png");
        std::fs::remove_file(image_path).unwrap();
        std::fs::write(&path, []).unwrap();
        assert!(read_choice(&path).unwrap().bytes().is_empty());
        let original = std::fs::File::options().write(true).open(&path).unwrap();
        original.set_len((MAX_UPLOAD_BYTES + 1) as u64).unwrap();
        drop(original);
        assert!(read_choice(&path).is_err());
        let error = read_choice(&directory.join("private-missing-file")).unwrap_err();
        assert!(
            !error.contains("private") && !error.contains(&directory.to_string_lossy().to_string())
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
