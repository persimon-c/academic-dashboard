use std::path::Path;
use regex::Regex;
use crate::data::{DayMetric, MetricKind, ReaderError};
use crate::data::gradejoin::normalize_course;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CourseAttendanceStatus {
    pub course: String,
    pub lecture_absences: u32,
    pub lab_absences: u32,
    pub lecture_limit: u32,
    pub lab_limit: u32,
}

#[allow(dead_code)]
#[derive(Debug)]
struct RawAttendanceEntry {
    date_str: String,
    course: String,
    component: String,
    detail: String,
}

// Parses attendance.md and returns DayMetrics representing absences
pub fn read_attendance_absences(path: &Path) -> Result<Vec<DayMetric>, ReaderError> {
    if !path.exists() {
        return Err(ReaderError::Missing(path.to_path_buf()));
    }

    let content = std::fs::read_to_string(path).map_err(|e| ReaderError::Parse {
        file: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let mut absences = Vec::new();
    let mut in_absences_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Absences") {
            in_absences_section = true;
            continue;
        } else if trimmed.starts_with("## ") {
            in_absences_section = false;
        }

        if in_absences_section && trimmed.starts_with("- ") {
            if let Some(entry) = parse_attendance_line(trimmed) {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(&entry.date_str, "%Y-%m-%d") {
                    absences.push(DayMetric {
                        date,
                        kind: MetricKind::Attendance,
                        value: 1, // 1 absence
                    });
                }
            }
        }
    }

    Ok(absences)
}

fn parse_attendance_line(line: &str) -> Option<RawAttendanceEntry> {
    // Remove comments
    let clean_line = if let Some(idx) = line.find("<!--") {
        line[..idx].trim()
    } else {
        line.trim()
    };
    let body = clean_line.strip_prefix("- ")?.trim();
    let parts: Vec<&str> = body.split('|').map(|s| s.trim()).collect();
    if parts.len() < 3 {
        return None;
    }

    Some(RawAttendanceEntry {
        date_str: parts[0].to_string(),
        course: parts[1].to_string(),
        component: parts[2].to_string(),
        detail: parts.get(3).cloned().unwrap_or("").to_string(),
    })
}

// Extracts attendance limits from a course's Grading.md
fn get_course_attendance_limits(academics_dir: &Path, course_name: &str) -> (u32, u32) {
    let mut lecture_limit = 4; // default conservative UP rule
    let mut lab_limit = 4;

    let norm = normalize_course(course_name);
    let course_dir = academics_dir.join(norm.replace(' ', ""));
    let grading_file = course_dir.join("Grading.md");

    if grading_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&grading_file) {
            for line in content.lines() {
                if line.to_lowercase().contains("absence") || line.to_lowercase().contains("attendance") {
                    // Match e.g. ">7 lecture", "≥7 lecture", "4 lab"
                    let re = Regex::new(r"(?i)([≥>]\s*)?(\d+)\s*(lecture|lab)").unwrap();
                    if let Some(caps) = re.captures(line) {
                        let n = caps.get(2).unwrap().as_str().parse::<u32>().unwrap_or(4);
                        let is_greater = caps.get(1).map_or(false, |m| m.as_str().contains('>'));
                        let final_limit = if is_greater { n + 1 } else { n };

                        if caps.get(3).unwrap().as_str().to_lowercase() == "lecture" {
                            lecture_limit = final_limit;
                        } else {
                            lab_limit = final_limit;
                        }
                    }
                }
            }
        }
    }

    (lecture_limit, lab_limit)
}

pub fn get_attendance_status(
    attendance_path: &Path,
    academics_dir: &Path,
) -> Result<Vec<CourseAttendanceStatus>, ReaderError> {
    if !attendance_path.exists() {
        return Err(ReaderError::Missing(attendance_path.to_path_buf()));
    }

    let content = std::fs::read_to_string(attendance_path).map_err(|e| ReaderError::Parse {
        file: attendance_path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let mut in_absences_section = false;
    let mut course_map: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Absences") {
            in_absences_section = true;
            continue;
        } else if trimmed.starts_with("## ") {
            in_absences_section = false;
        }

        if in_absences_section && trimmed.starts_with("- ") {
            if let Some(entry) = parse_attendance_line(trimmed) {
                let norm = normalize_course(&entry.course);
                let (ref mut lec, ref mut lab) = course_map.entry(norm).or_insert((0, 0));
                if entry.component.to_lowercase().contains("lab") {
                    *lab += 1;
                } else {
                    *lec += 1;
                }
            }
        }
    }

    let mut status_list = Vec::new();
    for (course, (lec_abs, lab_abs)) in course_map {
        let (lec_limit, lab_limit) = get_course_attendance_limits(academics_dir, &course);
        status_list.push(CourseAttendanceStatus {
            course,
            lecture_absences: lec_abs,
            lab_absences: lab_abs,
            lecture_limit: lec_limit,
            lab_limit,
        });
    }

    Ok(status_list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_attendance() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# Attendance Log").unwrap();
        writeln!(file, "## Absences").unwrap();
        writeln!(file, "- 2026-08-11 | CS 101 | lecture |  |  <!-- id:: 1 -->").unwrap();
        writeln!(file, "- 2026-08-12 | CS 101 | lab |  |  <!-- id:: 2 -->").unwrap();

        let absences = read_attendance_absences(file.path()).unwrap();
        assert_eq!(absences.len(), 2);
        assert_eq!(absences[0].value, 1);
        assert_eq!(absences[0].kind, MetricKind::Attendance);
    }
}
