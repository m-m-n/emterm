//! chrono-free epoch → calendar decomposition.
//!
//! native-poc deliberately avoids a chrono dependency. This low-level
//! module is the single home for "Unix epoch seconds →
//! `(year, month, day, hour, minute, second)`", so neither
//! `crate::logging` (cross-cutting infrastructure) nor
//! `crate::status_bar` (a UI feature) has to reach into the other for
//! it. Both depend downward on this utility instead.

/// Local-time decomposition of `secs` (Unix epoch seconds) into
/// `(year, month, day, hour, minute, second)`. Uses `localtime_r` on
/// unix; falls back to [`utc_components`] when that call fails.
pub fn local_components(secs: i64) -> (u32, u32, u32, u32, u32, u32) {
    local_components_and_offset(secs).0
}

/// Like [`local_components`], but also returns the UTC offset (in
/// seconds, east-positive) that was applied — i.e. the invariant
/// `local == utc_components(secs + offset)` holds. Callers that
/// display the timestamp use this to make the zone explicit instead
/// of emitting a bare clock reading.
#[cfg(unix)]
pub fn local_components_and_offset(secs: i64) -> ((u32, u32, u32, u32, u32, u32), i32) {
    use std::mem::MaybeUninit;
    let t: libc::time_t = secs as libc::time_t;
    let mut tm_buf: MaybeUninit<libc::tm> = MaybeUninit::uninit();
    let res = unsafe { libc::localtime_r(&t, tm_buf.as_mut_ptr()) };
    if res.is_null() {
        return (utc_components(secs), 0);
    }
    let tm = unsafe { tm_buf.assume_init() };
    (
        (
            (tm.tm_year + 1900) as u32,
            (tm.tm_mon + 1) as u32,
            tm.tm_mday as u32,
            tm.tm_hour as u32,
            tm.tm_min as u32,
            tm.tm_sec as u32,
        ),
        tm.tm_gmtoff as i32,
    )
}

/// Non-unix path: no `localtime_r`, so decompose in UTC with a zero
/// offset. The explicit `+00:00` a caller renders from this keeps the
/// reading unambiguous even though it is not the user's wall clock.
#[cfg(not(unix))]
pub fn local_components_and_offset(secs: i64) -> ((u32, u32, u32, u32, u32, u32), i32) {
    (utc_components(secs), 0)
}

/// UTC date+time decomposition by integer arithmetic. Used as the
/// non-unix path and the fallback when `localtime_r` fails.
pub fn utc_components(secs: i64) -> (u32, u32, u32, u32, u32, u32) {
    let day = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400) as u32;
    let h = day_secs / 3600;
    let mi = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    let (y, mo, d) = days_to_ymd(day);
    (y, mo, d, h, mi, s)
}

/// Convert days-since-1970-01-01 to (year, month, day) using the
/// Howard Hinnant civil-from-days algorithm (public domain).
fn days_to_ymd(z: i64) -> (u32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_to_ymd_known_dates() {
        // 1970-01-01 → day 0
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        // 2000-01-01 → day 10957
        assert_eq!(days_to_ymd(10_957), (2000, 1, 1));
        // 2024-02-29 (leap day) → day 19782
        assert_eq!(days_to_ymd(19_782), (2024, 2, 29));
    }

    #[test]
    fn utc_components_for_known_epoch() {
        // 2026-01-01T00:00:00Z = 1767225600
        let (y, mo, d, h, mi, s) = utc_components(1_767_225_600);
        assert_eq!((y, mo, d, h, mi, s), (2026, 1, 1, 0, 0, 0));
    }

    /// `tm_gmtoff` is defined so that local time == UTC + offset. The
    /// test holds for any host timezone, so it does not need to pin TZ.
    #[test]
    fn local_offset_invariant_against_utc() {
        let secs = 1_767_225_600_i64; // 2026-01-01T00:00:00Z
        let (local, off) = local_components_and_offset(secs);
        assert_eq!(local, utc_components(secs + off as i64));
        // Real-world offsets fall within UTC-12:00 .. UTC+14:00.
        assert!((-12 * 3600..=14 * 3600).contains(&off));
    }
}
