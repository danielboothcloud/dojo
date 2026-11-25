//! Opt-in subprocess call logging.
//!
//! Set `DOJO_LOG=1` to log to `.dojo/subprocess.log`, or `DOJO_LOG=<path>`
//! for a custom location. No log rotation — delete the file manually.

use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// Holds the open log file, or `None` if logging is disabled or init failed.
static LOG: OnceLock<Option<Mutex<BufWriter<std::fs::File>>>> = OnceLock::new();

/// Initialize subprocess logging based on the `DOJO_LOG` environment variable.
///
/// - `DOJO_LOG=1` → log to `repo_root/.dojo/subprocess.log`
/// - `DOJO_LOG=<path>` → log to that path
/// - Unset or empty → no logging
pub fn init(repo_root: &Path) {
    let val = match std::env::var("DOJO_LOG") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            let _ = LOG.set(None);
            return;
        }
    };

    let path = if val == "1" {
        if let Err(e) = crate::jj_utils::ensure_dojo_dir(repo_root) {
            eprintln!("(dojo)::log: failed to create .dojo directory: {e:#}");
            let _ = LOG.set(None);
            return;
        }
        repo_root.join(".dojo").join("subprocess.log")
    } else {
        std::path::PathBuf::from(val)
    };

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => {
            let _ = LOG.set(Some(Mutex::new(BufWriter::new(file))));
        }
        Err(e) => {
            eprintln!("(dojo)::log: cannot open {}: {e}", path.display());
            let _ = LOG.set(None);
        }
    }
}

/// Log a jj subprocess call. No-op if logging is not enabled.
pub fn log_subprocess(label: &str, duration: Duration) {
    let Some(Some(mtx)) = LOG.get() else {
        return;
    };
    let ts = rfc3339_millis(SystemTime::now());
    let ms = duration.as_millis();
    let line = format!("{ts}  {ms:>6}ms  jj {label}\n");
    if let Ok(mut w) = mtx.lock() {
        let _ = w.write_all(line.as_bytes());
        let _ = w.flush();
    }
}

/// Log a hook subprocess call. No-op if logging is not enabled.
pub fn log_hook(hook_label: &str, cmd: &str, duration: Duration) {
    let Some(Some(mtx)) = LOG.get() else {
        return;
    };
    let ts = rfc3339_millis(SystemTime::now());
    let ms = duration.as_millis();
    let line = format!("{ts}  {ms:>6}ms  hook:{hook_label}  {cmd}\n");
    if let Ok(mut w) = mtx.lock() {
        let _ = w.write_all(line.as_bytes());
        let _ = w.flush();
    }
}

/// UTC time as RFC 3339 with milliseconds, for example
/// `2026-11-25T16:08:24.123Z`. (Replaces `humantime`, which is unmaintained:
/// RUSTSEC-2026-0542.)
fn rfc3339_millis(t: SystemTime) -> String {
    let d = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let secs = d.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, day) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian
/// calendar (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_millis_formats_utc() {
        let at = |s: u64, ms: u64| {
            rfc3339_millis(SystemTime::UNIX_EPOCH + Duration::from_millis(s * 1000 + ms))
        };
        assert_eq!(at(0, 0), "1970-01-01T00:00:00.000Z");
        // 2000-02-29 (leap day) 12:34:56.789
        assert_eq!(at(951_827_696, 789), "2000-02-29T12:34:56.789Z");
        // 2026-11-25 16:08:24
        assert_eq!(at(1_795_622_904, 5), "2026-11-25T16:08:24.005Z");
        assert_eq!(at(4_102_444_799, 999), "2099-12-31T23:59:59.999Z");
    }
}
