//! Filesystem access, rooted so tests can point it at a fake sysfs tree.

use crate::{Error, Result};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Hw {
    root: PathBuf,
}

impl Hw {
    /// The real machine.
    pub fn system() -> Self {
        Hw { root: PathBuf::from("/") }
    }

    /// A fake tree for tests: every absolute path is resolved under `root`.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Hw { root: root.into() }
    }

    pub fn path(&self, abs: &str) -> PathBuf {
        self.root.join(abs.trim_start_matches('/'))
    }

    pub fn exists(&self, abs: &str) -> bool {
        self.path(abs).exists()
    }

    /// Trimmed file contents, or None when missing, unreadable or empty.
    pub fn read(&self, abs: &str) -> Option<String> {
        let text = fs::read_to_string(self.path(abs)).ok()?;
        let t = text.trim();
        if t.is_empty() { None } else { Some(t.to_owned()) }
    }

    pub fn read_u64(&self, abs: &str) -> Option<u64> {
        self.read(abs)?.parse().ok()
    }

    /// Whitespace-separated words, used for msi-ec `available_*` lists.
    pub fn read_words(&self, abs: &str) -> Vec<String> {
        self.read(abs)
            .map(|s| s.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Names of the entries in a directory, sorted.
    pub fn list_dir(&self, abs: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path(abs))
            .map(|it| it.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Write a sysfs attribute without checking the result.
    pub fn write_raw(&self, abs: &str, value: &str) -> Result<()> {
        let mut file = OpenOptions::new().write(true).truncate(true).open(self.path(abs))
            .map_err(|e| Error::new(format!("Cannot open {abs} for writing: {e}")))?;
        use std::io::Write;
        file.write_all(value.as_bytes())
            .map_err(|e| Error::new(format!("Writing {abs} failed: {e}")))
    }

    /// Write a sysfs attribute and confirm the kernel reports the same value back.
    pub fn write_verified(&self, abs: &str, value: &str) -> Result<()> {
        self.write_raw(abs, value)?;
        match self.read(abs) {
            Some(got) if got == value => Ok(()),
            got => Err(Error::new(format!(
                "{abs} reads back {:?} after writing {value:?}", got.unwrap_or_default()
            ))),
        }
    }

    pub fn read_at(&self, abs: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let file = fs::File::open(self.path(abs))
            .map_err(|e| Error::new(format!("Cannot open {abs}: {e}")))?;
        let mut buf = vec![0u8; len];
        let n = file.read_at(&mut buf, offset)
            .map_err(|e| Error::new(format!("Read of {abs} at 0x{offset:02X} failed: {e}")))?;
        if n != len {
            return Err(Error::new(format!("Short read of {abs} at 0x{offset:02X}")));
        }
        Ok(buf)
    }

    pub fn write_at(&self, abs: &str, offset: u64, data: &[u8]) -> Result<()> {
        let file = OpenOptions::new().write(true).open(self.path(abs))
            .map_err(|e| Error::new(format!("Cannot open {abs} for writing: {e}")))?;
        let n = file.write_at(data, offset)
            .map_err(|e| Error::new(format!("Write to {abs} at 0x{offset:02X} failed: {e}")))?;
        if n != data.len() {
            return Err(Error::new(format!("Short write to {abs} at 0x{offset:02X}")));
        }
        if self.read_at(abs, offset, data.len())? != data {
            return Err(Error::new(format!("{abs} at 0x{offset:02X} did not read back what was written")));
        }
        Ok(())
    }

    /// Atomically replace a private (0600) file: write a temp file, fsync, rename.
    pub fn save_private(&self, abs: &str, contents: &str) -> Result<()> {
        let target = self.path(abs);
        let dir = target.parent().ok_or_else(|| Error::new("State path has no parent"))?;
        fs::create_dir_all(dir).map_err(|e| Error::new(format!("Cannot create {}: {e}", dir.display())))?;
        let tmp = dir.join(format!(".{}.tmp", target.file_name().unwrap().to_string_lossy()));
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let _ = fs::remove_file(&tmp);
            let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)
                .map_err(|e| Error::new(format!("Cannot write {}: {e}", tmp.display())))?;
            f.write_all(contents.as_bytes()).and_then(|_| f.sync_all())
                .map_err(|e| Error::new(format!("Cannot write {}: {e}", tmp.display())))?;
        }
        fs::rename(&tmp, &target).map_err(|e| Error::new(format!("Cannot save {abs}: {e}")))?;
        if let Ok(d) = fs::File::open(dir) { let _ = d.sync_all(); }
        Ok(())
    }

    /// Name of the target of a symlink, e.g. the driver a PCI device is bound to.
    pub fn link_name(&self, abs: &str) -> Option<String> {
        let target = fs::read_link(self.path(abs)).ok()?;
        Some(target.file_name()?.to_string_lossy().into_owned())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
