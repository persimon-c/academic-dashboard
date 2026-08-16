use std::path::Path;
use chrono::NaiveDate;
use crate::data::{DayMetric, MetricKind, ReaderError};

pub fn read_github_contributions(path: &Path) -> Result<Vec<DayMetric>, ReaderError> {
    if !path.exists() {
        return Err(ReaderError::Missing(path.to_path_buf()));
    }

    let mut reader = csv::Reader::from_path(path).map_err(|e| ReaderError::Parse {
        file: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let mut metrics = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| ReaderError::Parse {
            file: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        if record.len() < 2 {
            return Err(ReaderError::Parse {
                file: path.to_path_buf(),
                reason: "Record has fewer than 2 columns".to_string(),
            });
        }

        let date_str = &record[0];
        let count_str = &record[1];

        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|e| ReaderError::Parse {
            file: path.to_path_buf(),
            reason: format!("Failed to parse date '{}': {}", date_str, e),
        })?;

        let value = count_str.parse::<u32>().map_err(|e| ReaderError::Parse {
            file: path.to_path_buf(),
            reason: format!("Failed to parse contributions '{}': {}", count_str, e),
        })?;

        metrics.push(DayMetric {
            date,
            kind: MetricKind::Github,
            value,
        });
    }

    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_github_contributions() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "date,contributions").unwrap();
        writeln!(file, "2026-07-01,32").unwrap();
        writeln!(file, "2026-07-02,14").unwrap();

        let metrics = read_github_contributions(file.path()).unwrap();
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].date, NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());
        assert_eq!(metrics[0].value, 32);
        assert_eq!(metrics[0].kind, MetricKind::Github);
    }
}
