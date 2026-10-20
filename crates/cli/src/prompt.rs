//! Terminal questions. Secrets are read without echo and never come from argv or the environment.

use std::io::{BufRead as _, IsTerminal as _, Write as _};

use leemusync_engine::EngineError;
use zeroize::Zeroizing;

use crate::i18n::{I18n, s};

pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn need_tty(t: &I18n, what: &str) -> Result<(), EngineError> {
    if interactive() {
        Ok(())
    } else {
        Err(EngineError::Usage(
            t.t("error-needs-tty", &[("what", s(what))]),
        ))
    }
}

pub fn secret(t: &I18n, key: &str) -> Result<Zeroizing<String>, EngineError> {
    need_tty(t, t.t(key, &[]).trim_end_matches(':'))?;
    let value = Zeroizing::new(rpassword::prompt_password(format!("{} ", t.t(key, &[])))?);
    Ok(Zeroizing::new(value.trim().to_owned()))
}

pub fn new_passphrase(t: &I18n) -> Result<Zeroizing<String>, EngineError> {
    let first = secret(t, "prompt-passphrase")?;
    let second = secret(t, "prompt-passphrase-again")?;
    if *first != *second {
        return Err(EngineError::Usage(t.t("prompt-passphrases-differ", &[])));
    }
    Ok(first)
}

fn line(question: &str) -> Result<String, EngineError> {
    eprint!("{question} ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(answer.trim().to_lowercase())
}

pub fn yes(t: &I18n, question: &str) -> Result<bool, EngineError> {
    need_tty(t, question)?;
    let answer = line(&format!("{question} {}", t.t("prompt-yes-no", &[])))?;
    Ok(answer == t.t("answer-yes", &[]))
}

/// `Some(true)` = replace, `Some(false)` = keep, `None` = no clear answer.
pub fn keep_or_replace(t: &I18n, question: &str) -> Result<Option<bool>, EngineError> {
    need_tty(t, question)?;
    let answer = line(&format!(
        "{question}\n{}",
        t.t("prompt-keep-or-replace-keys", &[])
    ))?;
    Ok(if answer == t.t("answer-replace", &[]) {
        Some(true)
    } else if answer == t.t("answer-keep", &[]) {
        Some(false)
    } else {
        None
    })
}
