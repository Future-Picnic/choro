//! Snapshot directory names before removal, and resolve every name relative
//! to an open directory descriptor. No project symlink is followed.
use std::{
    ffi::{CStr, CString},
    fs::File,
    io,
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path},
};

fn at_path(error: io::Error, path: &Path) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

fn component(name: &std::ffi::OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in cleanup path"))
}

fn open_directory(parent: &File, name: &CStr) -> io::Result<File> {
    // SAFETY: parent owns a live descriptor and name is NUL-terminated.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new descriptor owned by this File.
    Ok(unsafe { File::from_raw_fd(fd) })
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: this stream owns the valid pointer returned by fdopendir.
        unsafe {
            libc::closedir(self.0);
        }
    }
}

fn names(directory: &File) -> io::Result<Vec<CString>> {
    // A new open description starts enumeration at zero. dup would share the
    // original offset, which would skip entries if enumeration is repeated.
    let dot = CString::new(".").unwrap();
    let fd = open_directory(directory, &dot)?.into_raw_fd();
    // SAFETY: fd is a newly owned descriptor for a directory.
    let pointer = unsafe { libc::fdopendir(fd) };
    if pointer.is_null() {
        let error = io::Error::last_os_error();
        // SAFETY: failed fdopendir leaves ownership of fd with the caller.
        unsafe {
            libc::close(fd);
        }
        return Err(error);
    }
    let stream = DirectoryStream(pointer);
    let mut names = Vec::new();
    loop {
        // SAFETY: these functions expose the calling thread's errno storage.
        // Reset it to distinguish readdir's end-of-stream from an error.
        #[cfg(target_os = "macos")]
        unsafe {
            *libc::__error() = 0;
        }
        #[cfg(target_os = "linux")]
        unsafe {
            *libc::__errno_location() = 0;
        }
        // SAFETY: stream remains live, and no other thread uses this stream.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(0) {
                break;
            }
            return Err(error);
        }
        // SAFETY: readdir returned a valid dirent with a NUL-terminated name.
        // Copy it before readdir can reuse the buffer.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() != b"." && name.to_bytes() != b".." {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn is_directory(parent: &File, name: &CStr) -> io::Result<bool> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the descriptor and name are valid; stat points to writable storage.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatat initialized the entire stat structure.
    Ok(unsafe { stat.assume_init() }.st_mode & libc::S_IFMT == libc::S_IFDIR)
}

fn unlink(parent: &File, name: &CStr, directory: bool) -> io::Result<()> {
    // SAFETY: this removes one component relative to the owned directory fd.
    let result = unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(error)
    }
}

fn remove_directory(parent: &File, name: &CStr, path: &Path, depth: usize) -> io::Result<()> {
    if depth > 256 {
        return Err(at_path(
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Cleanup tree exceeds 256 nested directories",
            ),
            path,
        ));
    }
    let directory = open_directory(parent, name).map_err(|error| at_path(error, path))?;
    let entries = names(&directory).map_err(|error| at_path(error, path))?;
    for entry in entries {
        let child = path.join(std::ffi::OsStr::from_bytes(entry.to_bytes()));
        let result = match is_directory(&directory, &entry) {
            Ok(true) => remove_directory(&directory, &entry, &child, depth + 1),
            Ok(false) => unlink(&directory, &entry, false).map_err(|error| at_path(error, &child)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(at_path(error, &child)),
        };
        result?;
    }
    unlink(parent, name, true).map_err(|error| at_path(error, path))
}

pub(super) fn remove_snapshot_tree(root: &Path) -> io::Result<()> {
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cleanup path must be absolute",
        ));
    }
    let parent = root.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cleanup cannot remove the filesystem root",
        )
    })?;
    let name =
        component(root.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Cleanup root has no name")
        })?)?;
    let mut directory = File::open("/")?;
    for part in parent.components().skip(1) {
        let Component::Normal(part) = part else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cleanup path contains an unsafe component",
            ));
        };
        directory = open_directory(&directory, &component(part)?)
            .map_err(|error| at_path(error, parent))?;
    }
    remove_directory(&directory, &name, root, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn complete_directory_snapshots_remove_large_cache_trees_without_following_links() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("delegation");
        fs::create_dir(&root).unwrap();
        for index in 0..4096 {
            fs::write(root.join(format!("blob-{index}")), b"copy").unwrap();
        }
        for index in 0..64 {
            let nested = root.join(format!("cache-{index}/build/output"));
            fs::create_dir_all(&nested).unwrap();
            for file in 0..16 {
                fs::write(nested.join(format!("entry-{file}")), b"cache").unwrap();
            }
        }
        fs::write(other.path().join("keep"), b"real project").unwrap();
        std::os::unix::fs::symlink(other.path(), root.join("outside")).unwrap();
        std::os::unix::fs::symlink(other.path().join("keep"), root.join("outside-file")).unwrap();
        std::os::unix::fs::symlink("missing", root.join("broken-link")).unwrap();
        remove_snapshot_tree(&root).unwrap();
        assert!(!root.exists());
        assert_eq!(
            fs::read(other.path().join("keep")).unwrap(),
            b"real project"
        );
    }

    #[test]
    fn linked_parent_and_linked_root_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        fs::create_dir(other.path().join("delegation")).unwrap();
        fs::write(other.path().join("delegation/keep"), b"real project").unwrap();
        std::os::unix::fs::symlink(other.path(), base.join("linked-parent")).unwrap();
        assert!(remove_snapshot_tree(&base.join("linked-parent/delegation")).is_err());
        std::os::unix::fs::symlink(other.path().join("delegation"), base.join("linked-root"))
            .unwrap();
        assert!(remove_snapshot_tree(&base.join("linked-root")).is_err());
        assert_eq!(
            fs::read(other.path().join("delegation/keep")).unwrap(),
            b"real project"
        );
    }
}
