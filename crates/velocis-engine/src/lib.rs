//! `velocis_engine` — the editing engine.
//!
//! Layering (each layer only depends on the ones below it):
//!
//! ```text
//! ops      timeline editing commands (split/trim/move/...) — pure, undoable
//! render   Project -> ffmpeg filter graph (a value, inspectable, unit-testable)
//! export   runs the graph, reports progress, writes the file
//! probe    media introspection (ffprobe) + derivative generation (thumbnails, peaks)
//! model    domain types, no I/O
//! ```
//!
//! The engine never links against libav*: it drives `ffmpeg`/`ffprobe` as
//! subprocesses. That keeps the build simple and portable, lets the OS/browser
//! pick hardware acceleration through the normal ffmpeg device selection, and
//! makes the render step trivially reproducible from a project file alone.

pub mod export;
pub mod fonts;
pub mod model;
pub mod ops;
pub mod probe;
pub mod render;
pub mod thumbs;

pub use model::*;

/// RFC 3339 UTC timestamp without pulling in a date library.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_rfc3339(secs)
}

pub fn format_rfc3339(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let rem = unix_secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 -> (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_format_correctly() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_000_000_000), "2001-09-09T01:46:40Z");
        // Leap day.
        assert_eq!(format_rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn micros_round_trip_through_seconds() {
        let t: Micros = 3_500_000;
        assert_eq!(t / ONE_SECOND, 3);
    }
}
