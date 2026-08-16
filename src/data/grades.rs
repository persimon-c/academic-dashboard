use std::collections::HashMap;
use std::path::{Path, PathBuf};
use chrono::NaiveDate;
use regex::Regex;
use serde::Deserialize;

use crate::data::{DayMetric, MetricKind, ReaderError};
use crate::data::gradejoin::{normalize_course, match_grade_to_component, GradingComponent};

#[derive(Debug, Deserialize)]
struct GradeReturnEntry {
    course: String,
    assignment: String,
    grade: f64,
    max_points: f64,
    detected_at: String,
}

#[derive(Debug, Deserialize)]
struct GradeReturnsJson {
    returns: Vec<GradeReturnEntry>,
}

pub struct CourseGrading {
    pub course_name: String,
    pub components: Vec<GradingComponent>,
}

// Parses a single Grading.md file
pub fn parse_grading_md(path: &Path) -> Result<CourseGrading, ReaderError> {
    let content = std::fs::read_to_string(path).map_err(|e| ReaderError::Parse {
        file: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    // Parse course from frontmatter
    let course_re = Regex::new(r#"(?m)^course:\s*["']?([^"'\n]+)["']?"#).unwrap();
    let course_name = if let Some(caps) = course_re.captures(&content) {
        caps.get(1).unwrap().as_str().trim().to_string()
    } else {
        // Fallback to directory name
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("UNKNOWN")
            .to_string()
    };

    // Parse markdown components table: | Component | Weight | Category |
    // Match rows like: | Quizzes | 10% | Lecture |
    let table_re = Regex::new(r"(?m)^\s*\|\s*([^|]+?)\s*\|\s*(\d+(?:\.\d+)?)%\s*\|\s*([^|]+?)\s*\|").unwrap();
    let mut components = Vec::new();

    for caps in table_re.captures_iter(&content) {
        let name = caps.get(1).unwrap().as_str().trim().to_string();
        let weight_percent = caps.get(2).unwrap().as_str().parse::<f64>().unwrap_or(0.0);
        let category = caps.get(3).unwrap().as_str().trim().to_string();

        components.push(GradingComponent {
            name,
            weight_percent,
            category,
        });
    }

    Ok(CourseGrading {
        course_name,
        components,
    })
}

// Parses all Grading.md files in academics folder
pub fn load_all_gradings(academics_dir: &Path) -> Result<HashMap<String, CourseGrading>, ReaderError> {
    let mut gradings = HashMap::new();
    if !academics_dir.exists() {
        return Ok(gradings);
    }

    let entries = std::fs::read_dir(academics_dir).map_err(|e| ReaderError::Parse {
        file: academics_dir.to_path_buf(),
        reason: format!("Failed to read academics dir: {}", e),
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| ReaderError::Parse {
            file: academics_dir.to_path_buf(),
            reason: e.to_string(),
        })?;
        let path = entry.path();
        if path.is_dir() {
            let grading_file = path.join("Grading.md");
            if grading_file.exists() {
                if let Ok(grading) = parse_grading_md(&grading_file) {
                    let normalized = normalize_course(&grading.course_name);
                    gradings.insert(normalized, grading);
                }
            }
        }
    }

    Ok(gradings)
}

pub fn read_grade_returns(
    returns_json_path: &Path,
    academics_dir: &Path,
) -> Result<Vec<DayMetric>, ReaderError> {
    if !returns_json_path.exists() {
        return Err(ReaderError::Missing(returns_json_path.to_path_buf()));
    }

    // Load grading rules
    let gradings = load_all_gradings(academics_dir)?;

    let content = std::fs::read_to_string(returns_json_path).map_err(|e| ReaderError::Parse {
        file: returns_json_path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let json_data: GradeReturnsJson = serde_json::from_str(&content).map_err(|e| ReaderError::Parse {
        file: returns_json_path.to_path_buf(),
        reason: format!("JSON deserialization failed: {}", e),
    })?;

    let mut metrics = Vec::new();

    for entry in json_data.returns {
        // Parse date from ISO string (e.g. 2026-08-12T15:32:24.491Z)
        let date_str = entry.detected_at.split('T').next().unwrap_or("");
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|e| ReaderError::Parse {
            file: returns_json_path.to_path_buf(),
            reason: format!("Failed to parse date '{}': {}", date_str, e),
        })?;

        // Normalize course
        let norm_course = normalize_course(&entry.course);

        // Find components and map
        let mut value = 0;
        if let Some(grading) = gradings.get(&norm_course) {
            if let Some(component) = match_grade_to_component(&entry.course, &entry.assignment, &grading.components) {
                // Return metric value can be scaled or represented simply as the count of grade returns for now,
                // or the grade score itself, or weighted grade contribution.
                // The week grid MVP specifies stacked bars for "grade returns" counts, so we use count (value = 1).
                value = 1;
            }
        }

        if value == 0 {
            // Default value if mapping isn't found
            value = 1;
        }

        metrics.push(DayMetric {
            date,
            kind: MetricKind::Grade,
            value,
        });
    }

    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_parse_grading_md() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("Grading.md");
        let mut file = std::fs::File::create(&file_path).unwrap();

        writeln!(
            file,
            "---\ntype: grading\ncourse: \"CMSC 124\"\n---\n| Component | Weight | Category |\n|---|---|---|\n| Quizzes | 10% | Lecture |\n| Lab Exercises | 35% | Lab |"
        )
        .unwrap();

        let grading = parse_grading_md(&file_path).unwrap();
        assert_eq!(grading.course_name, "CMSC 124");
        assert_eq!(grading.components.len(), 2);
        assert_eq!(grading.components[0].name, "Quizzes");
        assert_eq!(grading.components[0].weight_percent, 10.0);
    }
}
