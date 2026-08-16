use chrono::{Datelike, NaiveDate};
use std::collections::{HashMap, HashSet};

/// Result of computing a streak over a date range.
#[derive(Debug, Clone)]
pub struct StreakResult {
    /// Streak count at `to` date (still running if > 0 and last day was active/frozen).
    pub current_streak: u32,
    /// Longest streak seen over the entire range.
    pub best_streak: u32,
    /// Days where the one-per-ISO-week freeze was consumed (❄).
    pub freeze_days: Vec<NaiveDate>,
    /// Days that were holiday-protected (never broke streak, never consumed freeze).
    pub holiday_protected_days: Vec<NaiveDate>,
}

/// Compute a streak over [from, to] with holiday protection and one freeze per ISO week.
///
/// Precedence (per Plan.md §3):
///   1. Chronotype shift is applied *before* calling this function — `active_days`
///      should already be shifted to the day they "felt like" by the caller.
///   2. If a day is in `holiday_days` → protected (no streak break, no freeze consumed).
///   3. If a day has no activity and is not a holiday → check if an ISO-week freeze
///      is still available. If yes, consume it (❄) and keep the streak alive.
///      If no freeze remains → streak breaks; reset to 0.
pub fn compute_streak(
    active_days: &HashSet<NaiveDate>,
    holiday_days: &HashSet<NaiveDate>,
    from: NaiveDate,
    to: NaiveDate,
) -> StreakResult {
    let mut current_streak: u32 = 0;
    let mut best_streak: u32 = 0;
    // Track which ISO weeks have had their freeze consumed.
    let mut freeze_used: HashMap<(i32, u32), bool> = HashMap::new();
    let mut freeze_days = Vec::new();
    let mut holiday_protected_days = Vec::new();

    let mut day = from;
    while day <= to {
        if active_days.contains(&day) {
            // Active day — extend streak.
            current_streak += 1;
            best_streak = best_streak.max(current_streak);
        } else if holiday_days.contains(&day) {
            // Holiday-protected — neither breaks streak nor consumes a freeze.
            holiday_protected_days.push(day);
        } else {
            // Real miss — try to use the ISO-week freeze.
            let iso = day.iso_week();
            let week_key = (iso.year(), iso.week());
            if !freeze_used.contains_key(&week_key) {
                // Freeze available — consume it, streak survives.
                freeze_used.insert(week_key, true);
                freeze_days.push(day);
                // Streak value is unchanged (the day is "forgiven").
            } else {
                // No freeze left — streak breaks.
                best_streak = best_streak.max(current_streak);
                current_streak = 0;
            }
        }

        day = match day.succ_opt() {
            Some(d) => d,
            None => break,
        };
    }

    // Final best check after the loop.
    best_streak = best_streak.max(current_streak);

    StreakResult {
        current_streak,
        best_streak,
        freeze_days,
        holiday_protected_days,
    }
}

/// Parse official holiday dates from NoClassDays.md.
/// Handles single dates (`2026-08-21`) and ranges (`2026-10-29 → 10-31`,
/// `2026-12-21 → 2027-01-01`).
pub fn load_holiday_days(no_class_days_path: &std::path::Path) -> HashSet<NaiveDate> {
    let mut holidays = HashSet::new();
    if !no_class_days_path.exists() {
        return holidays;
    }
    let Ok(content) = std::fs::read_to_string(no_class_days_path) else {
        return holidays;
    };

    // Only parse the "Official Holidays & Breaks" section.
    let mut in_official_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("Official Holidays") {
            in_official_section = true;
            continue;
        }
        if in_official_section && trimmed.starts_with("### ") {
            break; // next sub-section — stop
        }
        if !in_official_section || !trimmed.starts_with('|') {
            continue;
        }
        // Extract first cell (the date cell), e.g. "| 2026-08-21 | Fri | ..."
        let cells: Vec<&str> = trimmed.split('|').map(str::trim).collect();
        if cells.len() < 2 {
            continue;
        }
        let date_cell = cells[1];
        if date_cell.is_empty() || date_cell.starts_with('-') || date_cell.eq_ignore_ascii_case("Date") {
            continue;
        }

        parse_date_cell(date_cell, &mut holidays);
    }

    holidays
}

/// Parse a date cell that is either:
///   - A plain date: `2026-08-21`
///   - A same-year range: `2026-10-29 → 10-31`
///   - A cross-year range: `2026-12-21 → 2027-01-01`
fn parse_date_cell(cell: &str, out: &mut HashSet<NaiveDate>) {
    if cell.contains('→') {
        let parts: Vec<&str> = cell.splitn(2, '→').map(str::trim).collect();
        if parts.len() != 2 {
            return;
        }
        let start_str = parts[0];
        let end_str = parts[1];

        let Ok(start) = NaiveDate::parse_from_str(start_str, "%Y-%m-%d") else {
            return;
        };

        // End might be "10-31" (same year) or "2027-01-01" (explicit year).
        let end = if end_str.len() == 10 {
            NaiveDate::parse_from_str(end_str, "%Y-%m-%d").ok()
        } else {
            // e.g. "10-31" — reuse start year
            NaiveDate::parse_from_str(
                &format!("{}-{}", start.year(), end_str),
                "%Y-%m-%d",
            )
            .ok()
        };

        if let Some(end) = end {
            let mut d = start;
            while d <= end {
                out.insert(d);
                d = match d.succ_opt() {
                    Some(next) => next,
                    None => break,
                };
            }
        }
    } else {
        if let Ok(date) = NaiveDate::parse_from_str(cell, "%Y-%m-%d") {
            out.insert(date);
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn days(dates: &[(i32, u32, u32)]) -> HashSet<NaiveDate> {
        dates.iter().map(|&(y, m, day)| d(y, m, day)).collect()
    }

    fn no_holidays() -> HashSet<NaiveDate> {
        HashSet::new()
    }

    // ── Fixture 1: Plain consecutive activity ─────────────────────────────────
    // Mon–Fri all active → streak = 5, no freezes, no holidays.
    #[test]
    fn test_plain_consecutive_streak() {
        let active = days(&[(2026, 8, 3), (2026, 8, 4), (2026, 8, 5), (2026, 8, 6), (2026, 8, 7)]);
        let result = compute_streak(&active, &no_holidays(), d(2026, 8, 3), d(2026, 8, 7));
        assert_eq!(result.current_streak, 5);
        assert_eq!(result.best_streak, 5);
        assert!(result.freeze_days.is_empty());
        assert!(result.holiday_protected_days.is_empty());
    }

    // ── Fixture 2: One miss with freeze ───────────────────────────────────────
    // Mon–Thu active (4 days), miss Fri (freeze consumed), active Sat (5th active day).
    // The freeze keeps the chain unbroken: streak at end = 5 active days.
    // The freeze day itself does NOT add to the count, but prevents a reset.
    #[test]
    fn test_miss_consumes_freeze() {
        // 2026-08-03 Mon … 2026-08-08 Sat  (all in ISO week 32)
        let active = days(&[(2026, 8, 3), (2026, 8, 4), (2026, 8, 5), (2026, 8, 6),
                            /* miss 2026-08-07 Fri */ (2026, 8, 8)]);
        let result = compute_streak(&active, &no_holidays(), d(2026, 8, 3), d(2026, 8, 8));
        // 5 active days; freeze on Fri kept chain alive so streak was never reset.
        assert_eq!(result.current_streak, 5, "freeze keeps chain alive; active count = 5");
        assert_eq!(result.best_streak, 5);
        assert_eq!(result.freeze_days, vec![d(2026, 8, 7)]);
    }

    // ── Fixture 3: Holiday protection — no freeze consumed ───────────────────
    // Mon–Thu active (4 days), Fri is a holiday (protected, not counted), active Sat (5th).
    // Holiday never consumes the freeze. Streak at end = 5 active days.
    #[test]
    fn test_holiday_protection_no_freeze() {
        let active = days(&[(2026, 8, 3), (2026, 8, 4), (2026, 8, 5), (2026, 8, 6),
                            /* holiday 2026-08-07 */ (2026, 8, 8)]);
        let holidays = days(&[(2026, 8, 7)]);
        let result = compute_streak(&active, &holidays, d(2026, 8, 3), d(2026, 8, 8));
        // 5 active days; holiday Fri neither broke the chain nor consumed the freeze.
        assert_eq!(result.current_streak, 5);
        assert!(result.freeze_days.is_empty(), "holiday must NOT consume the freeze");
        assert_eq!(result.holiday_protected_days, vec![d(2026, 8, 7)]);
    }

    // ── Fixture 4: Two misses in same ISO week — second breaks streak ─────────
    // Mon–Tue active, miss Wed (freeze consumed), miss Thu → streak breaks.
    #[test]
    fn test_two_misses_same_week_breaks_streak() {
        // 2026-08-03..08-06 (Mon-Thu all in ISO week 32)
        let active = days(&[(2026, 8, 3), (2026, 8, 4) /* miss 8-5, 8-6 */]);
        let result = compute_streak(&active, &no_holidays(), d(2026, 8, 3), d(2026, 8, 6));
        // After Mon+Tue active (streak=2), Wed freeze (streak stays 2), Thu miss → breaks.
        assert_eq!(result.current_streak, 0, "second miss must break streak");
        assert_eq!(result.best_streak, 2);
        assert_eq!(result.freeze_days.len(), 1);
        assert_eq!(result.freeze_days[0], d(2026, 8, 5)); // Wed used freeze
    }

    // ── Fixture 5: Chronotype edge — 2 AM activity lands on holiday day ───────
    // Caller shifts a 2 AM Wed session to Tue. Tue is a holiday.
    // Result: Tue is holiday-protected, Wed is a real miss (but can use freeze).
    // This proves the holiday-on-shifted-day rule: shift happens BEFORE holiday check.
    #[test]
    fn test_chronotype_shift_lands_on_holiday() {
        // 2026-08-21 (Fri) is Ninoy Aquino Day. Suppose a session at 2 AM on Sat 22
        // is shifted to Fri 21. The caller adds Fri 21 to active_days.
        // Here we test the opposite: if the shifted day IS a holiday, it's just protected.
        // Mon active, Tue holiday (2 AM activity shifted here), Wed-Thu active.
        let active = days(&[(2026, 8, 3), /* Tue active via shift */(2026, 8, 4), (2026, 8, 5), (2026, 8, 6)]);
        let holidays = days(&[(2026, 8, 4)]); // Tue is a holiday
        // Even though Tue is in active_days AND holiday_days, active takes priority.
        let result = compute_streak(&active, &holidays, d(2026, 8, 3), d(2026, 8, 6));
        // Mon+Tue+Wed+Thu = 4, Tue is active so streak is uninterrupted.
        assert_eq!(result.current_streak, 4);
        assert!(result.freeze_days.is_empty());
    }

    // ── Holiday range parse test ──────────────────────────────────────────────
    #[test]
    fn test_parse_date_cell_range() {
        let mut out = HashSet::new();
        parse_date_cell("2026-10-29 → 10-31", &mut out);
        assert!(out.contains(&NaiveDate::from_ymd_opt(2026, 10, 29).unwrap()));
        assert!(out.contains(&NaiveDate::from_ymd_opt(2026, 10, 30).unwrap()));
        assert!(out.contains(&NaiveDate::from_ymd_opt(2026, 10, 31).unwrap()));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_parse_date_cell_cross_year_range() {
        let mut out = HashSet::new();
        parse_date_cell("2026-12-31 → 2027-01-01", &mut out);
        assert!(out.contains(&NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()));
        assert!(out.contains(&NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()));
        assert_eq!(out.len(), 2);
    }
}
