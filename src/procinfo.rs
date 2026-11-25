//! Minimal process-tree lookups used by active-shell detection.
//!
//! - macOS: `libproc` (`proc_pidinfo` / `proc_pidpath`).
//! - Linux: `/proc/<pid>/stat` (parent pid) and `/proc/<pid>/exe` (path).
//! - Other platforms: no process tree; callers fall back to `$SHELL`.

/// Parent pid of `pid`. `ppid <= 1` (init / launchd / reparented) is `None`,
/// which stops a tree walk.
pub(crate) fn parent_pid(pid: i32) -> Option<i32> {
    imp::parent_pid(pid).filter(|&ppid| ppid > 1)
}

/// Full executable path of `pid`, or `None` when it cannot be resolved.
pub(crate) fn exe_path(pid: i32) -> Option<String> {
    imp::exe_path(pid)
}

#[cfg(target_os = "macos")]
mod imp {
    use libproc::bsd_info::BSDInfo;
    use libproc::proc_pid::{pidinfo, pidpath};

    pub(super) fn parent_pid(pid: i32) -> Option<i32> {
        pidinfo::<BSDInfo>(pid, 0)
            .ok()
            .map(|info| info.pbi_ppid as i32)
    }

    pub(super) fn exe_path(pid: i32) -> Option<String> {
        pidpath(pid).ok()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub(super) fn parent_pid(pid: i32) -> Option<i32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        super::parse_stat_ppid(&stat)
    }

    pub(super) fn exe_path(pid: i32) -> Option<String> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod imp {
    pub(super) fn parent_pid(_pid: i32) -> Option<i32> {
        None
    }

    pub(super) fn exe_path(_pid: i32) -> Option<String> {
        None
    }
}

/// Parse the parent pid from a `/proc/<pid>/stat` line:
/// `pid (comm) state ppid ...`. `comm` may contain spaces and `)`, so the
/// fields are read after the LAST `)`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_stat_ppid(stat: &str) -> Option<i32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_ppid_handles_spaces_and_parens_in_comm() {
        assert_eq!(parse_stat_ppid("42 (bash) S 7 42 42 0"), Some(7));
        assert_eq!(parse_stat_ppid("42 (my (odd) cmd) R 99 1 1"), Some(99));
        assert_eq!(parse_stat_ppid("garbage"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_lookup_finds_own_parent_and_exe() {
        let me = std::process::id() as i32;
        assert!(imp::parent_pid(me).is_some());
        assert!(exe_path(me).is_some());
    }
}
