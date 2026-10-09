//! Scripts as commands (TICKET-010): a `*.sh` file inside a skill's directory is a
//! command, `rusty <name>`, resolved by its basename (`skill/name` when two skills share
//! one). Scripts live and are gated with their skills: a pending skill's script does not
//! run, the safety scan reads a script's text, and every write is committed by the caller
//! the way a skill's is.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{is_valid_skill_name, scan_skill_md, SkillsManager};

/// How long `run_script` waits before it kills the child.
pub const RUN_CAP: Duration = Duration::from_secs(60);
/// The most of each stream `run_script` keeps.
pub const MAX_OUTPUT: usize = 64 * 1024;

/// One script in the store.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Script {
    /// The command name: the file's basename without `.sh`.
    pub name: String,
    /// The skill directory it lives in.
    pub skill: String,
    /// The file.
    pub path: String,
    /// `active` or `pending` (the skill's status).
    pub status: String,
    /// Whether the file carries an execute bit.
    pub executable: bool,
}

/// What `run_script` hands back.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScriptRun {
    pub name: String,
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

fn header(name: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n# {name}: run with `rusty {name}`; edit it in Rusty's Skills tab or with\n# `rusty-cli scripts edit {name}`.\nset -euo pipefail\n\n"
    )
}

fn scripts_under(dir: &Path, status: &str) -> Vec<Script> {
    let mut out = Vec::new();
    let Ok(skills) = std::fs::read_dir(dir) else {
        return out;
    };
    for skill in skills.flatten() {
        let skill_dir = skill.path();
        if !skill_dir.is_dir() {
            continue;
        }
        let skill_name = skill.file_name().to_string_lossy().to_string();
        let Ok(files) = std::fs::read_dir(&skill_dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let is_sh = path.extension().is_some_and(|e| e == "sh") && path.is_file();
            if !is_sh {
                continue;
            }
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(&path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            };
            #[cfg(not(unix))]
            let executable = true;
            out.push(Script {
                name,
                skill: skill_name.clone(),
                path: path.to_string_lossy().to_string(),
                status: status.to_string(),
                executable,
            });
        }
    }
    out
}

fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    Ok(())
}

fn read_capped(mut stream: impl Read) -> String {
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    if bytes.len() > MAX_OUTPUT {
        bytes.truncate(MAX_OUTPUT);
        let mut text = String::from_utf8_lossy(&bytes).to_string();
        text.push_str("\n… (cut)\n");
        return text;
    }
    String::from_utf8_lossy(&bytes).to_string()
}

impl SkillsManager {
    /// Every script in the active skills, and in the pending ones when asked.
    pub fn scripts(&self, include_pending: bool) -> Vec<Script> {
        let mut out = scripts_under(&self.active_dir(), "active");
        if include_pending {
            out.extend(scripts_under(&self.staging_dir(), "pending"));
        }
        out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.skill.cmp(&b.skill)));
        out
    }

    /// The script named `name` or `skill/name`; a basename in two skills is an error
    /// that names both.
    pub fn resolve_script(&self, name: &str) -> Result<Script, String> {
        let name = name.trim();
        let name = name.strip_suffix(".sh").unwrap_or(name);
        let (skill, base) = match name.split_once('/') {
            Some((s, b)) => (Some(s), b),
            None => (None, name),
        };
        if base.is_empty() || !is_valid_skill_name(base) {
            return Err(format!("not a script name: {name:?}"));
        }
        let all = self.scripts(true);
        let matches: Vec<&Script> = all
            .iter()
            .filter(|s| s.name == base && skill.is_none_or(|k| s.skill == k))
            .collect();
        match matches.len() {
            0 => Err(format!("no script {name:?} in the store")),
            1 => Ok(matches[0].clone()),
            _ => Err(format!(
                "{base:?} is in more than one skill ({}); name it as skill/{base}",
                matches
                    .iter()
                    .map(|s| format!("{}/{}", s.skill, s.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// The script and its text.
    pub fn script_text(&self, name: &str) -> Result<(Script, String), String> {
        let script = self.resolve_script(name)?;
        let text = std::fs::read_to_string(&script.path)
            .map_err(|e| format!("read {}: {e}", script.path))?;
        Ok((script, text))
    }

    /// Create `<skill>/<name>.sh` (mode 0755) with `body` or a header; a missing skill
    /// is created, named after the script when none is given. Does not commit.
    pub fn create_script(
        &self,
        name: &str,
        skill: Option<&str>,
        body: Option<&str>,
        force: bool,
    ) -> Result<Script, String> {
        let name = name.trim();
        if !is_valid_skill_name(name) {
            return Err(format!(
                "invalid script name {name:?}: use lowercase letters, digits, and hyphens"
            ));
        }
        let skill_name = skill
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(name);
        if self.get(skill_name).is_none() {
            self.create_skill(
                skill_name,
                &format!("Runs {name}.sh: `rusty {name}`."),
                &format!(
                    "## Procedure\n\nRun `rusty {name}`; the script is `{name}.sh` beside this file.\n"
                ),
                false,
            )?;
        }
        let skill_md = self
            .get(skill_name)
            .map(|s| PathBuf::from(s.path))
            .ok_or_else(|| format!("skill {skill_name:?} not found"))?;
        let dir = skill_md
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("no directory for {}", skill_md.display()))?;
        let path = dir.join(format!("{name}.sh"));
        if path.exists() && !force {
            return Err(format!(
                "script {} already exists (use --force to overwrite)",
                path.display()
            ));
        }
        let content = match body {
            Some(b) if !b.trim().is_empty() => b.to_string(),
            _ => format!("{}echo \"{name}: nothing here yet\"\n", header(name)),
        };
        std::fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
        set_executable(&path)?;
        let script = self.resolve_script(&format!("{skill_name}/{name}"))?;
        self.record("script", &format!("{skill_name}/{name}"), "created");
        Ok(script)
    }

    /// Replace a script's text; the execute bit is kept. Does not commit.
    pub fn update_script(&self, name: &str, body: &str) -> Result<Script, String> {
        let script = self.resolve_script(name)?;
        std::fs::write(&script.path, body).map_err(|e| format!("write {}: {e}", script.path))?;
        set_executable(Path::new(&script.path))?;
        self.record(
            "script",
            &format!("{}/{}", script.skill, script.name),
            "updated",
        );
        Ok(script)
    }

    /// Remove a script's file; the skill stays. Does not commit.
    pub fn delete_script(&self, name: &str) -> Result<Script, String> {
        let script = self.resolve_script(name)?;
        std::fs::remove_file(&script.path).map_err(|e| format!("remove {}: {e}", script.path))?;
        self.record(
            "script",
            &format!("{}/{}", script.skill, script.name),
            "deleted",
        );
        Ok(script)
    }

    /// The safety scan over a script's text.
    pub fn scan_script(&self, name: &str) -> Result<Vec<String>, String> {
        let (_, text) = self.script_text(name)?;
        Ok(scan_skill_md(&text))
    }

    /// Run an active script with `args`, both streams captured, killed at `cap`.
    pub fn run_script(
        &self,
        name: &str,
        args: &[String],
        cap: Duration,
    ) -> Result<ScriptRun, String> {
        let script = self.resolve_script(name)?;
        if script.status != "active" {
            return Err(format!(
                "{} is pending; approve the skill {:?} first",
                script.name, script.skill
            ));
        }
        let mut child = Command::new("bash")
            .arg(&script.path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("start {}: {e}", script.path))?;
        let out = child
            .stdout
            .take()
            .map(|s| std::thread::spawn(move || read_capped(s)));
        let err = child
            .stderr
            .take()
            .map(|s| std::thread::spawn(move || read_capped(s)));
        let started = Instant::now();
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code().unwrap_or(-1),
                Ok(None) if started.elapsed() >= cap => {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break 124;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => return Err(format!("wait for {}: {e}", script.path)),
            }
        };
        let stdout = out.and_then(|t| t.join().ok()).unwrap_or_default();
        let stderr = err.and_then(|t| t.join().ok()).unwrap_or_default();
        Ok(ScriptRun {
            name: script.name,
            status,
            stdout,
            stderr,
            timed_out,
        })
    }

    /// Replace the current process with an active script; returns only on failure.
    #[cfg(unix)]
    pub fn exec_script(&self, name: &str, args: &[String]) -> Result<(), String> {
        use std::os::unix::process::CommandExt;
        let script = self.resolve_script(name)?;
        if script.status != "active" {
            return Err(format!(
                "{} is pending; approve the skill {:?} first",
                script.name, script.skill
            ));
        }
        let err = Command::new("bash").arg(&script.path).args(args).exec();
        Err(format!("exec {}: {err}", script.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (PathBuf, SkillsManager) {
        let dir = std::env::temp_dir().join(format!("rusty_scripts_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mgr = SkillsManager::new(dir.clone());
        mgr.ensure_dirs().unwrap();
        (dir, mgr)
    }

    #[test]
    fn scripts_resolve_by_basename_and_disambiguate() {
        let (dir, mgr) = store("resolve");
        let s = mgr
            .create_script(
                "usb-reset",
                Some("dev-box-usb"),
                Some("#!/usr/bin/env bash\necho hi \"$1\"\nexit 3\n"),
                false,
            )
            .unwrap();
        assert_eq!(
            (
                s.name.as_str(),
                s.skill.as_str(),
                s.status.as_str(),
                s.executable
            ),
            ("usb-reset", "dev-box-usb", "active", true)
        );
        assert!(mgr.get("dev-box-usb").is_some(), "the skill was created");
        assert_eq!(mgr.scripts(true).len(), 1);
        assert_eq!(mgr.resolve_script("usb-reset").unwrap().path, s.path);
        assert_eq!(mgr.resolve_script("usb-reset.sh").unwrap().path, s.path);
        mgr.create_script("usb-reset", Some("mac-mini"), None, false)
            .unwrap();
        let err = mgr.resolve_script("usb-reset").unwrap_err();
        assert!(
            err.contains("dev-box-usb/usb-reset") && err.contains("mac-mini/usb-reset"),
            "{err}"
        );
        assert_eq!(
            mgr.resolve_script("mac-mini/usb-reset").unwrap().skill,
            "mac-mini"
        );
        assert!(mgr.resolve_script("nope").is_err());
        assert!(mgr.resolve_script("Bad Name").is_err());
        assert!(
            mgr.create_script("usb-reset", Some("mac-mini"), None, false)
                .is_err(),
            "exists without force"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_script_runs_with_arguments_and_its_status_and_a_slow_one_is_cut() {
        let (dir, mgr) = store("run");
        mgr.create_script(
            "usb-reset",
            Some("dev-box-usb"),
            Some("#!/usr/bin/env bash\necho hi \"$1\"\necho oops >&2\nexit 3\n"),
            false,
        )
        .unwrap();
        let run = mgr
            .run_script("usb-reset", &["there".to_string()], RUN_CAP)
            .unwrap();
        assert_eq!(
            (
                run.status,
                run.stdout.as_str(),
                run.stderr.as_str(),
                run.timed_out
            ),
            (3, "hi there\n", "oops\n", false)
        );
        mgr.create_script(
            "slow",
            Some("dev-box-usb"),
            Some("#!/usr/bin/env bash\nsleep 5\n"),
            false,
        )
        .unwrap();
        let cut = mgr
            .run_script("slow", &[], Duration::from_millis(200))
            .unwrap();
        assert!(cut.timed_out && cut.status == 124, "{cut:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_pending_script_does_not_run() {
        let (dir, mgr) = store("pending");
        mgr.create_pending_skill("net-fix", "Fix the network.", "## Procedure\n")
            .unwrap();
        let path = mgr.staging_dir().join("net-fix").join("reset.sh");
        std::fs::write(&path, "#!/usr/bin/env bash\necho reset\n").unwrap();
        set_executable(&path).unwrap();
        let s = mgr.resolve_script("reset").unwrap();
        assert_eq!(s.status, "pending");
        let err = mgr.run_script("reset", &[], RUN_CAP).unwrap_err();
        assert!(err.contains("pending"), "{err}");
        mgr.approve("net-fix", false).unwrap();
        let run = mgr.run_script("reset", &[], RUN_CAP).unwrap();
        assert_eq!((run.status, run.stdout.as_str()), (0, "reset\n"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn update_scan_and_delete_a_script() {
        let (dir, mgr) = store("update");
        mgr.create_script("tidy", None, None, false).unwrap();
        assert!(
            mgr.get("tidy").is_some(),
            "a script without a skill gets one"
        );
        let (_, text) = mgr.script_text("tidy").unwrap();
        assert!(
            text.starts_with("#!/usr/bin/env bash") && text.contains("rusty tidy"),
            "{text}"
        );
        assert!(mgr.scan_script("tidy").unwrap().is_empty());
        mgr.update_script("tidy", "#!/usr/bin/env bash\necho !`whoami`\n")
            .unwrap();
        let findings = mgr.scan_script("tidy").unwrap();
        assert!(!findings.is_empty(), "the scan reads a script");
        let removed = mgr.delete_script("tidy").unwrap();
        assert!(!Path::new(&removed.path).exists());
        assert!(mgr.resolve_script("tidy").is_err());
        assert!(mgr.get("tidy").is_some(), "the skill stays");
        let _ = std::fs::remove_dir_all(dir);
    }
}
