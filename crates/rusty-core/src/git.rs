//! What the vault and the skills store share about the git repositories Rusty keeps.

use std::path::Path;
use std::process::{Command, Stdio};

/// The name Rusty commits under in its own repositories when git has none for the user.
pub(crate) const FALLBACK_NAME: &str = "Rusty";
/// The email that goes with [`FALLBACK_NAME`].
pub(crate) const FALLBACK_EMAIL: &str = "rusty@localhost";

/// Make sure commits in `repo` have an author. When git cannot determine a committer
/// identity (no `user.name` and `user.email` in any config, and none it can guess), every
/// commit fails and the store keeps no history; so Rusty sets its own in that repository
/// only (TICKET-067). A user with an identity keeps it, and git's global configuration is
/// never written.
pub(crate) fn ensure_identity(repo: &Path) {
    ensure_identity_with(repo, |_| {});
}

/// [`ensure_identity`], with `configure` applied to every git it runs: a test takes the
/// user's identity away for git alone, without touching the process environment.
pub(crate) fn ensure_identity_with(repo: &Path, configure: impl Fn(&mut Command)) {
    let git = |args: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(repo)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure(&mut cmd);
        cmd.status().is_ok_and(|status| status.success())
    };
    if git(&["var", "GIT_COMMITTER_IDENT"]) {
        return;
    }
    git(&["config", "--local", "user.name", FALLBACK_NAME]);
    git(&["config", "--local", "user.email", FALLBACK_EMAIL]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A git repository and a `HOME` beside it, both empty.
    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("rusty-git-{name}-{}-{nanos}", std::process::id()));
        let (repo, home) = (root.join("repo"), root.join("home"));
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        (repo, home)
    }

    /// Git as a user who never set an identity: an empty `HOME`, no system config, no
    /// identity in the environment, and no guessing one from the host name.
    fn without_identity(home: &Path) -> impl Fn(&mut Command) + '_ {
        move |cmd: &mut Command| {
            for var in [
                "GIT_AUTHOR_NAME",
                "GIT_AUTHOR_EMAIL",
                "GIT_COMMITTER_NAME",
                "GIT_COMMITTER_EMAIL",
                "EMAIL",
            ] {
                cmd.env_remove(var);
            }
            cmd.env("HOME", home)
                .env("XDG_CONFIG_HOME", home.join(".config"))
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "user.useConfigOnly")
                .env("GIT_CONFIG_VALUE_0", "true");
        }
    }

    fn local(repo: &Path, key: &str) -> Option<String> {
        let out = Command::new("git")
            .args(["config", "--local", key])
            .current_dir(repo)
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    #[test]
    fn a_user_without_an_identity_gets_rustys_in_the_repository_only() {
        let (repo, home) = scratch("none");
        let configure = without_identity(&home);
        let mut probe = Command::new("git");
        probe
            .args(["var", "GIT_COMMITTER_IDENT"])
            .current_dir(&repo)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure(&mut probe);
        assert!(
            !probe.status().unwrap().success(),
            "the setup has no identity"
        );

        ensure_identity_with(&repo, &configure);
        assert_eq!(local(&repo, "user.name").as_deref(), Some(FALLBACK_NAME));
        assert_eq!(local(&repo, "user.email").as_deref(), Some(FALLBACK_EMAIL));
        assert!(probe.status().unwrap().success(), "git can commit now");
        assert!(
            !home.join(".gitconfig").exists(),
            "the global config is untouched"
        );
        let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    }

    #[test]
    fn a_user_with_an_identity_keeps_it() {
        let (repo, home) = scratch("some");
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = Someone\n\temail = someone@example.invalid\n",
        )
        .unwrap();
        ensure_identity_with(&repo, without_identity(&home));
        assert_eq!(local(&repo, "user.name"), None);
        assert_eq!(local(&repo, "user.email"), None);
        let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    }
}
