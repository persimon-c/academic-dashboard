use std::path::Path;
use chrono::{NaiveDate, NaiveTime, Duration, Weekday, Datelike};
use icalendar::{Calendar, CalendarComponent, Component};
use crate::data::ReaderError;

/// A single expanded occurrence of a class block from schedule.ics.
#[derive(Debug, Clone)]
pub struct ClassBlock {
    pub date: NaiveDate,
    /// Start time in Manila time (UTC+8 already applied).
    pub start_time: NaiveTime,
    /// End time in Manila time (UTC+8 already applied).
    pub end_time: NaiveTime,
    pub summary: String,
    pub location: String,
}

/// Parse and expand all VEVENT occurrences in schedule.ics into concrete ClassBlocks.
/// Supports only FREQ=WEEKLY;BYDAY=<day>;UNTIL=<datetime> — the conservative subset
/// the plan mandates. Any other RRULE pattern is shown as first-occurrence-only.
pub fn expand_schedule(
    ics_path: &Path,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<ClassBlock>, ReaderError> {
    if !ics_path.exists() {
        return Err(ReaderError::Missing(ics_path.to_path_buf()));
    }

    let content = std::fs::read_to_string(ics_path).map_err(|e| ReaderError::Parse {
        file: ics_path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let calendar: Calendar = content.parse().map_err(|e| ReaderError::Parse {
        file: ics_path.to_path_buf(),
        reason: format!("ICS parse error: {:?}", e),
    })?;

    let mut blocks = Vec::new();

    for component in &calendar.components {
        if let CalendarComponent::Event(event) = component {
            let summary  = event.property_value("SUMMARY") .unwrap_or("").to_string();
            let location = event.property_value("LOCATION").unwrap_or("").to_string();
            let dtstart  = event.property_value("DTSTART") .unwrap_or("");
            let dtend    = event.property_value("DTEND")   .unwrap_or("");
            let rrule    = event.property_value("RRULE")   .unwrap_or("");

            // Parse UTC datetime strings like "20260803T020000Z"
            let Some((start_date, start_time)) = parse_ics_datetime_utc8(dtstart) else { continue };
            let end_time = parse_ics_datetime_utc8(dtend).map(|(_, t)| t).unwrap_or(start_time);

            if rrule.is_empty() {
                // One-off event
                if start_date >= from && start_date <= to {
                    blocks.push(ClassBlock { date: start_date, start_time, end_time, summary, location });
                }
                continue;
            }

            // Parse RRULE: FREQ=WEEKLY;BYDAY=MO;UNTIL=20261207T155959Z
            let byday   = rrule_field(rrule, "BYDAY");
            let until   = rrule_field(rrule, "UNTIL").and_then(|s| parse_ics_datetime_utc8(s).map(|(d, _)| d));
            let is_weekly = rrule.contains("FREQ=WEEKLY");

            if !is_weekly || byday.is_none() {
                // Non-standard: show first occurrence only, mark conservative.
                if start_date >= from && start_date <= to {
                    let marked = format!("{} [first only]", summary);
                    blocks.push(ClassBlock { date: start_date, start_time, end_time, summary: marked, location });
                }
                continue;
            }

            let target_weekday = parse_byday(byday.unwrap());
            let effective_until = until.unwrap_or(to).min(to);

            // Walk from start_date, stepping weekly, collecting occurrences in [from, to].
            let mut cur = start_date;
            while cur <= effective_until {
                if cur >= from {
                    // Verify it's the right weekday (should always match, but guard it)
                    if target_weekday.map_or(true, |wd| cur.weekday() == wd) {
                        blocks.push(ClassBlock {
                            date: cur,
                            start_time,
                            end_time,
                            summary: summary.clone(),
                            location: location.clone(),
                        });
                    }
                }
                cur = cur + Duration::weeks(1);
            }
        }
    }

    blocks.sort_by_key(|b| (b.date, b.start_time));
    Ok(blocks)
}

/// Parse "20260803T020000Z" → (NaiveDate, NaiveTime) in Manila time (UTC+8).
fn parse_ics_datetime_utc8(s: &str) -> Option<(NaiveDate, NaiveTime)> {
    // Format: YYYYMMDDTHHMMSSz  (with or without trailing Z)
    let s = s.trim_end_matches('Z');
    if s.len() < 15 { return None; }
    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[4..6].parse().ok()?;
    let day: u32   = s[6..8].parse().ok()?;
    let hour: u32  = s[9..11].parse().ok()?;
    let min: u32   = s[11..13].parse().ok()?;

    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    // Apply Manila offset (+8h) and handle day rollover
    let total_mins: u32 = hour * 60 + min + 8 * 60;
    let manila_hour = (total_mins / 60) % 24;
    let extra_days  = (total_mins / 60) / 24;
    let time = NaiveTime::from_hms_opt(manila_hour, total_mins % 60, 0)?;
    let date = date + Duration::days(extra_days as i64);
    Some((date, time))
}

/// Extract a field value from RRULE string, e.g. "BYDAY" → "MO".
fn rrule_field<'a>(rrule: &'a str, key: &str) -> Option<&'a str> {
    for part in rrule.split(';') {
        if let Some(val) = part.strip_prefix(key).and_then(|s| s.strip_prefix('=')) {
            return Some(val);
        }
    }
    None
}

/// Map BYDAY abbreviation to chrono Weekday.
fn parse_byday(s: &str) -> Option<Weekday> {
    match s {
        "MO" => Some(Weekday::Mon),
        "TU" => Some(Weekday::Tue),
        "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu),
        "FR" => Some(Weekday::Fri),
        "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun),
        _    => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ics_datetime_utc8() {
        // 2026-08-03 02:00:00Z → Manila 2026-08-03 10:00
        let (d, t) = parse_ics_datetime_utc8("20260803T020000Z").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());
        assert_eq!(t, NaiveTime::from_hms_opt(10, 0, 0).unwrap());
    }

    #[test]
    fn test_parse_ics_datetime_day_rollover() {
        // 23:00 UTC + 8h = 07:00 next day
        let (d, t) = parse_ics_datetime_utc8("20260803T230000Z").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 8, 4).unwrap());
        assert_eq!(t, NaiveTime::from_hms_opt(7, 0, 0).unwrap());
    }

    #[test]
    fn test_rrule_field() {
        let r = "FREQ=WEEKLY;BYDAY=MO;UNTIL=20261207T155959Z";
        assert_eq!(rrule_field(r, "BYDAY"),  Some("MO"));
        assert_eq!(rrule_field(r, "FREQ"),   Some("WEEKLY"));
        assert_eq!(rrule_field(r, "UNTIL"),  Some("20261207T155959Z"));
        assert_eq!(rrule_field(r, "ABSENT"), None);
    }
}
