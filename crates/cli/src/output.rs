//! Human (Fluent) or `--json` output. JSON goes to stdout; human results to stdout, problems to stderr.

use fluent_bundle::FluentValue;
use std::time::Duration;

use lemusync_api::{
    API_VERSION, ConflictInfo, DoctorReport, ErrorBody, ErrorReport, HistoryReport, LeaseInfo,
    PlayReport, ScanReport, StatusReport, StreamAction, SyncReport, Warning,
};
use lemusync_engine::EngineError;
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
        | EngineError::Busy(d)
        | EngineError::NeedsDecision(d)
        | EngineError::LeaseHeld(d) => d.clone(),
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

/// ANSI colors on stdout: only for a terminal, and never with `NO_COLOR` set (no-color.org).
pub fn color_on() -> bool {
    use std::io::IsTerminal;
    color_allowed(
        std::env::var_os("NO_COLOR"),
        std::io::stdout().is_terminal(),
    )
}

fn color_allowed(no_color: Option<std::ffi::OsString>, tty: bool) -> bool {
    tty && no_color.is_none_or(|v| v.is_empty())
}

/// Green when the stream is settled, yellow when it waits on the user, red for a conflict.
fn paint(action: StreamAction, text: String, color: bool) -> String {
    let code = match action {
        StreamAction::Nothing
        | StreamAction::Adopt
        | StreamAction::FastForward
        | StreamAction::Upload
        | StreamAction::Resolved => "32",
        StreamAction::Skipped | StreamAction::NeedsAccount | StreamAction::LocalMissing => "33",
        StreamAction::UploadThenConflict | StreamAction::Conflict | StreamAction::RemoteBehind => {
            "31"
        }
    };
    if color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text
    }
}

fn pad(text: &str, width: usize) -> String {
    format!(
        "{text}{}",
        " ".repeat(width.saturating_sub(text.chars().count()))
    )
}

/// One aligned line per stream, then warnings. `elapsed` is shown when given.
pub fn sync(t: &I18n, r: &SyncReport, color: bool, elapsed: Option<Duration>) {
    let pw = r
        .streams
        .iter()
        .map(|st| st.player.chars().count())
        .max()
        .unwrap_or(0);
    let gw = r
        .streams
        .iter()
        .map(|st| st.game.chars().count())
        .max()
        .unwrap_or(0);
    for st in &r.streams {
        let action = paint(
            st.action,
            t.t(&format!("action-{}", snake(&st.action)), &[]),
            color,
        );
        let mut args = vec![
            ("player", s(pad(&st.player, pw))),
            ("game", s(pad(&st.game, gw))),
            ("action", s(action)),
        ];
        let key = if st.files > 0 {
            args.push(("files", FluentValue::from(st.files)));
            args.push(("size", s(crate::progress::mb(st.bytes))));
            "sync-line-moved"
        } else {
            "sync-line"
        };
        line(t, key, &args);
    }
    warnings(t, &r.warnings);
    if r.dry_run {
        line(t, "dry-run", &[]);
    }
    if let Some(e) = elapsed {
        line(
            t,
            "sync-elapsed",
            &[("secs", s(format!("{:.1}", e.as_secs_f64())))],
        );
    }
}

/// After `play`: how the emulator exited, then the upload's stream line and warnings.
pub fn play(t: &I18n, emulator: &str, r: &PlayReport, color: bool) {
    line(
        t,
        "play-done",
        &[
            ("emulator", s(emulator.to_owned())),
            ("code", FluentValue::from(r.exit_code)),
        ],
    );
    let st = &r.uploaded;
    let action = paint(
        st.action,
        t.t(&format!("action-{}", snake(&st.action)), &[]),
        color,
    );
    let mut args = vec![
        ("player", s(st.player.clone())),
        ("game", s(st.game.clone())),
        ("action", s(action)),
    ];
    let key = if st.files > 0 {
        args.push(("files", FluentValue::from(st.files)));
        args.push(("size", s(crate::progress::mb(st.bytes))));
        "sync-line-moved"
    } else {
        "sync-line"
    };
    line(t, key, &args);
    warnings(t, &r.warnings);
}

/// Every conflict with its heads, numbered as `play` asks.
pub fn conflicts(t: &I18n, list: &[ConflictInfo]) {
    if list.is_empty() {
        line(t, "conflicts-none", &[]);
    }
    for c in list {
        line(
            t,
            "conflict-stream",
            &[
                ("player", s(c.player.clone())),
                ("emulator", s(c.emulator.clone())),
                ("game", s(c.game.clone())),
            ],
        );
        for (i, h) in c.heads.iter().enumerate() {
            println!(
                "  {}",
                t.t(
                    "conflict-head",
                    &[
                        ("n", FluentValue::from(i + 1)),
                        ("version", s(h.version.clone())),
                        ("device", s(h.device_name.clone())),
                        ("files", FluentValue::from(h.files)),
                    ],
                )
            );
        }
    }
}

pub fn leases(t: &I18n, list: &[LeaseInfo], now_ms: i64) {
    if list.is_empty() {
        line(t, "lease-none", &[]);
    }
    for l in list {
        let ago = now_ms.saturating_sub(l.renewed_at_ms).max(0) / 1000;
        line(
            t,
            if l.live {
                "lease-line"
            } else {
                "lease-line-stale"
            },
            &[
                ("player", s(l.player.clone())),
                ("emulator", s(l.emulator.clone())),
                ("game", s(l.game.clone())),
                ("device", s(l.device.clone())),
                ("ago", FluentValue::from(ago)),
                ("ours", s(if l.ours { "yes" } else { "no" })),
            ],
        );
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
    fn color_needs_a_terminal_and_no_no_color() {
        assert!(color_allowed(None, true));
        assert!(color_allowed(Some("".into()), true));
        assert!(!color_allowed(Some("1".into()), true));
        assert!(!color_allowed(None, false));
    }

    #[test]
    fn actions_are_painted_only_when_asked() {
        assert_eq!(paint(StreamAction::Upload, "up".into(), false), "up");
        assert_eq!(
            paint(StreamAction::Upload, "up".into(), true),
            "\x1b[32mup\x1b[0m"
        );
        assert!(paint(StreamAction::Conflict, "c".into(), true).starts_with("\x1b[31m"));
        assert!(paint(StreamAction::Skipped, "s".into(), true).starts_with("\x1b[33m"));
    }

    #[test]
    fn pad_counts_characters_not_bytes() {
        assert_eq!(pad("Ana", 5), "Ana  ");
        assert_eq!(pad("Íñigo", 5), "Íñigo");
    }

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
            assert!(repo.contains("lemusync init"), "{repo}");
            let other = error_text(&t, &EngineError::Io("disk full".into()));
            assert!(other.contains("disk full"), "{other}");
        }
    }
}
