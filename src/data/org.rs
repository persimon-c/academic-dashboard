use std::path::Path;
use chrono::NaiveDate;
use regex::Regex;
use crate::data::ReaderError;

/// An org-side event or deadline extracted from COSS files.
#[derive(Debug, Clone)]
pub struct OrgEvent {
    pub date: NaiveDate,
    /// Short label for display.
    pub label: String,
    /// Source file it came from (for drill-down).
    pub source: String,
}

/// Scan a directory of org files and extract all dated events from markdown tables.
/// Currently reads `RecruitmentCycle-2026-27.md` — the only file with a machine-readable
/// date table for the current semester. Other event files are reference-only (no dates).
pub fn read_org_events(events_dir: &Path) -> Result<Vec<OrgEvent>, ReaderError> {
    if !events_dir.exists() {
        return Err(ReaderError::Missing(events_dir.to_path_buf()));
    }

    let mut events = Vec::new();

    // Walk the events directory for files with date tables.
    let recruitment_path = events_dir.join("RecruitmentCycle-2026-27.md");
    if recruitment_path.exists() {
        parse_recruitment_table(&recruitment_path, &mut events);
    }

    // Also check dev-team/Sprints.md one level up — paths are relative to events_dir parent.
    // sprints.md has fuzzy windows only; skip for now (Plan.md: org.rs reads events/ for now).

    events.sort_by_key(|e| e.date);
    Ok(events)
}

/// Parse rows like `| Aug 17 | Mon | Fam Dinner, Reporting starts |` from the
/// recruitment timeline table. Year is always 2026 (current semester).
fn parse_recruitment_table(path: &Path, out: &mut Vec<OrgEvent>) {
    let Ok(content) = std::fs::read_to_string(path) else { return };

    // Match `| Aug 17 | Mon | <label> |`
    let row_re = Regex::new(r"(?m)^\s*\|\s*((?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{1,2})\s*\|\s*\w+\s*\|\s*([^|]+?)\s*\|").unwrap();
    let month_re = Regex::new(r"(?i)(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+(\d{1,2})").unwrap();
    let source = path.file_name().unwrap_or_default().to_string_lossy().to_string();

    for caps in row_re.captures_iter(&content) {
        let date_str  = caps.get(1).unwrap().as_str().trim();
        let label_raw = caps.get(2).unwrap().as_str().trim();

        if let Some(m) = month_re.captures(date_str) {
            let month_str = m.get(1).unwrap().as_str();
            let day: u32  = m.get(2).unwrap().as_str().parse().unwrap_or(0);
            let month = month_abbrev_to_num(month_str).unwrap_or(0);
            if let Some(date) = NaiveDate::from_ymd_opt(2026, month, day) {
                // Strip markdown bold (**) and truncate long labels
                let label = label_raw
                    .replace("**", "")
                    .split(',')
                    .next()
                    .unwrap_or(label_raw)
                    .trim()
                    .to_string();
                let label = if label.len() > 32 { format!("{}…", &label[..31]) } else { label };

                out.push(OrgEvent { date, label, source: source.clone() });
            }
        }
    }
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
    use tempfile::TempDir;

    #[test]
    fn test_parse_recruitment_table() {
        let dir = TempDir::new().unwrap();
        let mut file = std::fs::File::create(dir.path().join("RecruitmentCycle-2026-27.md")).unwrap();
        writeln!(file, "## Timeline (Aug 11 – Sep 5)").unwrap();
        writeln!(file, "| Date | Day | Activities |").unwrap();
        writeln!(file, "|---|---|---|").unwrap();
        writeln!(file, "| Aug 12 | Wed | **Orientation** |").unwrap();
        writeln!(file, "| Aug 17 | Mon | Fam Dinner, Reporting starts |").unwrap();
        writeln!(file, "| Sep 5 | Sat | **Finals**, **Acceptance Rites** |").unwrap();

        let events = read_org_events(dir.path()).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].date, NaiveDate::from_ymd_opt(2026, 8, 12).unwrap());
        assert_eq!(events[0].label, "Orientation");
        assert_eq!(events[1].date, NaiveDate::from_ymd_opt(2026, 8, 17).unwrap());
        assert!(events[1].label.contains("Fam Dinner") || events[1].label == "Fam Dinner");
        assert_eq!(events[2].date, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap());
    }
}
