//! Adapter presenting an already-open PTY master file descriptor as the
//! `portable_pty::MasterPty` abstraction the pane layer is written against
//! (mux daemon hot-upgrade, task0002).
//!
//! `portable_pty` offers no public way to build its master abstraction from
//! an existing descriptor — it only constructs one via its own `openpty`.
//! This module fills that gap for descriptors that survive a process
//! replacement (`execve`) rather than being freshly opened, so a restored
//! pane is indistinguishable from a freshly spawned one to its callers.
//!
//! Unix only: process replacement and the descriptor semantics this adapter
//! depends on (raw fds, `dup`, `fcntl`) have no Windows equivalent.

use std::io::{self, Read, Write};
use std::os::unix::io::{FromRawFd, RawFd};

use anyhow::{bail, Result};
use portable_pty::{MasterPty, PtySize};

/// Presents an inherited raw PTY master descriptor as a [`MasterPty`].
///
/// Owns `fd`: dropping the adapter closes it. The reader and writer handles
/// it produces are backed by independent duplicates of `fd`, so dropping
/// one never disturbs the owned descriptor or any other handle.
#[derive(Debug)]
pub struct InheritedMasterPty {
    fd: RawFd,
}

impl InheritedMasterPty {
    /// Takes ownership of `fd`, validating that it is still open and
    /// refers to a terminal before accepting it.
    ///
    /// On failure, `fd` is left untouched — no adapter is produced, so
    /// ownership was never taken, and the caller remains responsible for
    /// the descriptor.
    pub fn new(fd: RawFd) -> Result<Self> {
        // SAFETY: `isatty` only inspects `fd`'s kernel-side state; it does
        // not dereference any pointer we control.
        let is_tty = unsafe { libc::isatty(fd) } == 1;
        if !is_tty {
            bail!(
                "fd {fd} cannot back an inherited PTY master (closed, or \
                 not a terminal): {}",
                io::Error::last_os_error()
            );
        }
        Ok(Self { fd })
    }

    /// The owned raw descriptor.
    pub fn raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Drop for InheritedMasterPty {
    fn drop(&mut self) {
        // SAFETY: `fd` is owned exclusively by this adapter once
        // construction succeeded — nothing else holds it.
        unsafe {
            libc::close(self.fd);
        }
    }
}

/// Duplicates `fd`, atomically marking the new descriptor close-on-exec
/// (`F_DUPFD_CLOEXEC`) so it never leaks into a pane's child process.
/// Backs both [`InheritedMasterPty::try_clone_reader`] and
/// [`InheritedMasterPty::take_writer`].
fn dup_cloexec(fd: RawFd) -> Result<RawFd> {
    // SAFETY: `fd` is a descriptor the caller owns for the duration of this
    // call; `F_DUPFD_CLOEXEC` only reads it and returns a new descriptor.
    let new_fd = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if new_fd < 0 {
        bail!(
            "fcntl(F_DUPFD_CLOEXEC) failed duplicating fd {fd}: {}",
            io::Error::last_os_error()
        );
    }
    Ok(new_fd)
}

/// A duplicated PTY descriptor wrapped for `Read`/`Write`, closed on drop.
///
/// Mirrors `portable_pty`'s own unix master-reader behavior: `EIO` from a
/// hung-up slave is reported as a plain EOF (`Ok(0)`) rather than an error,
/// so a restored pane's reader loop sees the same termination signal a
/// freshly spawned pane's would.
struct DuplicatedPtyHandle(std::fs::File);

impl Read for DuplicatedPtyHandle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.0.read(buf) {
            Err(ref e) if e.raw_os_error() == Some(libc::EIO) => Ok(0),
            other => other,
        }
    }
}

impl Write for DuplicatedPtyHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl MasterPty for InheritedMasterPty {
    fn resize(&self, size: PtySize) -> Result<()> {
        let ws = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.pixel_width,
            ws_ypixel: size.pixel_height,
        };
        // SAFETY: `ws` is a fully-initialized `winsize` the ioctl only
        // reads; `self.fd` is owned and open for the adapter's lifetime.
        let rc = unsafe { libc::ioctl(self.fd, libc::TIOCSWINSZ as _, &ws as *const _) };
        if rc != 0 {
            bail!(
                "ioctl(TIOCSWINSZ) failed on fd {}: {}",
                self.fd,
                io::Error::last_os_error()
            );
        }
        Ok(())
    }

    fn get_size(&self) -> Result<PtySize> {
        // SAFETY: `ws` is zero-initialized POD the ioctl fills in; `self.fd`
        // is owned and open for the adapter's lifetime.
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::ioctl(self.fd, libc::TIOCGWINSZ as _, &mut ws as *mut _) };
        if rc != 0 {
            bail!(
                "ioctl(TIOCGWINSZ) failed on fd {}: {}",
                self.fd,
                io::Error::last_os_error()
            );
        }
        Ok(PtySize {
            rows: ws.ws_row,
            cols: ws.ws_col,
            pixel_width: ws.ws_xpixel,
            pixel_height: ws.ws_ypixel,
        })
    }

    fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>> {
        let dup_fd = dup_cloexec(self.fd)?;
        // SAFETY: `dup_fd` was just returned by `F_DUPFD_CLOEXEC`; nothing
        // else references it, so this `File` exclusively owns it.
        let file = unsafe { std::fs::File::from_raw_fd(dup_fd) };
        Ok(Box::new(DuplicatedPtyHandle(file)))
    }

    fn take_writer(&self) -> Result<Box<dyn Write + Send>> {
        let dup_fd = dup_cloexec(self.fd)?;
        // SAFETY: see `try_clone_reader` above.
        let file = unsafe { std::fs::File::from_raw_fd(dup_fd) };
        Ok(Box::new(DuplicatedPtyHandle(file)))
    }

    fn as_raw_fd(&self) -> Option<RawFd> {
        Some(self.fd)
    }

    fn process_group_leader(&self) -> Option<libc::pid_t> {
        // SAFETY: `self.fd` is owned and open for the adapter's lifetime.
        match unsafe { libc::tcgetpgrp(self.fd) } {
            pid if pid > 0 => Some(pid),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::io::AsRawFd as _;

    /// Opens a real PTY pair (as the existing pane tests do) and returns it
    /// alongside a SECOND, independently-owned duplicate of the master's
    /// raw descriptor — standing in for "a descriptor obtained by opening a
    /// PTY pair" that has been handed to this process across a process
    /// replacement. Duplicating avoids a double-close race between
    /// `pair.master`'s own `Drop` and the adapter under test, since each
    /// then owns a distinct descriptor number referring to the same open
    /// master.
    fn open_master_dup() -> (portable_pty::PtyPair, RawFd) {
        let pty_system = portable_pty::native_pty_system();
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system
            .openpty(size)
            .expect("openpty must succeed in the test environment");
        let master_fd = pair
            .master
            .as_raw_fd()
            .expect("unix PTY master exposes a raw fd");
        // SAFETY: `master_fd` is a live descriptor owned by `pair.master`
        // for the duration of this call; `dup` only reads it.
        let dup_fd = unsafe { libc::dup(master_fd) };
        assert!(dup_fd >= 0, "dup(2) failed: {}", io::Error::last_os_error());
        (pair, dup_fd)
    }

    /// Resolves the `/dev/pts/<N>` path of the slave paired with
    /// `master_fd`, so tests can open an independent handle to the slave
    /// side without spawning a child process into it.
    fn slave_device_path(master_fd: RawFd) -> std::path::PathBuf {
        let mut ptn: libc::c_uint = 0;
        // SAFETY: `ptn` is a valid `c_uint` the ioctl fills in; `master_fd`
        // is a live PTY master for the duration of this call.
        let rc = unsafe { libc::ioctl(master_fd, libc::TIOCGPTN, &mut ptn as *mut _) };
        assert_eq!(
            rc,
            0,
            "ioctl(TIOCGPTN) failed: {}",
            io::Error::last_os_error()
        );
        std::path::PathBuf::from(format!("/dev/pts/{ptn}"))
    }

    /// Switches the pty pair to raw mode (no canonical line buffering, no
    /// echo), so read/write assertions in the tests below see exact bytes
    /// without waiting on a line terminator or seeing echoed input.
    fn set_raw_mode(fd: RawFd) {
        // SAFETY: `term` is fully initialized by `tcgetattr` before
        // `cfmakeraw`/`tcsetattr` read or write it; `fd` is a live tty for
        // the duration of this call.
        unsafe {
            let mut term: libc::termios = std::mem::zeroed();
            assert_eq!(
                libc::tcgetattr(fd, &mut term),
                0,
                "tcgetattr failed: {}",
                io::Error::last_os_error()
            );
            libc::cfmakeraw(&mut term);
            assert_eq!(
                libc::tcsetattr(fd, libc::TCSANOW, &term),
                0,
                "tcsetattr failed: {}",
                io::Error::last_os_error()
            );
        }
    }

    /// The device number `fstat` reports for `fd`, or `None` if `fd` is not
    /// a currently-open descriptor.
    ///
    /// Used (rather than a plain "is this fd number valid" check) so tests
    /// that assert a descriptor was closed stay correct even when `cargo
    /// test`'s parallel threads — sharing one process-wide fd table — hand
    /// that freed NUMBER to an unrelated descriptor from a concurrently
    /// running test before the assertion runs: comparing the device
    /// identity, not just fd validity, tells the two cases apart.
    fn stat_rdev(fd: RawFd) -> Option<libc::dev_t> {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `st` is a zero-initialized POD `fstat` fills in; `fd` is
        // a caller-supplied descriptor number, valid or not (`fstat` itself
        // reports the invalid case via its return code).
        let rc = unsafe { libc::fstat(fd, &mut st) };
        if rc == 0 { Some(st.st_rdev) } else { None }
    }

    fn has_cloexec(fd: RawFd) -> bool {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(
            flags != -1,
            "fcntl(F_GETFD) failed: {}",
            io::Error::last_os_error()
        );
        flags & libc::FD_CLOEXEC != 0
    }

    /// AC-1: given a descriptor obtained by opening a PTY pair, the adapter
    /// can be constructed and reports that same descriptor number.
    #[test]
    fn construction_reports_the_same_descriptor_number() {
        let (_pair, dup_fd) = open_master_dup();

        let adapter =
            InheritedMasterPty::new(dup_fd).expect("a live PTY master descriptor must construct");

        assert_eq!(adapter.raw_fd(), dup_fd);
        assert_eq!(MasterPty::as_raw_fd(&adapter), Some(dup_fd));
    }

    /// AC-2: bytes written to the adapter's writer handle are readable
    /// from the corresponding PTY slave.
    #[test]
    fn writer_handle_bytes_are_readable_from_the_pty_slave() {
        let (_pair, dup_fd) = open_master_dup();
        set_raw_mode(dup_fd);
        let slave_path = slave_device_path(dup_fd);
        let mut slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&slave_path)
            .expect("slave device must be openable");

        let adapter = InheritedMasterPty::new(dup_fd).unwrap();
        let mut writer = adapter.take_writer().expect("take_writer must succeed");
        writer.write_all(b"ac2-writer-to-slave").unwrap();
        writer.flush().unwrap();

        let mut buf = [0u8; 64];
        let n = slave.read(&mut buf).expect("slave read must succeed");
        assert_eq!(&buf[..n], b"ac2-writer-to-slave");
    }

    /// AC-3: bytes written to the PTY slave are readable through the
    /// adapter's reader handle.
    #[test]
    fn slave_bytes_are_readable_through_the_adapter_reader_handle() {
        let (_pair, dup_fd) = open_master_dup();
        set_raw_mode(dup_fd);
        let slave_path = slave_device_path(dup_fd);
        let mut slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&slave_path)
            .expect("slave device must be openable");

        let adapter = InheritedMasterPty::new(dup_fd).unwrap();
        let mut reader = adapter
            .try_clone_reader()
            .expect("try_clone_reader must succeed");

        slave.write_all(b"ac3-slave-to-reader").unwrap();
        slave.flush().unwrap();

        let mut buf = [0u8; 64];
        let n = reader
            .read(&mut buf)
            .expect("adapter reader read must succeed");
        assert_eq!(&buf[..n], b"ac3-slave-to-reader");
    }

    /// AC-4: two reader handles obtained from the same adapter are
    /// independent — dropping one leaves the other usable.
    #[test]
    fn dropping_one_reader_handle_leaves_another_usable() {
        let (_pair, dup_fd) = open_master_dup();
        set_raw_mode(dup_fd);
        let slave_path = slave_device_path(dup_fd);
        let mut slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&slave_path)
            .expect("slave device must be openable");

        let adapter = InheritedMasterPty::new(dup_fd).unwrap();
        let reader_a = adapter
            .try_clone_reader()
            .expect("first try_clone_reader must succeed");
        let mut reader_b = adapter
            .try_clone_reader()
            .expect("second try_clone_reader must succeed");

        drop(reader_a);

        slave.write_all(b"ac4-still-usable").unwrap();
        slave.flush().unwrap();

        let mut buf = [0u8; 64];
        let n = reader_b
            .read(&mut buf)
            .expect("surviving reader handle must still read after its sibling dropped");
        assert_eq!(&buf[..n], b"ac4-still-usable");
    }

    /// AC-5: setting the window size through the adapter is observable
    /// through a subsequent size query on the same adapter.
    #[test]
    fn resized_window_is_observable_on_a_subsequent_query() {
        let (_pair, dup_fd) = open_master_dup();
        let adapter = InheritedMasterPty::new(dup_fd).unwrap();

        let target = PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        };
        adapter
            .resize(target)
            .expect("resize must succeed on a live master");

        let observed = adapter
            .get_size()
            .expect("get_size must succeed on a live master");
        assert_eq!(observed, target);
    }

    /// AC-6: construction over an ordinary (non-terminal) file descriptor
    /// fails instead of yielding an adapter.
    #[test]
    fn construction_fails_over_an_ordinary_file_descriptor() {
        let file = tempfile::tempfile().expect("must create a temp file");

        let result = InheritedMasterPty::new(file.as_raw_fd());

        assert!(
            result.is_err(),
            "an ordinary file descriptor must not construct a MasterPty adapter"
        );
        // `file` still owns its fd (construction failed, so no ownership
        // was taken) — dropping `file` here is what actually closes it.
    }

    /// AC-6: construction over an already-closed descriptor fails instead
    /// of yielding an adapter.
    #[test]
    fn construction_fails_over_a_closed_descriptor() {
        let (_pair, dup_fd) = open_master_dup();
        // SAFETY: `dup_fd` is a descriptor this test exclusively owns (a
        // fresh `dup` above); no other code holds it.
        unsafe {
            libc::close(dup_fd);
        }

        let result = InheritedMasterPty::new(dup_fd);

        assert!(
            result.is_err(),
            "a closed descriptor must not construct a MasterPty adapter"
        );
    }

    /// AC-7: dropping the adapter closes the owned descriptor.
    #[test]
    fn dropping_the_adapter_closes_the_owned_descriptor() {
        let (_pair, dup_fd) = open_master_dup();
        let original_rdev =
            stat_rdev(dup_fd).expect("sanity: fd must be open right after construction");
        let adapter = InheritedMasterPty::new(dup_fd).unwrap();

        drop(adapter);

        // `_pair.master` (still in scope) keeps this test's own pty device
        // minor number open for the whole test, so if `dup_fd`'s NUMBER
        // gets reassigned to an unrelated descriptor by a concurrently
        // running test before this check runs, that descriptor can only
        // ever refer to a DIFFERENT device — never ours — which still
        // proves our own descriptor was closed.
        match stat_rdev(dup_fd) {
            None => {}
            Some(rdev) => assert_ne!(
                rdev, original_rdev,
                "the owned descriptor must be closed once the adapter drops"
            ),
        }
    }

    /// AC-8: the descriptor-duplication mechanism backing both
    /// `try_clone_reader` and `take_writer` (`dup_cloexec`) marks the new
    /// descriptor close-on-exec.
    ///
    /// Verified directly on the raw fd `dup_cloexec` returns rather than
    /// through `try_clone_reader`/`take_writer`'s own return values:
    /// `Box<dyn Read + Send>` / `Box<dyn Write + Send>` erase the concrete
    /// fd-backed type, so the flag cannot be observed through those boxed
    /// handles. `dup_cloexec` is the sole mechanism both methods use to
    /// produce their duplicate, so this exercises the exact code path.
    #[test]
    fn duplicated_descriptors_have_close_on_exec_set() {
        let (_pair, dup_fd) = open_master_dup();

        let new_fd = dup_cloexec(dup_fd).expect("duplicating a live descriptor must succeed");
        assert!(
            has_cloexec(new_fd),
            "a descriptor produced for a reader/writer handle must be close-on-exec"
        );
        // SAFETY: `new_fd` is a descriptor this test exclusively owns (just
        // duplicated above, never handed to anything else).
        unsafe {
            libc::close(new_fd);
        }

        // Sanity: the mechanism verified above is exactly what backs the
        // public reader/writer handles.
        let adapter = InheritedMasterPty::new(dup_fd).unwrap();
        assert!(adapter.try_clone_reader().is_ok());
        assert!(adapter.take_writer().is_ok());
    }
}
