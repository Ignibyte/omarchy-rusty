//! The PIN that guards the secrets tools: an argon2id hash on disk, one short-lived unlock
//! token in memory per server process, and a lockout after repeated wrong PINs. The PIN
//! protects the tools, not the file: `~/.rusty/.secret` stays owner-readable, because the
//! back end reads it headless and an agent with a shell reads it regardless. Nothing here logs a PIN, a
//! token or a value.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::{OsRng, RngCore};

/// The shortest PIN or passphrase accepted, in characters.
pub const MIN_PIN_CHARS: usize = 6;
/// Wrong PINs in a row before the lockout.
pub const MAX_FAILURES: u32 = 5;
/// How long the lockout lasts.
pub const LOCKOUT: Duration = Duration::from_secs(60);
/// The unlock's life when the setting is unset, in minutes.
pub const DEFAULT_TIMEOUT_MINUTES: u64 = 5;
/// The settings key that sizes an unlock, in minutes.
pub const TIMEOUT_SETTING: &str = "pin_timeout_minutes";

/// What [`PinLock::unlock`] hands back.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Unlock {
    /// The token the reveal and update tools need.
    pub token: String,
    /// How long it lives.
    pub expires_in_seconds: u64,
}

/// What a client asks before it offers to reveal or change a secret.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PinStatus {
    /// A PIN exists.
    pub set: bool,
    /// A token is live right now.
    pub unlocked: bool,
    /// Seconds left on a lockout, or zero.
    pub locked_out_seconds: u64,
}

#[derive(Default)]
struct State {
    failures: u32,
    locked_until: Option<Instant>,
    token: Option<(String, Instant)>,
}

/// The PIN and its unlock, owned by the back end.
pub struct PinLock {
    path: PathBuf,
    state: Mutex<State>,
}

impl PinLock {
    /// A lock whose hash lives at `path`, set or not.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Mutex::new(State::default()),
        }
    }

    /// Whether a PIN has been set.
    pub fn is_set(&self) -> bool {
        self.path.is_file()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Set the PIN. Changing an existing one needs the live token. The new PIN relocks.
    pub fn set(&self, pin: &str, token: Option<&str>) -> Result<(), String> {
        if self.is_set() {
            self.check(token.unwrap_or(""))
                .map_err(|_| "unlock with the current PIN before changing it".to_string())?;
        }
        if pin.chars().count() < MIN_PIN_CHARS {
            return Err(format!("a PIN needs at least {MIN_PIN_CHARS} characters"));
        }
        if pin.contains(['\n', '\r']) {
            return Err("a PIN cannot hold a line break".to_string());
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|e| format!("hash the PIN: {e}"))?
            .to_string();
        write_private(&self.path, &hash)?;
        self.lock();
        Ok(())
    }

    /// Verify the PIN and hand back a token good for `ttl`. Five wrong PINs in a row
    /// refuse every try for a minute.
    pub fn unlock(&self, pin: &str, ttl: Duration) -> Result<Unlock, String> {
        if !self.is_set() {
            return Err("no PIN is set; set one with secret_pin_set first".to_string());
        }
        let mut state = self.state();
        let now = Instant::now();
        if let Some(until) = state.locked_until {
            if until > now {
                let left = until.saturating_duration_since(now).as_secs().max(1);
                return Err(format!("locked for another {left} seconds"));
            }
            state.locked_until = None;
        }
        let stored =
            std::fs::read_to_string(&self.path).map_err(|e| format!("read the PIN file: {e}"))?;
        let parsed = PasswordHash::new(stored.trim())
            .map_err(|e| format!("the PIN file does not parse: {e}"))?;
        if Argon2::default()
            .verify_password(pin.as_bytes(), &parsed)
            .is_err()
        {
            state.failures += 1;
            if state.failures >= MAX_FAILURES {
                state.failures = 0;
                state.locked_until = Some(now + LOCKOUT);
                return Err(format!(
                    "wrong PIN {MAX_FAILURES} times; locked for a minute"
                ));
            }
            return Err("wrong PIN".to_string());
        }
        state.failures = 0;
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        state.token = Some((token.clone(), now + ttl));
        Ok(Unlock {
            token,
            expires_in_seconds: ttl.as_secs(),
        })
    }

    /// Accept the live token until its expiry; an expired one is dropped.
    pub fn check(&self, token: &str) -> Result<(), String> {
        let mut state = self.state();
        let now = Instant::now();
        let verdict = match &state.token {
            Some((live, until)) if now < *until => usize::from(!same(live, token)),
            Some(_) => 2,
            None => 3,
        };
        if verdict == 2 {
            state.token = None;
        }
        match verdict {
            0 => Ok(()),
            1 => Err("that unlock is not the live one; unlock again".to_string()),
            2 => Err("the unlock has expired; unlock again".to_string()),
            _ => Err("locked; unlock with the PIN".to_string()),
        }
    }

    /// May a write to the vault go ahead? With no PIN set there is nothing to unlock and
    /// it may; once one is set, only with the live token (TICKET-049).
    pub fn check_write(&self, token: Option<&str>) -> Result<(), String> {
        if !self.is_set() {
            return Ok(());
        }
        match token.filter(|t| !t.is_empty()) {
            Some(token) => self.check(token),
            None => Err("the vault is locked; unlock with the PIN and pass the token".to_string()),
        }
    }

    /// Drop the live token.
    pub fn lock(&self) {
        self.state().token = None;
    }

    /// Set, unlocked, and any lockout left.
    pub fn status(&self) -> PinStatus {
        let state = self.state();
        let now = Instant::now();
        PinStatus {
            set: self.is_set(),
            unlocked: state.token.as_ref().is_some_and(|(_, until)| now < *until),
            locked_out_seconds: state
                .locked_until
                .filter(|until| *until > now)
                .map(|until| until.saturating_duration_since(now).as_secs())
                .unwrap_or(0),
        }
    }
}

/// A comparison that does not stop at the first differing byte.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Write `text` to `path` readable by its owner alone.
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        file.write_all(text.as_bytes())
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh(name: &str) -> (PathBuf, PinLock) {
        let dir = std::env::temp_dir().join(format!("rusty_pin_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lock = PinLock::new(dir.join(".pin"));
        (dir, lock)
    }

    #[test]
    fn set_unlock_check_lock() {
        let (dir, lock) = fresh("round_trip");
        assert!(!lock.is_set());
        lock.set("123456", None).unwrap();
        assert!(lock.is_set());
        let status = lock.status();
        assert!(status.set && !status.unlocked && status.locked_out_seconds == 0);
        let unlock = lock.unlock("123456", Duration::from_secs(300)).unwrap();
        assert_eq!(unlock.token.len(), 64);
        assert_eq!(unlock.expires_in_seconds, 300);
        lock.check(&unlock.token).unwrap();
        assert!(lock.status().unlocked);
        assert!(lock.check("not the token").is_err());
        lock.lock();
        assert!(lock.check(&unlock.token).is_err());
        assert!(!lock.status().unlocked);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn writes_need_the_unlock_once_a_pin_is_set() {
        let (dir, lock) = fresh("writes");
        lock.check_write(None).unwrap();
        lock.check_write(Some("anything")).unwrap();
        lock.set("123456", None).unwrap();
        let err = lock.check_write(None).unwrap_err();
        assert!(err.contains("locked"), "{err}");
        assert!(lock.check_write(Some("")).is_err());
        let unlock = lock.unlock("123456", Duration::from_secs(60)).unwrap();
        assert!(lock.check_write(Some("not the token")).is_err());
        assert!(
            lock.check_write(None).is_err(),
            "a live unlock elsewhere does not open a write without the token"
        );
        lock.check_write(Some(&unlock.token)).unwrap();
        lock.lock();
        assert!(lock.check_write(Some(&unlock.token)).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_short_pin_and_a_wrong_pin_are_refused() {
        let (dir, lock) = fresh("refused");
        let err = lock.set("12345", None).unwrap_err();
        assert!(err.contains('6'), "{err}");
        assert!(
            lock.unlock("123456", Duration::from_secs(60)).is_err(),
            "no PIN yet"
        );
        lock.set("123456", None).unwrap();
        let err = lock.unlock("000000", Duration::from_secs(60)).unwrap_err();
        assert_eq!(err, "wrong PIN");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn five_wrong_pins_lock_out_for_a_minute() {
        let (dir, lock) = fresh("lockout");
        lock.set("123456", None).unwrap();
        for _ in 0..4 {
            assert_eq!(
                lock.unlock("000000", Duration::from_secs(60)).unwrap_err(),
                "wrong PIN"
            );
        }
        let err = lock.unlock("000000", Duration::from_secs(60)).unwrap_err();
        assert!(err.contains("locked"), "{err}");
        assert!(lock.status().locked_out_seconds > 0);
        let err = lock.unlock("123456", Duration::from_secs(60)).unwrap_err();
        assert!(err.contains("locked"), "the right PIN waits too: {err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_expired_unlock_is_no_unlock() {
        let (dir, lock) = fresh("expiry");
        lock.set("123456", None).unwrap();
        let unlock = lock.unlock("123456", Duration::from_secs(0)).unwrap();
        let err = lock.check(&unlock.token).unwrap_err();
        assert!(err.contains("expired"), "{err}");
        assert!(!lock.status().unlocked);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn changing_the_pin_needs_the_live_unlock() {
        let (dir, lock) = fresh("change");
        lock.set("123456", None).unwrap();
        assert!(lock.set("654321", None).is_err());
        let unlock = lock.unlock("123456", Duration::from_secs(60)).unwrap();
        lock.set("654321", Some(&unlock.token)).unwrap();
        assert!(lock.check(&unlock.token).is_err(), "a new PIN relocks");
        assert!(lock.unlock("123456", Duration::from_secs(60)).is_err());
        lock.unlock("654321", Duration::from_secs(60)).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_pin_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, lock) = fresh("private");
        lock.set("123456", None).unwrap();
        let mode = std::fs::metadata(dir.join(".pin"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
