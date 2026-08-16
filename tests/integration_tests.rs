use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;
use chrono::NaiveDate;

use academic_dashboard::data::github::read_github_contributions;
use academic_dashboard::data::todos::read_todo_completions;
use academic_dashboard::data::grades::read_grade_returns;
use academic_dashboard::data::attendance::get_attendance_status;
use academic_dashboard::data::deadlines::read_classroom_deadlines;
use academic_dashboard::data::streak::load_holiday_days;
use academic_dashboard::data::rrule::expand_schedule;
use academic_dashboard::data::exams::read_exam_events;
use academic_dashboard::data::org::read_org_events;

struct MockVault {
    _dir: TempDir,
    pub path: PathBuf,
}

impl MockVault {
    fn create() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        // Recreate directory tree
        fs::create_dir_all(path.join("_AI/tracking")).unwrap();
        fs::create_dir_all(path.join("_AI/todos")).unwrap();
        fs::create_dir_all(path.join("_AI/inbox")).unwrap();
        fs::create_dir_all(path.join("02_AREAS/academics/CS101")).unwrap();
        fs::create_dir_all(path.join("02_AREAS/org/COSS/events")).unwrap();

        let mut val = Self { _dir: dir, path };
        val.write_defaults();
        val
    }

    fn write_defaults(&mut self) {
        // 1. GitHub Contributions
        let mut f = File::create(self.path.join("_AI/tracking/github-contributions.csv")).unwrap();
        writeln!(f, "date,commits").unwrap();
        writeln!(f, "2026-08-04,5").unwrap();
        writeln!(f, "2026-08-05,2").unwrap();

        // 2. Todos
        let mut f = File::create(self.path.join("_AI/todos/todos.md")).unwrap();
        writeln!(f, "- [x] Done todo <!-- id:: 1 done_at:: 2026-08-04 -->").unwrap();
        writeln!(f, "- [x] Another <!-- id:: 2 done_at:: 2026-08-06 -->").unwrap();

        // 3. Grades
        let mut f = File::create(self.path.join("_AI/tracking/grade-returns.json")).unwrap();
        writeln!(f, r#"{{
            "returns": [
                {{"course": "CS 101", "assignment": "Lab 1", "grade": 90.0, "max_points": 100.0, "detected_at": "2026-08-10T10:00:00+08:00"}},
                {{"course": "CS 101", "assignment": "Exam 1", "grade": 85.0, "max_points": 100.0, "detected_at": "2026-08-11T12:00:00+08:00"}}
            ]
        }}"#).unwrap();

        let mut f = File::create(self.path.join("02_AREAS/academics/CS101/Grading.md")).unwrap();
        writeln!(f, "| Category | Weight |").unwrap();
        writeln!(f, "|---|---|").unwrap();
        writeln!(f, "| Exams | 100% |").unwrap();

        // 4. Attendance
        let mut f = File::create(self.path.join("_AI/tracking/attendance.md")).unwrap();
        writeln!(f, "# Attendance Log").unwrap();
        writeln!(f, "## Absences").unwrap();
        writeln!(f, "- 2026-08-05 | CS 101 | lecture |<!-- id:: 1 -->").unwrap();

        let mut f = File::create(self.path.join("02_AREAS/academics/CS101/CourseGuide.md")).unwrap();
        writeln!(f, "Attendance rules: 3 lecture absences limit").unwrap();

        // 5. Deadlines
        let mut f = File::create(self.path.join("_AI/inbox/classroom-data.json")).unwrap();
        writeln!(f, r#"{{
            "courses": [
                {{
                    "name": "CS 101",
                    "assignments": [
                        {{
                            "id": "1",
                            "title": "Exercise 1",
                            "due": "2026-08-15T15:59:59Z",
                            "max_points": 100.0,
                            "submitted": false,
                            "submission_state": "TURNED_IN"
                        }}
                    ]
                }}
            ]
        }}"#).unwrap();

        // 6. Sleep Log
        let mut f = File::create(self.path.join("_AI/tracking/sleep-log.csv")).unwrap();
        writeln!(f, "date,wake_time,bed_time,notes").unwrap();
        writeln!(f, "2026-08-04,07:00,23:00,Good sleep").unwrap();

        // 7. No Class Days (Holidays)
        let mut f = File::create(self.path.join("02_AREAS/academics/NoClassDays.md")).unwrap();
        writeln!(f, "## Official Holidays & Breaks (no classes)").unwrap();
        writeln!(f, "| Date | Day | Event |").unwrap();
        writeln!(f, "|---|---|---|").unwrap();
        writeln!(f, "| 2026-08-21 | Fri | Ninoy Aquino Day |").unwrap();

        // 8. Schedule (ICS)
        let mut f = File::create(self.path.join("_AI/inbox/schedule.ics")).unwrap();
        writeln!(f, "BEGIN:VCALENDAR").unwrap();
        writeln!(f, "BEGIN:VEVENT").unwrap();
        writeln!(f, "SUMMARY:[LEC] 101 ST").unwrap();
        writeln!(f, "DTSTART:20260804T000000Z").unwrap();
        writeln!(f, "DTEND:20260804T010000Z").unwrap();
        writeln!(f, "RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20260818T000000Z").unwrap();
        writeln!(f, "END:VEVENT").unwrap();
        writeln!(f, "END:VCALENDAR").unwrap();

        // 9. Exams
        let mut f = File::create(self.path.join("02_AREAS/academics/ExamSeasons.md")).unwrap();
        writeln!(f, "| Course | Exam Type | Est. Week | Dates | Source |").unwrap();
        writeln!(f, "|---|---|---|---|---|").unwrap();
        writeln!(f, "| CS 101 | Lecture Exam 1 | Week 5 | Sep 3 (Wed) | guide |").unwrap();

        // 10. Org Events
        let mut f = File::create(self.path.join("02_AREAS/org/COSS/events/RecruitmentCycle-2026-27.md")).unwrap();
        writeln!(f, "## Timeline").unwrap();
        writeln!(f, "| Date | Day | Activities |").unwrap();
        writeln!(f, "|---|---|---|").unwrap();
        writeln!(f, "| Aug 12 | Wed | **Orientation** |").unwrap();
    }
}

#[test]
fn test_integration_mock_vault_read() {
    let mock = MockVault::create();

    // Assert GitHub
    let git = read_github_contributions(&mock.path.join("_AI/tracking/github-contributions.csv")).unwrap();
    assert_eq!(git.len(), 2);

    // Assert Todos
    let todos = read_todo_completions(&mock.path.join("_AI/todos/todos.md")).unwrap();
    assert_eq!(todos.len(), 2);

    // Assert Grades
    let grades = read_grade_returns(
        &mock.path.join("_AI/tracking/grade-returns.json"),
        &mock.path.join("02_AREAS/academics")
    ).unwrap();
    assert_eq!(grades.len(), 2);

    // Assert Attendance
    let att = get_attendance_status(
        &mock.path.join("_AI/tracking/attendance.md"),
        &mock.path.join("02_AREAS/academics")
    ).unwrap();
    assert_eq!(att.len(), 1);
    assert_eq!(att[0].course, "CS 101");
    assert_eq!(att[0].lecture_absences, 1);

    // Assert Deadlines
    let dl = read_classroom_deadlines(&mock.path.join("_AI/inbox/classroom-data.json")).unwrap();
    assert_eq!(dl.len(), 1);
    assert_eq!(dl[0].title, "Exercise 1");

    // Assert Holidays
    let holidays = load_holiday_days(&mock.path.join("02_AREAS/academics/NoClassDays.md"));
    assert!(holidays.contains(&NaiveDate::from_ymd_opt(2026, 8, 21).unwrap()));

    // Assert Schedule
    let sched = expand_schedule(
        &mock.path.join("_AI/inbox/schedule.ics"),
        NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(),
        NaiveDate::from_ymd_opt(2026, 8, 20).unwrap()
    ).unwrap();
    assert_eq!(sched.len(), 3);

    // Assert Exams
    let exams = read_exam_events(&mock.path.join("02_AREAS/academics/ExamSeasons.md")).unwrap();
    assert_eq!(exams.len(), 1);
    assert_eq!(exams[0].course, "CS 101");

    // Assert Org Events
    let org = read_org_events(&mock.path.join("02_AREAS/org/COSS/events")).unwrap();
    assert_eq!(org.len(), 1);
    assert_eq!(org[0].label, "Orientation");
}
