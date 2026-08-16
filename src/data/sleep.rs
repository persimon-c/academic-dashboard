use std::path::Path;
use chrono::{NaiveDate, NaiveTime, Timelike};
use crate::data::ReaderError;

#[derive(Debug, Clone)]
pub struct SleepEntry {
    /// The calendar date this entry belongs to.
    pub date: NaiveDate,
    /// Wake time (HH:MM). May be None if unparseable.
    pub wake_time: Option<NaiveTime>,
    /// Bed time (HH:MM). Note: bed times like "02:00" may be past midnight —
    /// callers can apply chronotype shifting if needed (M3 defers the heuristic).
    pub bed_time: Option<NaiveTime>,
    /// Raw notes column.
    pub notes: String,
}

impl SleepEntry {
    /// Sleep duration in hours, naively computed.
    /// Returns None if either time is missing.
    /// If bed_time < wake_time (cross-midnight), adds 24 h.
    pub fn duration_hours(&self) -> Option<f64> {
        let wake = self.wake_time?;
        let bed = self.bed_time?;

        let wake_mins = wake.num_seconds_from_midnight() as i64;
        let mut bed_mins = bed.num_seconds_from_midnight() as i64;

        // If bed time is "before" wake time in clock terms, it's the next day.
        if bed_mins < wake_mins {
            bed_mins += 24 * 3600;
        }

        Some((bed_mins - wake_mins) as f64 / 3600.0)
    }
}

pub fn read_sleep_log(path: &Path) -> Result<Vec<SleepEntry>, ReaderError> {
    if !path.exists() {
        return Err(ReaderError::Missing(path.to_path_buf()));
    }

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|e| ReaderError::Parse {
            file: path.to_path_buf(),
            reason: e.to_string(),
        })?;

    let mut entries = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| ReaderError::Parse {
            file: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        if record.len() < 3 {
            continue; // skip malformed rows
        }

        let date_str = record[0].trim().trim_matches('"');
        let wake_str = record[1].trim().trim_matches('"');
        let bed_str  = record[2].trim().trim_matches('"');
        let notes    = record.get(3).unwrap_or("").trim().trim_matches('"').to_string();

        let date = match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue, // skip unparseable dates
        };

        let wake_time = NaiveTime::parse_from_str(wake_str, "%H:%M").ok();
        let bed_time  = NaiveTime::parse_from_str(bed_str,  "%H:%M").ok();

        entries.push(SleepEntry {
            date,
            wake_time,
            bed_time,
            notes,
        });
    }

    entries.sort_by_key(|e| e.date);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_sleep_log() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "date,wake_time,bed_time,notes").unwrap();
        writeln!(file, "\"2026-07-29\",\"18:00\",\"02:00\",\"Late night session\"").unwrap();
        writeln!(file, "\"2026-07-30\",\"10:00\",\"23:00\",\"\"").unwrap();

        let entries = read_sleep_log(file.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].date, NaiveDate::from_ymd_opt(2026, 7, 29).unwrap());
        assert_eq!(entries[0].wake_time, NaiveTime::from_hms_opt(18, 0, 0));
        assert_eq!(entries[0].bed_time,  NaiveTime::from_hms_opt(2,  0, 0));
        // Cross-midnight: 18:00 wake → 02:00 bed = 8 hours
        let dur = entries[0].duration_hours().unwrap();
        assert!((dur - 8.0).abs() < 0.001, "expected 8h, got {}", dur);
    }

    #[test]
    fn test_duration_same_day() {
        let entry = SleepEntry {
            date: NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            wake_time: NaiveTime::from_hms_opt(10, 0, 0),
            bed_time:  NaiveTime::from_hms_opt(23, 0, 0),
            notes: String::new(),
        };
        let dur = entry.duration_hours().unwrap();
        assert!((dur - 13.0).abs() < 0.001);
    }
}
