use std::path::Path;
use chrono::NaiveDate;
use regex::Regex;
use crate::data::ReaderError;

/// A parsed exam event with an optional concrete date.
#[derive(Debug, Clone)]
pub struct ExamEvent {
    pub course: String,
    pub label: String,
    /// Concrete date if parseable, None for TBD/range-only entries.
    pub date: Option<NaiveDate>,
    /// Raw date string from the table (for display).
    pub window_raw: String,
}

impl ExamEvent {
    /// Days until this exam from today. Negative = already past.
    pub fn days_until(&self, today: NaiveDate) -> Option<i64> {
        self.date.map(|d| (d - today).num_days())
    }
}

/// Parse ExamSeasons.md and return all exam events that have at least a window.
pub fn read_exam_events(path: &Path) -> Result<Vec<ExamEvent>, ReaderError> {
    if !path.exists() {
        return Err(ReaderError::Missing(path.to_path_buf()));
    }
    let content = std::fs::read_to_string(path).map_err(|e| ReaderError::Parse {
        file: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    // Match table rows: | Course | Exam Type | Est. Week | Dates | Source |
    // e.g. | CMSC 124 | Lecture Exam 1 | Week 5 | Sep 3 (Wed) | course guide |
    let row_re = Regex::new(r"(?m)^\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|").unwrap();

    // Month name to number
    let month_re = Regex::new(r"(?i)(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+(\d{1,2})").unwrap();

    let mut events = Vec::new();

    for caps in row_re.captures_iter(&content) {
        let course  = caps.get(1).unwrap().as_str().trim().to_string();
        let label   = caps.get(2).unwrap().as_str().trim().to_string();
        let window  = caps.get(4).unwrap().as_str().trim().to_string();

        // Skip header rows
        if course.eq_ignore_ascii_case("course") || course.starts_with('-') || window.eq_ignore_ascii_case("dates") {
            continue;
        }
        // Skip rows with no date info
        if window == "—" || window.is_empty() {
            events.push(ExamEvent { course, label, date: None, window_raw: window });
            continue;
        }

        // Try to parse a concrete date. "Sep 3 (Wed)" → Sep 3 2026, "Sep 28–Oct 2" → Sep 28 2026
        let date = month_re.captures(&window).and_then(|m| {
            let month_str = m.get(1)?.as_str();
            let day: u32  = m.get(2)?.as_str().parse().ok()?;
            let month = month_abbrev_to_num(month_str)?;
            NaiveDate::from_ymd_opt(2026, month, day)
        });

        events.push(ExamEvent { course, label, date, window_raw: window });
    }

    // Sort by date (TBD entries go last)
    events.sort_by(|a, b| match (a.date, b.date) {
        (Some(da), Some(db)) => da.cmp(&db),
        (Some(_), None)      => std::cmp::Ordering::Less,
        (None, Some(_))      => std::cmp::Ordering::Greater,
        (None, None)         => std::cmp::Ordering::Equal,
    });

    Ok(events)
}

fn month_abbrev_to_num(s: &str) -> Option<u32> {
    match s.to_lowercase().as_str() {
        "jan" => Some(1),  "feb" => Some(2),  "mar" => Some(3),
        "apr" => Some(4),  "may" => Some(5),  "jun" => Some(6),
        "jul" => Some(7),  "aug" => Some(8),  "sep" => Some(9),
        "oct" => Some(10), "nov" => Some(11), "dec" => Some(12),
        _     => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_exam_events() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# Exam Seasons").unwrap();
        writeln!(file, "| Course | Exam Type | Est. Week | Dates | Source |").unwrap();
        writeln!(file, "|---|---|---|---|---|").unwrap();
        writeln!(file, "| CS 101 | Lecture Exam 1 | Week 5 | Sep 3 (Wed) | course guide |").unwrap();
        writeln!(file, "| CS 102 | Exam 1 | Week 8 | Sep 25 (7–9 PM) | course site |").unwrap();
        writeln!(file, "| CS 103 | — | — | — | TBD |").unwrap();

        let events = read_exam_events(file.path()).unwrap();
        // Exam entries (non-dash-date) should be parsed
        let with_date: Vec<_> = events.iter().filter(|e| e.date.is_some()).collect();
        assert_eq!(with_date.len(), 2);
        assert_eq!(with_date[0].date.unwrap(), NaiveDate::from_ymd_opt(2026, 9, 3).unwrap());
        assert_eq!(with_date[0].course, "CS 101");
        assert_eq!(with_date[1].date.unwrap(), NaiveDate::from_ymd_opt(2026, 9, 25).unwrap());
        assert_eq!(with_date[1].course, "CS 102");
    }
}
