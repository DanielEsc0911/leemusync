//! Is an emulator running? (I3; emulator-profiles §Linux paths: match the executable name, not `comm`.)

use std::sync::atomic::{AtomicBool, Ordering};

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
                .any(|n| exe.as_deref() == Some(n.as_str()) || comm == *n)
        })
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
    fn sysinfo_does_not_see_a_made_up_process() {
        assert!(!SysinfoProbe.running(&["leemusync-no-such-process-7f3a".into()]));
    }
}
