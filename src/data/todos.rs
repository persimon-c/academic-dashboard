use std::collections::HashMap;
use std::path::Path;
use chrono::NaiveDate;
use regex::Regex;
use crate::data::{DayMetric, MetricKind, ReaderError};

pub fn read_todo_completions(path: &Path) -> Result<Vec<DayMetric>, ReaderError> {
    if !path.exists() {
        return Err(ReaderError::Missing(path.to_path_buf()));
    }

    let content = std::fs::read_to_string(path).map_err(|e| ReaderError::Parse {
        file: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    // Regex to match done tasks and extract done_at date:
    // e.g. - [x] ... <!-- id:: 11 done_at:: 2026-08-01 -->
    let re = Regex::new(r"-\s*\[x\].*?done_at::\s*(\d{4}-\d{2}-\d{2})").unwrap();

    let mut daily_counts: HashMap<NaiveDate, u32> = HashMap::new();

    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            if let Some(date_match) = caps.get(1) {
                if let Ok(date) = NaiveDate::parse_from_str(date_match.as_str(), "%Y-%m-%d") {
                    *daily_counts.entry(date).or_insert(0) += 1;
                }
            }
        }
    }

    let mut metrics: Vec<DayMetric> = daily_counts
        .into_iter()
        .map(|(date, value)| DayMetric {
            date,
            kind: MetricKind::Todo,
            value,
        })
        .collect();

    metrics.sort_by_key(|m| m.date);
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_todo_completions() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# SMON OS TODO").unwrap();
        writeln!(file, "- [x] Task 1 <!-- id:: 1 done_at:: 2026-08-01 -->").unwrap();
        writeln!(file, "- [x] Task 2 <!-- id:: 2 done_at:: 2026-08-01 -->").unwrap();
        writeln!(file, "- [x] Task 3 <!-- id:: 3 done_at:: 2026-08-02 -->").unwrap();
        writeln!(file, "- [ ] Task 4 <!-- id:: 4 -->").unwrap(); // Pending

        let metrics = read_todo_completions(file.path()).unwrap();
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].date, NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());
        assert_eq!(metrics[0].value, 2);
        assert_eq!(metrics[1].date, NaiveDate::from_ymd_opt(2026, 8, 2).unwrap());
        assert_eq!(metrics[1].value, 1);
    }
}
