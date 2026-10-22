//! Paper DirectoryLock: held for the entire writable world-storage lifetime.
//! Java FileChannel uses POSIX record locks on Unix, not flock. Record locks
//! are process-wide and closing ANY descriptor for the inode releases them;
//! keep duplicate descriptors alive until the owning guard is released.
use rustix::fs::{FlockOperation, fcntl_lock};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

type FileIdentity = (u64, u64);
static LOCKS: LazyLock<Mutex<HashMap<FileIdentity, Vec<File>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Non-clonable ownership of an OS world lock. Do not independently open or
/// close session.lock elsewhere in this process: POSIX locks are per-process.
#[derive(Debug)]
pub struct DirectoryLock {
    identity: FileIdentity,
    directory: PathBuf,
}

fn already_locked(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!(
            "{}: already locked (possibly by other Minecraft instance?)",
            path.display()
        ),
    )
}

impl DirectoryLock {
    /// Paper's non-creating lock probe. Permission denial is treated as locked;
    /// absence is unlocked. Consult the local registry before touching a live
    /// descriptor because POSIX does not report conflicts within one process.
    pub fn is_locked(directory: &Path) -> io::Result<bool> {
        let path = directory.join("session.lock");
        let mut locks = LOCKS
            .lock()
            .map_err(|_| io::Error::other("world lock registry poisoned"))?;
        if let Ok(metadata) = std::fs::metadata(&path)
            && locks.contains_key(&(metadata.dev(), metadata.ino()))
        {
            return Ok(true);
        }
        let file = match OpenOptions::new().write(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Ok(true),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if let Some(descriptors) = locks.get_mut(&(metadata.dev(), metadata.ino())) {
            descriptors.push(file);
            return Ok(true);
        }
        match fcntl_lock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {
                fcntl_lock(&file, FlockOperation::Unlock)?;
                Ok(false)
            }
            Err(error)
                if error == rustix::io::Errno::AGAIN || error == rustix::io::Errno::ACCESS =>
            {
                Ok(true)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn create(directory: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let path = directory.join("session.lock");
        let mut locks = LOCKS
            .lock()
            .map_err(|_| io::Error::other("world lock registry poisoned"))?;
        // Fast duplicate rejection also handles symlinks and hard links without
        // opening another descriptor and accidentally releasing our own lock.
        if let Ok(metadata) = std::fs::metadata(&path)
            && locks.contains_key(&(metadata.dev(), metadata.ino()))
        {
            return Err(already_locked(&path));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let metadata = file.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        // A path may have been replaced between stat and open. Never close an
        // alias of a currently locked inode on the error path.
        if let Some(descriptors) = locks.get_mut(&identity) {
            descriptors.push(file);
            return Err(already_locked(&path));
        }
        file.write_all("☃".as_bytes())?;
        file.sync_all()?;
        fcntl_lock(&file, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
            if error == rustix::io::Errno::AGAIN || error == rustix::io::Errno::ACCESS {
                already_locked(&path)
            } else {
                io::Error::from(error)
            }
        })?;
        locks.insert(identity, vec![file]);
        Ok(Self {
            identity,
            directory: directory.to_path_buf(),
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        // Even after a panic, close the held descriptors and release the lock.
        let mut locks = LOCKS.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(files) = locks.remove(&self.identity) {
            if let Some(file) = files.first()
                && let Err(error) = fcntl_lock(file, FlockOperation::Unlock)
            {
                tracing::error!(%error, "Error releasing world session lock");
            }
            drop(files);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run with the actual pinned Paper runtime and compiled DirectoryLockProbe.
    #[test]
    #[ignore = "requires RIVET_LOCK_JAVA and RIVET_LOCK_CLASSPATH for pinned Paper"]
    fn actual_paper_and_rivet_exclude_each_other() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let java = std::env::var_os("RIVET_LOCK_JAVA").expect("Java executable");
        let classpath = std::env::var_os("RIVET_LOCK_CLASSPATH").expect("Paper classpath");
        let root = tempfile::tempdir().unwrap();
        let run = |mode: &str| {
            let mut command = Command::new(&java);
            command
                .args(["-cp"])
                .arg(&classpath)
                .arg("DirectoryLockProbe")
                .arg(mode)
                .arg(root.path());
            command
        };
        let guard = DirectoryLock::create(root.path()).unwrap();
        assert!(DirectoryLock::is_locked(root.path()).unwrap());
        assert!(run("locked").status().unwrap().success());
        // Rejecting aliases must not release the original OS lock.
        assert!(DirectoryLock::create(root.path()).is_err());
        let alias = tempfile::tempdir().unwrap();
        std::fs::hard_link(
            root.path().join("session.lock"),
            alias.path().join("session.lock"),
        )
        .unwrap();
        assert!(DirectoryLock::create(alias.path()).is_err());
        assert!(run("locked").status().unwrap().success());
        drop(guard);
        let mut child = run("hold")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "READY");
        assert!(DirectoryLock::is_locked(root.path()).unwrap());
        let result = DirectoryLock::create(root.path());
        // Release Java even if the exclusion assertion fails.
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
        assert!(!DirectoryLock::is_locked(root.path()).unwrap());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        drop(DirectoryLock::create(root.path()).unwrap());
        assert!(run("acquire").status().unwrap().success());
    }

    #[test]
    fn aliases_cannot_reopen_and_drop_releases_without_deleting_lock_file() {
        let root = tempfile::tempdir().unwrap();
        let world = root.path().join("world");
        assert!(!DirectoryLock::is_locked(&world).unwrap());
        assert!(!world.exists(), "probing must not create directories");
        let guard = DirectoryLock::create(&world).unwrap();
        assert_eq!(
            DirectoryLock::create(&world).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&world, &alias).unwrap();
        assert_eq!(
            DirectoryLock::create(&alias).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let hard = root.path().join("hard");
        std::fs::create_dir(&hard).unwrap();
        std::fs::hard_link(world.join("session.lock"), hard.join("session.lock")).unwrap();
        assert_eq!(
            DirectoryLock::create(&hard).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(guard);
        // Read only after dropping: closing a read descriptor would release a
        // live process-associated lock, too.
        assert_eq!(
            std::fs::read(world.join("session.lock")).unwrap(),
            "☃".as_bytes()
        );
        drop(DirectoryLock::create(&hard).unwrap());
    }

    #[test]
    fn lock_marker_does_not_truncate_existing_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("session.lock"), b"abcdef").unwrap();
        drop(DirectoryLock::create(root.path()).unwrap());
        assert_eq!(
            std::fs::read(root.path().join("session.lock")).unwrap(),
            "☃def".as_bytes()
        );
    }
}
