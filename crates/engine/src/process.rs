//! Is an emulator running? (I3; emulator-profiles §Linux paths: match the executable name, not `comm`.)
//! Starting one and waiting for it to exit (sync-model §Launcher mode): argv only, never a shell.

use std::collections::BTreeSet;
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

/// `std::process::Command` with argv[0] as the program and no shell (B2). On Unix the
/// emulator gets its own process group, so Ctrl-C in the terminal never reaches it.
pub struct OsLauncher;

impl Launcher for OsLauncher {
    fn spawn(&self, argv: &[OsString]) -> std::io::Result<Box<dyn Child>> {
        let (exe, rest) = argv
            .split_first()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty argv"))?;
        let mut cmd = std::process::Command::new(exe);
        cmd.args(rest);
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let child = cmd.spawn()?;
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

    /// The emulator ids with a running process, from one refresh of the process list for all
    /// of them (`names_by_emulator`: emulator id → its profile's process names).
    fn running_set(&self, names_by_emulator: &[(String, Vec<String>)]) -> BTreeSet<String>;
}

pub struct SysinfoProbe;

impl SysinfoProbe {
    /// One walk of the process list: each process's executable file name and `comm`.
    fn snapshot() -> Vec<(Option<String>, String)> {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        sys.processes()
            .values()
            .map(|p| {
                let exe = p
                    .exe()
                    .and_then(|e| e.file_name())
                    .map(|n| n.to_string_lossy().into_owned());
                (exe, p.name().to_string_lossy().into_owned())
            })
            .collect()
    }
}

fn any_matches(procs: &[(Option<String>, String)], names: &[String]) -> bool {
    procs.iter().any(|(exe, comm)| {
        names
            .iter()
            .any(|n| exe.as_deref().is_some_and(|e| same_name(e, n)) || same_name(comm, n))
    })
}

impl ProcessProbe for SysinfoProbe {
    fn running(&self, names: &[String]) -> bool {
        any_matches(&Self::snapshot(), names)
    }

    fn running_set(&self, names_by_emulator: &[(String, Vec<String>)]) -> BTreeSet<String> {
        let procs = Self::snapshot();
        names_by_emulator
            .iter()
            .filter(|(_, names)| any_matches(&procs, names))
            .map(|(id, _)| id.clone())
            .collect()
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

    /// Every emulator asked about, when set.
    fn running_set(&self, names_by_emulator: &[(String, Vec<String>)]) -> BTreeSet<String> {
        if self.0.load(Ordering::SeqCst) {
            names_by_emulator.iter().map(|(id, _)| id.clone()).collect()
        } else {
            BTreeSet::new()
        }
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
        assert!(!SysinfoProbe.running(&["lemusync-no-such-process-7f3a".into()]));
    }

    #[test]
    fn running_set_names_only_the_running_emulators() {
        let me = std::env::current_exe().unwrap();
        let me = me.file_name().unwrap().to_string_lossy().into_owned();
        let set = SysinfoProbe.running_set(&[
            ("me".into(), vec![me]),
            ("ghost".into(), vec!["lemusync-no-such-process-7f3a".into()]),
        ]);
        assert_eq!(set, BTreeSet::from(["me".to_owned()]));
        let p = FakeProbe::default();
        let ask = [("cemu".to_owned(), vec![])];
        assert!(p.running_set(&ask).is_empty());
        p.set(true);
        assert_eq!(p.running_set(&ask), BTreeSet::from(["cemu".to_owned()]));
    }
}
