//! Human (Fluent) or `--json` output. JSON goes to stdout; human results to stdout, problems to stderr.

use fluent_bundle::FluentValue;
use leemusync_api::{
    API_VERSION, DoctorReport, ErrorBody, ErrorReport, HistoryReport, ScanReport, StatusReport,
    SyncReport, Warning,
};
use leemusync_engine::EngineError;
use serde::Serialize;

use crate::i18n::{I18n, s};

pub fn json<T: Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("{e}"),
    }
}

fn snake<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The data part of an error, never translated (paths, ids, provider text).
fn detail(e: &EngineError) -> String {
    match e {
        EngineError::Usage(d)
        | EngineError::Config(d)
        | EngineError::Io(d)
        | EngineError::Locked(d)
        | EngineError::EmulatorRunning(d)
        | EngineError::NotGranted(d)
        | EngineError::Secrets(d)
        | EngineError::Data(d)
        | EngineError::Busy(d) => d.clone(),
        EngineError::Store(s) => s.message().to_owned(),
        EngineError::Crypto(c) => c.to_string(),
        EngineError::NotSetUp(d) | EngineError::Injected(d) => (*d).to_owned(),
        EngineError::Unlock | EngineError::NeedsConfirmation => String::new(),
    }
}

pub fn error(t: &I18n, as_json: bool, e: &EngineError) {
    if as_json {
        json(&ErrorReport {
            api_version: API_VERSION,
            error: ErrorBody {
                code: e.code().to_owned(),
                message: e.to_string(),
            },
        });
    } else {
        eprintln!("{}", error_text(t, e));
    }
}

/// Engine payloads are keys (`key` or `key:data`, Task 11/13): each key has its own message
/// `error-<code>-<key>`. Anything else is data and goes through the generic `error-<code>`.
fn error_text(t: &I18n, e: &EngineError) -> String {
    let detail = detail(e);
    let (key, data) = detail.split_once(':').unwrap_or((detail.as_str(), ""));
    t.get(&format!("error-{}-{key}", e.code()), &[("detail", s(data))])
        .unwrap_or_else(|| {
            t.t(
                &format!("error-{}", e.code()),
                &[("detail", s(detail.clone()))],
            )
        })
}

pub fn line(t: &I18n, key: &str, args: &[(&str, FluentValue<'static>)]) {
    println!("{}", t.t(key, args));
}

pub fn warnings(t: &I18n, warnings: &[Warning]) {
    for w in warnings {
        eprintln!(
            "{}",
            t.t(
                &format!("warning-{}", snake(&w.code)),
                &[("detail", s(w.detail.clone()))]
            )
        );
    }
}

pub fn sync(t: &I18n, r: &SyncReport) {
    for st in &r.streams {
        let action = t.t(&format!("action-{}", snake(&st.action)), &[]);
        line(
            t,
            "sync-line",
            &[
                ("player", s(st.player.clone())),
                ("game", s(st.game.clone())),
                ("action", s(action)),
            ],
        );
    }
    warnings(t, &r.warnings);
    if r.dry_run {
        line(t, "dry-run", &[]);
    }
}

pub fn scan(t: &I18n, r: &ScanReport) {
    for i in &r.installs {
        line(
            t,
            "scan-install",
            &[
                ("emulator", s(i.emulator.clone())),
                ("base", s(i.base.clone())),
            ],
        );
        for p in &i.needs_grant {
            line(t, "scan-needs-grant", &[("path", s(p.clone()))]);
        }
        let unknown = t.t("scan-unknown", &[]);
        for a in &i.accounts {
            line(
                t,
                "scan-account",
                &[
                    ("id", s(a.id.clone())),
                    ("name", s(a.name.clone().unwrap_or_else(|| unknown.clone()))),
                    (
                        "player",
                        s(a.player.clone().unwrap_or_else(|| unknown.clone())),
                    ),
                ],
            );
        }
        for sv in &i.saves {
            let key = if sv.orphaned {
                "scan-orphan"
            } else {
                "scan-save"
            };
            line(
                t,
                key,
                &[
                    ("game", s(sv.game.clone())),
                    ("account", s(sv.account.clone())),
                    ("files", FluentValue::from(sv.files)),
                ],
            );
        }
    }
    warnings(t, &r.warnings);
}

pub fn status(t: &I18n, r: &StatusReport) {
    line(
        t,
        "status-device",
        &[("device", s(format!("{} ({})", r.device, r.device_id)))],
    );
    match (&r.repo, &r.backend) {
        (Some(repo), backend) => line(
            t,
            "status-repo",
            &[
                ("repo", s(repo.clone())),
                ("backend", s(backend.clone().unwrap_or_default())),
            ],
        ),
        (None, _) => line(t, "status-no-repo", &[]),
    }
    for g in &r.grants {
        line(t, "grant-done", &[("path", s(g.clone()))]);
    }
    let none = t.t("scan-unknown", &[]);
    for st in &r.streams {
        line(
            t,
            "status-stream",
            &[
                ("player", s(st.player.clone())),
                ("emulator", s(st.emulator.clone())),
                ("game", s(st.game.clone())),
                (
                    "account",
                    s(st.account.clone().unwrap_or_else(|| none.clone())),
                ),
            ],
        );
    }
}

pub fn history(t: &I18n, r: &HistoryReport) {
    let head = t.t("history-head", &[]);
    for v in &r.versions {
        let marker = if v.head { head.clone() } else { String::new() };
        line(
            t,
            "history-version",
            &[
                ("id", s(v.id.clone())),
                ("files", FluentValue::from(v.files)),
                ("head", s(marker)),
            ],
        );
    }
    for b in &r.backups {
        line(t, "history-backup", &[("name", s(b.clone()))]);
    }
}

pub fn doctor(t: &I18n, r: &DoctorReport) {
    for c in &r.checks {
        let status = t.t(if c.ok { "doctor-ok" } else { "doctor-fail" }, &[]);
        line(
            t,
            "doctor-check",
            &[
                ("status", s(status)),
                ("name", s(c.name.clone())),
                ("detail", s(c.detail.clone())),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_keys_get_their_own_message_and_data_stays_data() {
        for lang in ["en", "es"] {
            let t = I18n::new(lang);
            let short = error_text(&t, &EngineError::Usage("passphrase_too_short".into()));
            assert!(
                !short.contains("passphrase_too_short") && short.contains('8'),
                "{short}"
            );
            let big = error_text(&t, &EngineError::Usage("file_too_large:a/b.sav".into()));
            assert!(
                big.contains("a/b.sav") && !big.contains("file_too_large"),
                "{big}"
            );
            let repo = error_text(&t, &EngineError::NotSetUp("repo"));
            assert!(repo.contains("leemusync init"), "{repo}");
            let other = error_text(&t, &EngineError::Io("disk full".into()));
            assert!(other.contains("disk full"), "{other}");
        }
    }
}
