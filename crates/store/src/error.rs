//! Store errors, classified for retry and user prompts (storage-backends §Interface).

use std::fmt;

use opendal::ErrorKind;

use crate::redact::redact;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorClass {
    NotFound,
    AlreadyExists,
    /// Retry with backoff.
    Transient,
    /// Ask the user to log in again.
    Auth,
    Quota,
    Permanent,
}

#[derive(Debug, Clone)]
pub struct StoreError {
    class: ErrorClass,
    message: String,
}

impl StoreError {
    #[must_use]
    pub fn new(class: ErrorClass, message: &str) -> Self {
        Self {
            class,
            message: redact(message),
        }
    }

    #[must_use]
    pub fn class(&self) -> ErrorClass {
        self.class
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StoreError {}

impl From<opendal::Error> for StoreError {
    fn from(e: opendal::Error) -> Self {
        let text = e.to_string();
        Self::new(classify(e.kind(), e.is_temporary(), &text), &text)
    }
}

pub(crate) fn classify(kind: ErrorKind, temporary: bool, text: &str) -> ErrorClass {
    match kind {
        ErrorKind::NotFound => ErrorClass::NotFound,
        ErrorKind::ConditionNotMatch | ErrorKind::AlreadyExists => ErrorClass::AlreadyExists,
        ErrorKind::PermissionDenied => ErrorClass::Auth,
        ErrorKind::RateLimited => ErrorClass::Transient,
        _ if temporary => ErrorClass::Transient,
        _ if text.contains("status: 401") => ErrorClass::Auth,
        _ if text.contains("storageQuotaExceeded") || text.contains("status: 507") => {
            ErrorClass::Quota
        }
        _ if text.contains("status: 429") || text.contains("status: 5") => ErrorClass::Transient,
        _ => ErrorClass::Permanent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendal::ErrorKind;

    #[test]
    fn classifies_by_kind_then_by_http_status() {
        assert_eq!(
            classify(ErrorKind::NotFound, false, ""),
            ErrorClass::NotFound
        );
        assert_eq!(
            classify(ErrorKind::ConditionNotMatch, false, ""),
            ErrorClass::AlreadyExists
        );
        assert_eq!(
            classify(ErrorKind::PermissionDenied, false, ""),
            ErrorClass::Auth
        );
        assert_eq!(
            classify(ErrorKind::RateLimited, false, ""),
            ErrorClass::Transient
        );
        assert_eq!(
            classify(ErrorKind::Unexpected, true, ""),
            ErrorClass::Transient
        );
        // OpenDAL's gdrive service reports a 401 as Unexpected (seen 2026-10-18 with a fake token).
        assert_eq!(
            classify(
                ErrorKind::Unexpected,
                false,
                "response: Parts { status: 401, ..."
            ),
            ErrorClass::Auth
        );
        assert_eq!(
            classify(
                ErrorKind::Unexpected,
                false,
                "status: 403 ... storageQuotaExceeded"
            ),
            ErrorClass::Quota
        );
        assert_eq!(
            classify(ErrorKind::Unexpected, false, "status: 503"),
            ErrorClass::Transient
        );
        assert_eq!(
            classify(ErrorKind::Unexpected, false, "status: 400"),
            ErrorClass::Permanent
        );
    }

    #[test]
    fn messages_are_always_redacted() {
        let e = StoreError::new(ErrorClass::Permanent, "refresh_token=SECRET");
        assert_eq!(e.message(), "refresh_token=REDACTED");
        assert_eq!(e.to_string(), "refresh_token=REDACTED");
    }

    #[test]
    fn provider_errors_are_redacted_on_conversion() {
        let raw = opendal::Error::new(
            ErrorKind::Unexpected,
            "send failed: refresh_token=SECRET&client_secret=SECRET2",
        );
        let e = StoreError::from(raw);
        assert!(!e.message().contains("SECRET"), "{}", e.message());
        assert!(!e.to_string().contains("SECRET"));
    }
}
