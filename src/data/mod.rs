use std::path::PathBuf;
use chrono::NaiveDate;

pub mod github;
pub mod todos;


#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MetricKind {
    Github,
    Todo,
    Grade,
    Attendance,
    Sleep,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DayMetric {
    pub date: NaiveDate,
    pub kind: MetricKind,
    pub value: u32,
}

#[derive(thiserror::Error, Debug)]
pub enum ReaderError {
    #[error("missing file: {0}")]
    Missing(PathBuf),
    #[error("parse failed in {file}: {reason}")]
    Parse { file: PathBuf, reason: String },
}
