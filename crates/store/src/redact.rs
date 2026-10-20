//! Scrubs OAuth secrets out of any text before it can reach output or logs (V-SEC-4).

/// Secret names, lowercase. The flag says whether `name: value` (no quotes) counts; it is off for
/// words that also appear in prose ("error code: 404").
const SECRET_NAMES: [(&str, bool); 16] = [
    ("access_token", true),
    ("refresh_token", true),
    ("client_secret", true),
    ("code", false),
    ("code_verifier", true),
    ("id_token", true),
    ("password", true),
    ("token", false),
    ("api_key", true),
    ("apikey", true),
    ("x-amz-signature", true),
    ("x-amz-credential", true),
    ("x-amz-security-token", true),
    ("secret_access_key", true),
    // SigV4 `Authorization: AWS4-HMAC-SHA256 Credential=<key id>/…, Signature=…`.
    ("credential", false),
    ("signature", false),
];
const VALUE_END: [char; 9] = ['&', ' ', '"', '\'', ',', '}', ')', '\n', '\r'];
const MARK: &str = "REDACTED";

/// Replaces OAuth secret values: `name=value`, `name: value`, JSON `"name": "value"` (also
/// backslash-escaped), `Bearer <token>` and `Authorization: Basic <token>`. Names and schemes are
/// matched without regard to case. `token_type` and words that merely end in a secret name are kept.
#[must_use]
pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for (name, colon_ok) in SECRET_NAMES {
        out = redact_named(&out, name, colon_ok);
    }
    out = redact_scheme(&out, "bearer ", false);
    redact_scheme(&out, "basic ", true)
}

/// True when `at` is not in the middle of a longer identifier.
fn starts_param(text: &str, at: usize) -> bool {
    !text[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn skip_blanks(text: &str, from: usize) -> usize {
    from + (text[from..].len() - text[from..].trim_start_matches([' ', '\t']).len())
}

/// Where the secret value starting at `at` begins and ends.
fn value_span(text: &str, at: usize) -> (usize, usize) {
    let rest = &text[at..];
    if let Some(inner) = rest.strip_prefix("\\\"") {
        let start = at + 2;
        let end = [inner.find("\\\""), inner.find('"')]
            .into_iter()
            .flatten()
            .min()
            .map_or(text.len(), |n| start + n);
        (start, end)
    } else if rest.starts_with(['"', '\'']) {
        let quote = &rest[..1];
        let start = at + 1;
        (
            start,
            text[start..].find(quote).map_or(text.len(), |n| start + n),
        )
    } else {
        (at, rest.find(VALUE_END).map_or(text.len(), |n| at + n))
    }
}

fn redact_named(text: &str, name: &str, colon_ok: bool) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut search = 0;
    while let Some(rel) = lower[search..].find(name) {
        let at = search + rel;
        let mut i = at + name.len();
        search = i;
        if !starts_param(text, at) {
            continue;
        }
        let quoted_key = text[i..].starts_with("\\\"") || text[i..].starts_with('"');
        if text[i..].starts_with("\\\"") {
            i += 2;
        } else if quoted_key {
            i += 1;
        }
        i = skip_blanks(text, i);
        let sep_ok = match text[i..].chars().next() {
            Some('=') => true,
            Some(':') => colon_ok || quoted_key,
            _ => false,
        };
        if !sep_ok {
            continue;
        }
        let (start, end) = value_span(text, skip_blanks(text, i + 1));
        if start == end {
            continue;
        }
        out.push_str(&text[cursor..start]);
        out.push_str(MARK);
        cursor = end;
        search = end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// `scheme` is lowercase with a trailing space. `needs_header` limits the match to the value of an
/// `Authorization:` header, for words that are also plain English ("Basic usage").
fn redact_scheme(text: &str, scheme: &str, needs_header: bool) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut search = 0;
    while let Some(rel) = lower[search..].find(scheme) {
        let at = search + rel;
        let start = at + scheme.len();
        search = start;
        let header_ok = !needs_header || lower[..at].trim_end().ends_with("authorization:");
        if !starts_param(text, at) || !header_ok {
            continue;
        }
        let end = text[start..]
            .find(VALUE_END)
            .map_or(text.len(), |n| start + n);
        if start == end {
            continue;
        }
        out.push_str(&text[cursor..start]);
        out.push_str(MARK);
        cursor = end;
        search = end;
    }
    out.push_str(&text[cursor..]);
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

    #[test]
    fn names_and_schemes_match_any_case() {
        assert_eq!(
            redact("Refresh_Token=abc&x=1"),
            "Refresh_Token=REDACTED&x=1"
        );
        assert_eq!(
            redact("Authorization: BEARER abc rest"),
            "Authorization: BEARER REDACTED rest"
        );
        assert_eq!(
            redact("authorization: bearer abc"),
            "authorization: bearer REDACTED"
        );
    }

    #[test]
    fn tolerates_whitespace_around_separators() {
        assert_eq!(
            redact("client_secret: abc, ok"),
            "client_secret: REDACTED, ok"
        );
        assert_eq!(
            redact("client_secret = abc ok"),
            "client_secret = REDACTED ok"
        );
        assert_eq!(
            redact(r#"{"access_token" : "abc", "n": 1}"#),
            r#"{"access_token" : "REDACTED", "n": 1}"#
        );
    }

    #[test]
    fn handles_backslash_escaped_json() {
        let out = redact(r#"body: {\"access_token\":\"ya29.SECRET\",\"expires_in\":3599}"#);
        assert!(!out.contains("SECRET"), "{out}");
        assert!(out.contains("3599"));
    }

    #[test]
    fn scrubs_the_extra_names() {
        for text in [
            "password=SECRET",
            "password: SECRET",
            "token=SECRET",
            r#"{"token":"SECRET"}"#,
            "api_key=SECRET",
            "apikey=SECRET",
            "X-Amz-Signature=SECRET",
            "x-amz-credential=SECRET",
            "x-amz-security-token=SECRET",
            "secret_access_key: SECRET",
            "AWS4-HMAC-SHA256 Credential=SECRET/20261020/auto/s3/aws4_request, SignedHeaders=host, Signature=SECRET",
            "Authorization: Basic SECRET",
        ] {
            assert!(
                !redact(text).contains("SECRET"),
                "{text} -> {}",
                redact(text)
            );
        }
    }

    #[test]
    fn token_type_and_prose_are_not_secrets() {
        assert_eq!(
            redact(r#"{"token_type":"Bearer"}"#),
            r#"{"token_type":"Bearer"}"#
        );
        assert_eq!(redact("token_type=Bearer&a=1"), "token_type=Bearer&a=1");
        assert_eq!(
            redact("Basic usage: error code: 404"),
            "Basic usage: error code: 404"
        );
        assert_eq!(redact("mytoken=1 x-token=2"), "mytoken=1 x-token=2");
    }
}
