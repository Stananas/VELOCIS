//! Font resolution.
//!
//! `drawtext` in many ffmpeg builds has no `bold`/`italic` options, and its
//! `font=` value cannot carry a fontconfig pattern (the `:` in
//! `DejaVu Sans:bold` is parsed as an option separator by the filtergraph
//! parser). The portable answer is to resolve the family *and style* to a real
//! font file with `fc-match`, then pass `fontfile=`.
//!
//! Resolution is cached because `fc-match` is a subprocess and the timeline asks
//! for the same font on every repaint.

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve a family + style to an absolute font file path, if the host can.
///
/// Returns `None` when fontconfig is unavailable (typical on Windows), in which
/// case callers should fall back to passing the family name to `drawtext`.
pub fn resolve(family: &str, bold: bool, italic: bool) -> Option<String> {
    let family = family.trim();
    if family.is_empty() {
        return None;
    }
    let mut pattern = family.to_string();
    match (bold, italic) {
        (true, true) => pattern.push_str(":bold:italic"),
        (true, false) => pattern.push_str(":bold"),
        (false, true) => pattern.push_str(":italic"),
        (false, false) => {}
    }

    if let Ok(guard) = cache().lock() {
        if let Some(hit) = guard.get(&pattern) {
            return hit.clone();
        }
    }

    let resolved = run_fc_match(&pattern);
    if let Ok(mut guard) = cache().lock() {
        guard.insert(pattern, resolved.clone());
    }
    resolved
}

fn run_fc_match(pattern: &str) -> Option<String> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() || !std::path::Path::new(&path).is_file() {
        return None;
    }
    Some(path)
}

/// Does this ffmpeg build's `drawtext` accept `bold`/`italic`?
pub fn drawtext_has_style_options() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let Ok(output) = Command::new(crate::probe::ffmpeg_bin())
            .args(["-hide_banner", "-h", "filter=drawtext"])
            .output()
        else {
            return false;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("bold ") || trimmed.starts_with("italic ")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_family_never_resolves() {
        assert!(resolve("", false, false).is_none());
        assert!(resolve("   ", true, false).is_none());
    }

    #[test]
    fn the_probe_is_cached_and_consistent() {
        assert_eq!(drawtext_has_style_options(), drawtext_has_style_options());
    }
}
