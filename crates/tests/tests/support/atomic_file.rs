use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A completed same-directory temporary file that removes itself unless renamed.
struct TempSnapshotFile {
    path: PathBuf,
    file: Option<File>,
}

impl TempSnapshotFile {
    fn create(target: &Path) -> Result<Self, String> {
        let parent = target
            .parent()
            .expect("snapshot path always has a parent directory");
        let file_name = target
            .file_name()
            .and_then(|name| name.to_str())
            .expect("snapshot file name is valid UTF-8");

        loop {
            let nonce = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("create {}: {error}", path.display())),
            }
        }
    }

    fn write(mut self, contents: &str) -> Result<Self, String> {
        let file = self.file.as_mut().expect("temporary file remains open");
        file.write_all(contents.as_bytes())
            .map_err(|error| format!("write {}: {error}", self.path.display()))?;
        file.flush()
            .map_err(|error| format!("flush {}: {error}", self.path.display()))?;
        self.file.take();
        Ok(self)
    }

    fn replace(mut self, target: &Path) -> Result<(), String> {
        fs::rename(&self.path, target).map_err(|error| {
            format!(
                "replace {} with {}: {error}",
                target.display(),
                self.path.display()
            )
        })?;
        self.path = PathBuf::new();
        Ok(())
    }
}

impl Drop for TempSnapshotFile {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Replace `path` atomically after all bytes have been written and flushed.
///
/// This guarantees readers see either the old complete snapshot or the new one.
/// It does not promise crash durability; the snapshot workflow does not require
/// an `fsync` of the file and parent directory.
pub fn replace(path: &Path, contents: &str) -> Result<(), String> {
    TempSnapshotFile::create(path)?
        .write(contents)?
        .replace(path)
}
