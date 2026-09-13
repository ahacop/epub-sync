//! Ejects the Kobo volume: unmount, flush, and tell the device the USB
//! session is over. A plain unmount is not enough, because the Kobo keeps
//! showing "connected" until it gets the SCSI eject.
//!
//! On Linux this goes through udisks2 over D-Bus: `Filesystem.Unmount`
//! then `Drive.Eject`, which runs the eject as root with polkit deciding
//! whether the logged-in user may. On macOS `diskutil eject` does both.

use std::path::Path;

use anyhow::{Context, Result, bail};

/// What the eject did.
#[derive(Debug, PartialEq, Eq)]
pub enum Ejected {
    /// The volume was unmounted and the device told to disconnect.
    Yes,
    /// The path is a plain folder, not a mounted volume. Nothing to eject.
    NotAVolume,
}

pub fn eject(root: &Path) -> Result<Ejected> {
    if !is_mount_point(root)? {
        return Ok(Ejected::NotAVolume);
    }
    platform::eject(root)?;
    Ok(Ejected::Yes)
}

/// True when `path` is the root of a mounted filesystem: its device
/// differs from its parent's.
fn is_mount_point(path: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let path = path
        .canonicalize()
        .with_context(|| format!("resolve {}", path.display()))?;
    let Some(parent) = path.parent() else {
        return Ok(true);
    };
    Ok(std::fs::metadata(&path)?.dev() != std::fs::metadata(parent)?.dev())
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::collections::HashMap;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedObjectPath, Value};

    const UDISKS: &str = "org.freedesktop.UDisks2";

    pub fn eject(root: &Path) -> Result<()> {
        let device = block_device(root)?;
        let name = device
            .strip_prefix("/dev/")
            .with_context(|| format!("{device} is not under /dev"))?;
        let block_path = format!("/org/freedesktop/UDisks2/block_devices/{name}");
        let conn = Connection::system().context("connect to the system D-Bus")?;
        let no_options: HashMap<&str, Value> = HashMap::new();

        let filesystem = Proxy::new(
            &conn,
            UDISKS,
            block_path.as_str(),
            "org.freedesktop.UDisks2.Filesystem",
        )?;
        filesystem
            .call_method("Unmount", &(&no_options,))
            .with_context(|| format!("unmount {device} through udisks2"))?;

        let block = Proxy::new(
            &conn,
            UDISKS,
            block_path.as_str(),
            "org.freedesktop.UDisks2.Block",
        )?;
        let drive: OwnedObjectPath = block
            .get_property("Drive")
            .context("find the drive of the volume")?;
        let drive = Proxy::new(&conn, UDISKS, drive, "org.freedesktop.UDisks2.Drive")?;
        drive
            .call_method("Eject", &(&no_options,))
            .with_context(|| format!("eject {device} through udisks2"))?;
        Ok(())
    }

    /// The device mounted at `root`, from /proc/self/mountinfo.
    fn block_device(root: &Path) -> Result<String> {
        let root = root.canonicalize()?;
        let info =
            std::fs::read_to_string("/proc/self/mountinfo").context("read /proc/self/mountinfo")?;
        for line in info.lines() {
            let fields: Vec<&str> = line.split(' ').collect();
            let Some(sep) = fields.iter().position(|f| *f == "-") else {
                continue;
            };
            if fields.len() < sep + 3 {
                continue;
            }
            if Path::new(&unescape(fields[4])) == root {
                return Ok(unescape(fields[sep + 2]));
            }
        }
        bail!("{} is not in the mount table", root.display())
    }

    /// Undoes the octal escapes mountinfo uses for space, tab, newline,
    /// and backslash.
    fn unescape(s: &str) -> String {
        s.replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\")
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    pub fn eject(root: &Path) -> Result<()> {
        let status = std::process::Command::new("diskutil")
            .arg("eject")
            .arg(root)
            .status()
            .context("run diskutil")?;
        if !status.success() {
            bail!("diskutil eject {} exited with {status}", root.display());
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;

    pub fn eject(root: &Path) -> Result<()> {
        bail!(
            "eject is not supported on this system; eject {} yourself",
            root.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_folder_is_not_a_volume() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(eject(dir.path()).unwrap(), Ejected::NotAVolume);
    }

    #[test]
    fn the_filesystem_root_is_a_mount_point() {
        assert!(is_mount_point(Path::new("/")).unwrap());
    }
}
