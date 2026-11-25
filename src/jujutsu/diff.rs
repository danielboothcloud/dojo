//! A change's diff as data: files, hunks, and lines with line numbers, for
//! the TUI change detail.
//!
//! The source is `jj diff --git`. This parses git's unified diff format (a
//! stable, documented format, not jj's human output), like the Sundae
//! refusal parser is a documented exception to "parse templates only".

use anyhow::{Context, Result};
use std::path::Path;

use super::run_jj_query;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    /// `\ No newline at end of file`
    NoNewline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
    /// Byte range of `text` that changed against the paired line (a
    /// removed line and the added line that replaces it).
    pub emph: Option<(usize, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub new_start: u32,
    /// Text after the second `@@` (often the enclosing function).
    pub context: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    /// Source path of a rename or copy.
    pub old_path: Option<String>,
    /// `A` added, `D` deleted, `M` modified, `R` renamed, `C` copied.
    pub status: char,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    pub added: usize,
    pub removed: usize,
}

/// The diff of one revision, one entry per file.
pub fn file_diffs(repo: &Path, rev: &str) -> Result<Vec<FileDiff>> {
    let out = run_jj_query(repo, &["diff", "--git", "--revision", rev]).context("jj diff")?;
    Ok(parse(&out))
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    match s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => s.to_string(),
    }
}

/// Paths from `diff --git a/X b/Y`.
fn header_paths(rest: &str) -> (String, String) {
    let rest = rest.trim();
    // Same path on both sides (the common case, also with spaces).
    if rest.len() % 2 == 1 {
        let mid = rest.len() / 2;
        if rest.is_char_boundary(mid)
            && let (Some(a), Some(b)) = (
                rest[..mid].strip_prefix("a/"),
                rest[mid + 1..].strip_prefix("b/"),
            )
            && a == b
        {
            return (a.to_string(), b.to_string());
        }
    }
    match rest.split_once(" b/") {
        Some((a, b)) => (unquote(a.strip_prefix("a/").unwrap_or(a)), unquote(b)),
        None => (unquote(rest), unquote(rest)),
    }
}

/// `-a,b +c,d` of a hunk header: (old start, new start, context text).
fn hunk_header(line: &str) -> Option<(u32, u32, String)> {
    let rest = line.strip_prefix("@@ ")?;
    let (ranges, context) = rest.split_once(" @@").unwrap_or((rest, ""));
    let mut parts = ranges.split_whitespace();
    let start = |p: Option<&str>, sign: char| -> Option<u32> {
        p?.strip_prefix(sign)?.split(',').next()?.parse().ok()
    };
    let old = start(parts.next(), '-')?;
    let new = start(parts.next(), '+')?;
    Some((old, new, context.trim_start().to_string()))
}

pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut old_no = 0;
    let mut new_no = 0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let (a, b) = header_paths(rest);
            files.push(FileDiff {
                old_path: (a != b).then_some(a),
                path: b,
                status: 'M',
                binary: false,
                hunks: Vec::new(),
                added: 0,
                removed: 0,
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(hunk) = file.hunks.last_mut() {
            let (kind, body) = match line.chars().next() {
                Some('+') => (LineKind::Added, &line[1..]),
                Some('-') => (LineKind::Removed, &line[1..]),
                Some(' ') => (LineKind::Context, &line[1..]),
                Some('\\') => (LineKind::NoNewline, line),
                None => (LineKind::Context, ""),
                _ => (LineKind::Context, line),
            };
            if !line.starts_with("@@ ") {
                let (o, n) = match kind {
                    LineKind::Added => {
                        new_no += 1;
                        file.added += 1;
                        (None, Some(new_no - 1))
                    }
                    LineKind::Removed => {
                        old_no += 1;
                        file.removed += 1;
                        (Some(old_no - 1), None)
                    }
                    LineKind::Context => {
                        old_no += 1;
                        new_no += 1;
                        (Some(old_no - 1), Some(new_no - 1))
                    }
                    LineKind::NoNewline => (None, None),
                };
                hunk.lines.push(DiffLine {
                    kind,
                    old_no: o,
                    new_no: n,
                    text: body.to_string(),
                    emph: None,
                });
                continue;
            }
        }
        if let Some((o, n, context)) = hunk_header(line) {
            old_no = o;
            new_no = n;
            file.hunks.push(Hunk {
                old_start: o,
                new_start: n,
                context,
                lines: Vec::new(),
            });
        } else if line.starts_with("new file mode") {
            file.status = 'A';
        } else if line.starts_with("deleted file mode") {
            file.status = 'D';
        } else if let Some(p) = line.strip_prefix("rename from ") {
            file.status = 'R';
            file.old_path = Some(unquote(p));
        } else if let Some(p) = line.strip_prefix("rename to ") {
            file.path = unquote(p);
        } else if let Some(p) = line.strip_prefix("copy from ") {
            file.status = 'C';
            file.old_path = Some(unquote(p));
        } else if let Some(p) = line.strip_prefix("copy to ") {
            file.path = unquote(p);
        } else if line.starts_with("Binary files ") {
            file.binary = true;
        }
    }
    for f in &mut files {
        if !matches!(f.status, 'R' | 'C') {
            f.old_path = None;
        }
        for h in &mut f.hunks {
            mark_changed_words(&mut h.lines);
        }
    }
    files
}

/// For each run of removed lines followed by the same number of added
/// lines, mark the part of each line pair that differs (the common prefix
/// and suffix stay plain).
pub fn mark_changed_words(lines: &mut [DiffLine]) {
    let mut i = 0;
    while i < lines.len() {
        let start = i;
        while i < lines.len() && lines[i].kind == LineKind::Removed {
            i += 1;
        }
        let removed = i - start;
        let add_start = i;
        while i < lines.len() && lines[i].kind == LineKind::Added {
            i += 1;
        }
        let added = i - add_start;
        if removed == 0 && added == 0 {
            i += 1;
            continue;
        }
        if removed == added {
            for k in 0..removed {
                let (old, new) = (&lines[start + k].text, &lines[add_start + k].text);
                if let Some((o, n)) = changed_ranges(old, new) {
                    lines[start + k].emph = Some(o);
                    lines[add_start + k].emph = Some(n);
                }
            }
        }
    }
}

/// Byte ranges of the changed middle part of two lines, or `None` when the
/// lines have nothing in common (then the whole line is the change).
fn changed_ranges(old: &str, new: &str) -> Option<((usize, usize), (usize, usize))> {
    let prefix: usize = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let suffix: usize = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    if prefix + suffix == 0 {
        return None;
    }
    Some(((prefix, old.len() - suffix), (prefix, new.len() - suffix)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
diff --git a/b.txt b/b.txt
deleted file mode 100644
index 6178079822..0000000000
--- a/b.txt
+++ /dev/null
@@ -1,1 +0,0 @@ b
-b
diff --git a/bin.dat b/bin.dat
new file mode 100644
index 0000000000..bdc955b7b2
Binary files /dev/null and b/bin.dat differ
diff --git a/goals.txt b/goals2.txt
rename from goals.txt
rename to goals2.txt
diff --git a/sp ace.txt b/sp ace.txt
index 1111111111..2222222222 100644
--- a/sp ace.txt
+++ b/sp ace.txt
@@ -10,4 +10,4 @@ fn main() {
 keep
-let x = 1;
+let x = 2;
 end
\\ No newline at end of file
";

    #[test]
    fn parses_files_status_and_line_numbers() {
        let files = parse(SAMPLE);
        let summary: Vec<(char, &str, bool)> = files
            .iter()
            .map(|f| (f.status, f.path.as_str(), f.binary))
            .collect();
        assert_eq!(
            summary,
            vec![
                ('D', "b.txt", false),
                ('A', "bin.dat", true),
                ('R', "goals2.txt", false),
                ('M', "sp ace.txt", false),
            ]
        );
        assert_eq!(files[2].old_path.as_deref(), Some("goals.txt"));
        assert_eq!(files[0].removed, 1);
        let f = &files[3];
        assert_eq!((f.added, f.removed), (1, 1));
        let h = &f.hunks[0];
        assert_eq!(h.context, "fn main() {");
        let nums: Vec<(Option<u32>, Option<u32>)> =
            h.lines.iter().map(|l| (l.old_no, l.new_no)).collect();
        assert_eq!(
            nums,
            vec![
                (Some(10), Some(10)),
                (Some(11), None),
                (None, Some(11)),
                (Some(12), Some(12)),
                (None, None),
            ]
        );
        assert_eq!(h.lines[4].kind, LineKind::NoNewline);
    }

    #[test]
    fn marks_the_changed_part_of_a_line_pair() {
        let files = parse(SAMPLE);
        let lines = &files[3].hunks[0].lines;
        // "let x = 1;" -> "let x = 2;": only the digit differs.
        assert_eq!(lines[1].emph, Some((8, 9)));
        assert_eq!(lines[2].emph, Some((8, 9)));
        assert_eq!(changed_ranges("abc", "xyz"), None);
    }
}
