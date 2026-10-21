//! Is an emulator running? (I3; emulator-profiles §Linux paths: match the executable name, not `comm`.)
//! Starting one and waiting for it to exit (sync-model §Launcher mode): argv only, never a shell.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};

/// Starts the emulator. The CLI uses `OsLauncher`; tests use a fake.
pub trait Launcher: Send + Sync {
    /// # Errors
    /// The OS error when the process can't start.
    fn spawn(&self, argv: &[OsString]) -> std::io::Result<Box<dyn Child>>;
}

/// A started emulator. `wait` blocks, so the engine calls it on a blocking thread.
pub trait Child: Send {
    /// Blocks until the process exits; its exit code, -1 when killed by a signal.
    ///
    /// # Errors
    /// The OS error from waiting.
    fn wait(&mut self) -> std::io::Result<i32>;
}

/// `std::process::Command` with argv[0] as the program and no shell (B2).
pub struct OsLauncher;

impl Launcher for OsLauncher {
    fn spawn(&self, argv: &[OsString]) -> std::io::Result<Box<dyn Child>> {
        let (exe, rest) = argv
            .split_first()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty argv"))?;
        let child = std::process::Command::new(exe).args(rest).spawn()?;
        Ok(Box::new(OsChild(child)))
    }
}

struct OsChild(std::process::Child);

impl Child for OsChild {
    fn wait(&mut self) -> std::io::Result<i32> {
        Ok(self.0.wait()?.code().unwrap_or(-1))
    }
}

pub trait ProcessProbe: Send + Sync {
    fn running(&self, names: &[String]) -> bool;
}

pub struct SysinfoProbe;

impl ProcessProbe for SysinfoProbe {
    fn running(&self, names: &[String]) -> bool {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        sys.processes().values().any(|p| {
            let exe = p
                .exe()
                .and_then(|e| e.file_name())
                .map(|n| n.to_string_lossy().into_owned());
            let comm = p.name().to_string_lossy().into_owned();
            names
                .iter()
                .any(|n| exe.as_deref().is_some_and(|e| same_name(e, n)) || same_name(&comm, n))
        })
    }
}

/// Executable names are case-insensitive on Windows and macOS file systems.
fn same_name(found: &str, wanted: &str) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        found.eq_ignore_ascii_case(wanted)
    } else {
        found == wanted
    }
}

/// For tests: reports "running" when set.
#[derive(Default)]
pub struct FakeProbe(AtomicBool);

impl FakeProbe {
    pub fn set(&self, running: bool) {
        self.0.store(running, Ordering::SeqCst);
    }
}

impl ProcessProbe for FakeProbe {
    fn running(&self, _names: &[String]) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_probe_reports_what_it_is_told() {
        let p = FakeProbe::default();
        assert!(!p.running(&["Cemu".into()]));
        p.set(true);
        assert!(p.running(&["Cemu".into()]));
    }

    #[test]
    fn names_ignore_case_only_where_the_file_system_does() {
        assert!(same_name("Cemu", "Cemu"));
        assert_eq!(
            same_name("cemu.exe", "Cemu.exe"),
            cfg!(any(windows, target_os = "macos"))
        );
    }

    #[test]
    fn sysinfo_does_not_see_a_made_up_process() {
        assert!(!SysinfoProbe.running(&["leemusync-no-such-process-7f3a".into()]));
    }
}
