use std::path::PathBuf;
use chrono::NaiveDate;

pub mod github;
pub mod todos;
pub mod calendar;
pub mod gradejoin;
pub mod grades;
pub mod attendance;
pub mod deadlines;
pub mod streak;
pub mod sleep;
pub mod rrule;
pub mod exams;
pub mod org;




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
