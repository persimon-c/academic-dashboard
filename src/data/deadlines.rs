use std::path::Path;
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use crate::data::ReaderError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deadline {
    pub id: String,
    pub course: String,
    pub title: String,
    pub due: DateTime<FixedOffset>,
    pub max_points: Option<f64>,
    pub submitted: bool,
    pub submission_state: String,
}

#[derive(Debug, Deserialize)]
struct RawAssignment {
    id: String,
    title: String,
    due: Option<String>,
    max_points: Option<f64>,
    submitted: bool,
    submission_state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCourse {
    name: String,
    assignments: Option<Vec<RawAssignment>>,
}

#[derive(Debug, Deserialize)]
struct RawClassroomData {
    courses: Option<Vec<RawCourse>>,
}

pub fn read_classroom_deadlines(path: &Path) -> Result<Vec<Deadline>, ReaderError> {
    if !path.exists() {
        return Err(ReaderError::Missing(path.to_path_buf()));
    }

    let content = std::fs::read_to_string(path).map_err(|e| ReaderError::Parse {
        file: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let raw_data: RawClassroomData = serde_json::from_str(&content).map_err(|e| ReaderError::Parse {
        file: path.to_path_buf(),
        reason: format!("JSON parse failed: {}", e),
    })?;

    let mut deadlines = Vec::new();
    let manila_offset = FixedOffset::east_opt(8 * 3600).unwrap();

    if let Some(courses) = raw_data.courses {
        for course in courses {
            if let Some(assignments) = course.assignments {
                for ass in assignments {
                    if let Some(due_str) = ass.due {
                        // Parse ISO/RFC3339 string (e.g. 2026-08-12T00:00:00.000Z)
                        if let Ok(utc_dt) = DateTime::parse_from_rfc3339(&due_str) {
                            // Convert to Manila time (UTC+8)
                            let manila_dt = utc_dt.with_timezone(&manila_offset);
                            deadlines.push(Deadline {
                                id: ass.id,
                                course: course.name.clone(),
                                title: ass.title,
                                due: manila_dt,
                                max_points: ass.max_points,
                                submitted: ass.submitted,
                                submission_state: ass.submission_state.unwrap_or_else(|| "assigned".to_string()),
                            });
                        }
                    }
                }
            }
        }
    }

    // Sort by due date/time
    deadlines.sort_by_key(|d| d.due);
    Ok(deadlines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_classroom_deadlines() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{
                "courses": [
                    {{
                        "name": "CS 101 SEC-A",
                        "assignments": [
                            {{
                                "id": "1",
                                "title": "Ex 1",
                                "due": "2026-08-10T12:00:00Z",
                                "max_points": 100.0,
                                "submitted": false,
                                "submission_state": "assigned"
                            }}
                        ]
                    }}
                ]
            }}"#
        )
        .unwrap();

        let deadlines = read_classroom_deadlines(file.path()).unwrap();
        assert_eq!(deadlines.len(), 1);
        assert_eq!(deadlines[0].course, "CS 101 SEC-A");
        // Verify Manila conversion: 2026-08-10T12:00:00Z -> 2026-08-10T20:00:00+08:00
        assert_eq!(deadlines[0].due.to_rfc3339(), "2026-08-10T20:00:00+08:00");
    }
}
