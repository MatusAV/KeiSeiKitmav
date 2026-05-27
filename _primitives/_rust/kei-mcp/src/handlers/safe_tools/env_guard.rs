//! Subprocess environment + process-group hardening for kei_* tools.
//!
//! v0.46: extracted from monolithic safe_tools.rs.

use tokio::process::Command;

/// v0.41 fix #5: process-group helper (Unix-only; no-op on other platforms).
/// tokio::process::Command::process_group is available on Unix without
/// requiring the std::os::unix::process::CommandExt trait import.
#[cfg(unix)]
pub fn set_process_group(cmd: &mut Command) {
    cmd.process_group(0);
}
#[cfg(not(unix))]
pub fn set_process_group(_cmd: &mut Command) {}

/// v0.41 fix #5: SIGKILL the entire process group (negative pid).
#[cfg(unix)]
pub fn killpg_best_effort(pid: u32) {
    unsafe {
        let _ = libc::kill(-(pid as i32), libc::SIGKILL);
    }
}
#[cfg(not(unix))]
pub fn killpg_best_effort(_pid: u32) {}

/// v0.46 architectural fix: RAII guard. `kill_on_drop` only kills the
/// immediate child; backgrounded grandchildren survive (e.g. `bash -c
/// 'sleep 1000 &'`). v0.41 killpg fix only ran on the timeout error path.
/// Now: killpg fires on EVERY exit path (success, error, panic, early return)
/// via Drop. Caller disarms on clean wait_with_output success via `disarm()`.
pub struct KillPgGuard {
    pid: Option<u32>,
}

impl KillPgGuard {
    pub fn new(pid: Option<u32>) -> Self { Self { pid } }
    /// Caller succeeded cleanly; child is already reaped by wait_with_output.
    /// Skip the killpg fire on Drop.
    pub fn disarm(&mut self) { self.pid = None; }
}

impl Drop for KillPgGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            killpg_best_effort(pid);
        }
    }
}

/// v0.44 fix #4 (Gemini HIGH): strip parent env on subprocess spawn so secrets
/// like AWS_*, GITHUB_TOKEN, MOONSHOT_API_KEY etc. don't leak to user-controlled
/// bash commands or hook scripts. Whitelist forwards only PATH/HOME/USER/LANG/
/// TERM/SHELL/PWD/TMPDIR/LOGNAME/LC_* — enough to keep tools functional, none
/// of it sensitive.
///
/// Override: `KEI_SAFE_ENV_EXTRA=":-separated list"` adds named vars to the
/// whitelist for callers that legitimately need (e.g. NIX_PATH, JAVA_HOME).
pub fn apply_safe_env(cmd: &mut Command) {
    cmd.env_clear();
    let default_keep = [
        "PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL",
        "LC_CTYPE", "TERM", "PWD", "TMPDIR",
    ];
    for k in default_keep {
        if let Ok(v) = std::env::var(k) {
            cmd.env(k, v);
        }
    }
    if let Ok(extras) = std::env::var("KEI_SAFE_ENV_EXTRA") {
        for k in extras.split(':') {
            let k = k.trim();
            if k.is_empty() { continue; }
            if let Ok(v) = std::env::var(k) {
                cmd.env(k, v);
            }
        }
    }
}
