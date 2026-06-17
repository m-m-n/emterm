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
/// unix and `localtime_s` on Windows; falls back to [`utc_components`]
/// when the platform call fails.
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

/// Windows path: MSVCRT exposes `localtime_s` instead of POSIX
/// `localtime_r`, and `libc::tm` on Windows has no `tm_gmtoff` field.
/// We therefore derive the offset by treating the local components as
/// if they were UTC and subtracting the original epoch — the inverse
/// of [`utc_components`] is [`secs_from_utc_components`]. This keeps
/// the same `local == utc_components(secs + offset)` invariant the
/// unix path maintains.
#[cfg(windows)]
pub fn local_components_and_offset(secs: i64) -> ((u32, u32, u32, u32, u32, u32), i32) {
    use std::mem::MaybeUninit;
    let t: libc::time_t = secs as libc::time_t;
    let mut tm_buf: MaybeUninit<libc::tm> = MaybeUninit::uninit();
    // Windows signature: `localtime_s(tm*, const time_t*)` — note the
    // argument order is reversed compared to POSIX `localtime_r`.
    let res = unsafe { libc::localtime_s(tm_buf.as_mut_ptr(), &t) };
    if res != 0 {
        return (utc_components(secs), 0);
    }
    let tm = unsafe { tm_buf.assume_init() };
    let local = (
        (tm.tm_year + 1900) as u32,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
    );
    let local_as_utc =
        secs_from_utc_components(local.0, local.1, local.2, local.3, local.4, local.5);
    let offset = (local_as_utc - secs) as i32;
    (local, offset)
}

/// Other non-unix path: no platform local-time API available, so
/// decompose in UTC with a zero offset. The explicit `+00:00` a caller
/// renders from this keeps the reading unambiguous even though it is
/// not the user's wall clock.
#[cfg(not(any(unix, windows)))]
pub fn local_components_and_offset(secs: i64) -> ((u32, u32, u32, u32, u32, u32), i32) {
    (utc_components(secs), 0)
}

/// UTC date+time decomposition by integer arithmetic. Used as the
/// fallback when the platform local-time call (`localtime_r` on unix,
/// `localtime_s` on Windows) fails or is unavailable.
pub fn utc_components(secs: i64) -> (u32, u32, u32, u32, u32, u32) {
    let day = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400) as u32;
    let h = day_secs / 3600;
    let mi = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    let (y, mo, d) = days_to_ymd(day);
    (y, mo, d, h, mi, s)
}

/// Reverse of [`utc_components`]: turn UTC `(y, mo, d, h, mi, s)`
/// back into Unix epoch seconds. The Windows path uses this to derive
/// the UTC offset from `localtime_s` output, since `libc::tm` on
/// Windows lacks `tm_gmtoff`. Kept unconditional (rather than
/// `cfg(windows)`) so the inverse math can be round-trip tested on
/// the Linux CI host.
#[cfg_attr(not(windows), allow(dead_code))]
fn secs_from_utc_components(y: u32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> i64 {
    let days = ymd_to_days(y as i64, mo as i64, d as i64);
    days * 86_400 + h as i64 * 3600 + mi as i64 * 60 + s as i64
}

/// Inverse of [`days_to_ymd`]: Howard Hinnant `days_from_civil`
/// (public domain). Accepts a proleptic Gregorian `(year, month, day)`
/// and returns days since 1970-01-01.
#[cfg_attr(not(windows), allow(dead_code))]
fn ymd_to_days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
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

    /// `ymd_to_days` is the inverse of [`days_to_ymd`]. The Windows
    /// `local_components_and_offset` path relies on this to derive the
    /// UTC offset from `localtime_s` components, so verify the round
    /// trip on the (non-Windows) CI host as well.
    #[test]
    fn ymd_to_days_round_trips() {
        for &z in &[0_i64, 10_957, 19_782, -100_000, 100_000] {
            let (y, m, d) = days_to_ymd(z);
            assert_eq!(
                ymd_to_days(y as i64, m as i64, d as i64),
                z,
                "round trip failed for z={z}"
            );
        }
    }

    /// `secs_from_utc_components` is the inverse of [`utc_components`].
    /// Same rationale as `ymd_to_days_round_trips`.
    #[test]
    fn secs_from_utc_components_round_trips() {
        for &secs in &[0_i64, 1_767_225_600, 1_700_000_000, -86_400] {
            let (y, mo, d, h, mi, s) = utc_components(secs);
            assert_eq!(
                secs_from_utc_components(y, mo, d, h, mi, s),
                secs,
                "round trip failed for secs={secs}"
            );
        }
    }
}
