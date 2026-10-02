//! The password, signed session cookies, and the owner-only layers (R11, R12).

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::App;
use crate::store;

/// The name of the session cookie.
pub const COOKIE_NAME: &str = "mewiki_session";

/// How long a session lasts from login. Using the wiki doesn't extend it.
pub const SESSION_LIFETIME: Duration = Duration::from_secs(90 * 24 * 60 * 60);

/// How long a failed login waits before answering, which caps guessing at about 3,600 attempts an hour.
pub const FAILED_LOGIN_DELAY: Duration = Duration::from_secs(1);

type HmacSha256 = Hmac<Sha256>;

/// Checks passwords and signs and checks session cookies.
///
/// The signing key is `HMAC-SHA256(secret.key, password)`, so changing the password, or deleting `secret.key`,
/// cancels every session.
pub struct Auth {
    key: [u8; 32],
    password_hash: [u8; 32],
    cookie_secure: bool,
    login_queue: tokio::sync::Mutex<()>,
}

impl Auth {
    /// Builds the checker from the secret key and the password.
    ///
    /// `cookie_secure` adds the `Secure` attribute to the session cookie; turn it off only for plain-HTTP testing.
    pub fn new(secret: &[u8], password: &str, cookie_secure: bool) -> Self {
        let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts keys of any length");
        mac.update(password.as_bytes());
        Auth {
            key: mac.finalize().into_bytes().into(),
            password_hash: Sha256::digest(password.as_bytes()).into(),
            cookie_secure,
            login_queue: tokio::sync::Mutex::new(()),
        }
    }

    /// Reads `secret.key` from `config_dir`, creating it with 32 random bytes on first run.
    pub fn load_or_create_secret(config_dir: &Path) -> io::Result<Vec<u8>> {
        let path = config_dir.join("secret.key");
        match fs::read(&path) {
            Ok(secret) if !secret.is_empty() => return Ok(secret),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let mut secret = vec![0u8; 32];
        getrandom::fill(&mut secret).map_err(io::Error::other)?;
        store::write_atomic(&path, &secret)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(secret)
    }

    /// Returns whether `input` is the password, comparing SHA-256 hashes in constant time so the time taken
    /// reveals neither the password nor its length.
    pub fn password_matches(&self, input: &str) -> bool {
        let input_hash: [u8; 32] = Sha256::digest(input.as_bytes()).into();
        input_hash.ct_eq(&self.password_hash).into()
    }

    /// Checks a login attempt. Attempts are handled one at a time, and a failure waits [`FAILED_LOGIN_DELAY`]
    /// before answering, so a flood of guesses also delays the owner's own login.
    pub async fn login(&self, input: &str) -> bool {
        let _turn = self.login_queue.lock().await;
        if self.password_matches(input) {
            return true;
        }
        tokio::time::sleep(FAILED_LOGIN_DELAY).await;
        false
    }

    /// Returns a session cookie value that expires at `expiry`, in seconds since the Unix epoch.
    pub fn sign(&self, expiry: u64) -> String {
        format!("{expiry}.{}", hex(&self.mac(expiry)))
    }

    /// Returns whether `value` is a session cookie this key signed and that hasn't expired at `now`.
    pub fn verify(&self, value: &str, now: u64) -> bool {
        let Some((expiry, signature)) = value.split_once('.') else {
            return false;
        };
        let Ok(expiry) = expiry.parse::<u64>() else {
            return false;
        };
        let expected = hex(&self.mac(expiry));
        expiry > now && bool::from(expected.as_bytes().ct_eq(signature.as_bytes()))
    }

    /// Returns whether the request carries a valid session cookie.
    pub fn is_owner(&self, headers: &HeaderMap) -> bool {
        session_cookie(headers).is_some_and(|value| self.verify(value, now()))
    }

    /// Returns the `Set-Cookie` value that starts a new session.
    pub fn new_session_cookie(&self) -> String {
        let value = self.sign(now() + SESSION_LIFETIME.as_secs());
        self.cookie(&value, SESSION_LIFETIME.as_secs())
    }

    /// Returns the `Set-Cookie` value that clears the session in this browser.
    pub fn clear_session_cookie(&self) -> String {
        self.cookie("", 0)
    }

    fn cookie(&self, value: &str, max_age: u64) -> String {
        let secure = if self.cookie_secure { "; Secure" } else { "" };
        format!("{COOKIE_NAME}={value}; Max-Age={max_age}; Path=/; HttpOnly; SameSite=Strict{secure}")
    }

    fn mac(&self, expiry: u64) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC accepts keys of any length");
        mac.update(expiry.to_string().as_bytes());
        mac.finalize().into_bytes().into()
    }
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(COOKIE_NAME)?.strip_prefix('='))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Answers `401` to any request without a valid session. Wraps the owner API routes.
pub async fn require_owner_api(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    if app.auth.is_owner(request.headers()) {
        next.run(request).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// Sends any request without a valid session to the login page, which returns there afterwards. Wraps the owner
/// pages.
pub async fn require_owner_page(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    if app.auth.is_owner(request.headers()) {
        next.run(request).await
    } else {
        let target = request.uri().path_and_query().map_or("/", |p| p.as_str());
        Redirect::to(&format!("/_/login?next={}", percent_encode(target))).into_response()
    }
}

/// Returns `next` when it is a path on this site, or `/` otherwise, so a login link can't send the owner elsewhere.
pub fn safe_next(next: Option<&str>) -> &str {
    match next {
        Some(path) if path.starts_with('/') && !path.starts_with("//") && !path.contains('\\') => path,
        _ => "/",
    }
}

/// Percent-encodes everything except unreserved characters and `/`.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(password: &str) -> Auth {
        Auth::new(b"secret", password, true)
    }

    #[test]
    fn accepts_its_own_unexpired_cookie() {
        let auth = auth("pw");
        assert!(auth.verify(&auth.sign(1_000), 999));
    }

    #[test]
    fn rejects_an_expired_cookie() {
        let auth = auth("pw");
        assert!(!auth.verify(&auth.sign(1_000), 1_000));
    }

    #[test]
    fn rejects_cookies_after_the_password_changes() {
        let cookie = auth("old").sign(1_000);
        assert!(!auth("new").verify(&cookie, 0));
    }

    #[test]
    fn rejects_cookies_after_the_secret_changes() {
        let cookie = Auth::new(b"one", "pw", true).sign(1_000);
        assert!(!Auth::new(b"two", "pw", true).verify(&cookie, 0));
    }

    #[test]
    fn rejects_a_cookie_with_a_moved_expiry() {
        let auth = auth("pw");
        let signature = auth.sign(1_000).split_once('.').unwrap().1.to_owned();
        assert!(!auth.verify(&format!("9999999999.{signature}"), 0));
    }

    #[test]
    fn rejects_malformed_cookies() {
        let auth = auth("pw");
        for value in ["", "nodot", "abc.def", "1000.", ".abc"] {
            assert!(!auth.verify(value, 0), "{value:?}");
        }
    }

    #[test]
    fn matches_only_the_exact_password() {
        let auth = auth("correct horse");
        assert!(auth.password_matches("correct horse"));
        assert!(!auth.password_matches("correct horse "));
        assert!(!auth.password_matches(""));
    }

    #[test]
    fn finds_the_session_among_other_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "a=1; mewiki_session=v.x; b=2".parse().unwrap());
        assert_eq!(session_cookie(&headers), Some("v.x"));
    }

    #[test]
    fn ignores_cookies_whose_name_only_starts_the_same() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "mewiki_session_old=v.x".parse().unwrap());
        assert_eq!(session_cookie(&headers), None);
    }

    #[test]
    fn sets_secure_cookie_attributes() {
        let cookie = auth("pw").new_session_cookie();
        assert!(cookie.starts_with("mewiki_session="));
        assert!(cookie.contains("; HttpOnly; SameSite=Strict; Secure"));
        assert!(cookie.contains("Max-Age=7776000"));
        assert!(!Auth::new(b"s", "pw", false).new_session_cookie().contains("Secure"));
    }

    #[test]
    fn keeps_the_owner_on_this_site_after_login() {
        assert_eq!(safe_next(Some("/mushroom")), "/mushroom");
        for bad in [
            None,
            Some("https://evil.example"),
            Some("//evil.example"),
            Some("/\\evil.example"),
            Some(""),
        ] {
            assert_eq!(safe_next(bad), "/", "{bad:?}");
        }
    }

    #[test]
    fn percent_encodes_query_characters() {
        assert_eq!(percent_encode("/_/new?parent=/a b"), "/_/new%3Fparent%3D/a%20b");
    }

    #[test]
    fn creates_the_secret_once_and_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let first = Auth::load_or_create_secret(dir.path()).unwrap();
        let second = Auth::load_or_create_secret(dir.path()).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(first, second);
    }
}
