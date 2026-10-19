//! Scrubs OAuth secrets out of any text before it can reach output or logs (V-SEC-4).

const SECRET_PARAMS: [&str; 6] = [
    "access_token",
    "refresh_token",
    "client_secret",
    "code",
    "code_verifier",
    "id_token",
];
const VALUE_END: [char; 9] = ['&', ' ', '"', '\'', ',', '}', ')', '\n', '\r'];
const MARK: &str = "REDACTED";

/// Replaces OAuth secret values (query/form `name=value`, JSON `"name": "value"`, `Bearer <token>`).
#[must_use]
pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for name in SECRET_PARAMS {
        out = redact_query(&out, name);
        out = redact_json(&out, name);
    }
    redact_bearer(&out)
}

fn starts_param(text: &str, at: usize) -> bool {
    at == 0
        || !text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn redact_query(text: &str, name: &str) -> String {
    let needle = format!("{name}=");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(&needle) {
        let absolute = text.len() - rest.len() + pos;
        out.push_str(&rest[..pos + needle.len()]);
        let value = &rest[pos + needle.len()..];
        let end = value.find(VALUE_END).unwrap_or(value.len());
        if starts_param(text, absolute) {
            out.push_str(MARK);
        } else {
            out.push_str(&value[..end]);
        }
        rest = &value[end..];
    }
    out.push_str(rest);
    out
}

fn redact_json(text: &str, name: &str) -> String {
    let needle = format!("\"{name}\":");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(&needle) {
        out.push_str(&rest[..pos + needle.len()]);
        let after = &rest[pos + needle.len()..];
        let trimmed = after.trim_start();
        out.push_str(&after[..after.len() - trimmed.len()]);
        if let Some(value) = trimmed.strip_prefix('"') {
            let end = value.find('"').unwrap_or(value.len());
            out.push('"');
            out.push_str(MARK);
            rest = &value[end..];
        } else {
            rest = trimmed;
        }
    }
    out.push_str(rest);
    out
}

fn redact_bearer(text: &str) -> String {
    let needle = "Bearer ";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(needle) {
        out.push_str(&rest[..pos + needle.len()]);
        let value = &rest[pos + needle.len()..];
        let end = value.find(VALUE_END).unwrap_or(value.len());
        out.push_str(MARK);
        rest = &value[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_the_opendal_token_url_from_the_spec() {
        let text = "Unexpected (temporary) at stat, context: { url: https://oauth2.googleapis.com/token?refresh_token=FAKE-REFRESH-TOKEN&client_id=FAKE-CLIENT-ID&client_secret=FAKE-CLIENT-SECRET&grant_type=refresh_token, called: reqwest::send, service: gdrive, path: a } => send http request, source: error sending request";
        let out = redact(text);
        assert!(!out.contains("FAKE-REFRESH-TOKEN"));
        assert!(!out.contains("FAKE-CLIENT-SECRET"));
        assert!(
            out.contains("client_id=FAKE-CLIENT-ID"),
            "client ids aren't secret"
        );
        assert!(out.contains("grant_type=refresh_token"));
    }

    #[test]
    fn scrubs_json_bodies_and_bearer_headers() {
        let out = redact(
            r#"{"access_token": "ya29.SECRET", "expires_in": 3599, "refresh_token":"1//SECRET2"}"#,
        );
        assert!(!out.contains("SECRET"));
        assert!(out.contains("3599"));
        assert_eq!(
            redact("authorization: Bearer ya29.SECRET rest"),
            "authorization: Bearer REDACTED rest"
        );
    }

    #[test]
    fn leaves_lookalikes_alone() {
        assert_eq!(redact("barcode=1 zipcode=2"), "barcode=1 zipcode=2");
        assert_eq!(redact("code=abc&state=xyz"), "code=REDACTED&state=xyz");
    }
}
