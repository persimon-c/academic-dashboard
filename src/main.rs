pub mod data;

use std::collections::HashSet;
use std::path::PathBuf;
use std::thread;
use std::time::Duration as StdDuration;
use clap::Parser;
use chrono::{Local, FixedOffset, NaiveDate, Days, Datelike};
use crossterm::event::{self, Event, KeyCode};
use crossbeam_channel::{unbounded, Receiver, Sender};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

use crate::data::calendar::{date_to_canonical_week, SEMESTER_START_DATE};
use crate::data::github::read_github_contributions;
use crate::data::todos::read_todo_completions;
use crate::data::grades::read_grade_returns;
use crate::data::attendance::{get_attendance_status, CourseAttendanceStatus};
use crate::data::deadlines::{read_classroom_deadlines, Deadline};
use crate::data::streak::{compute_streak, load_holiday_days, StreakResult};
use crate::data::sleep::{read_sleep_log, SleepEntry};
use crate::data::rrule::{expand_schedule, ClassBlock};
use crate::data::exams::{read_exam_events, ExamEvent};
use crate::data::org::{read_org_events, OrgEvent};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "/home/simone/smon-os")]
    vault_path: PathBuf,
}

struct PersonalBests {
    best_github_week: u32,
    best_github_week_num: usize,
    best_todo_week: u32,
    best_todo_week_num: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    WeekGrid,
    MonthCalendar,
}

/// Dynamic components reloaded in the background sync thread
struct LoadedData {
    weekly_github: [u32; 18],
    weekly_todos: [u32; 18],
    weekly_grades: [u32; 18],
    attendance_status: Vec<CourseAttendanceStatus>,
    upcoming_deadlines: Vec<Deadline>,
    streak: StreakResult,
    sleep_entries: Vec<SleepEntry>,
    bests: PersonalBests,
    class_schedule: Vec<ClassBlock>,
    exam_events: Vec<ExamEvent>,
    org_events: Vec<OrgEvent>,
    error_message: Option<String>,
    last_synced: String,
}

enum AppEvent {
    Key(crossterm::event::KeyEvent),
    DataLoaded(LoadedData),
    Tick,
}

struct AppState {
    data: LoadedData,
    active_tab: ActiveTab,
    selected_date: NaiveDate,
}

fn load_all_data(args: &Args) -> LoadedData {
    let github_path     = args.vault_path.join("_AI/tracking/github-contributions.csv");
    let todos_path      = args.vault_path.join("_AI/todos/todos.md");
    let grades_path     = args.vault_path.join("_AI/tracking/grade-returns.json");
    let academics_dir   = args.vault_path.join("02_AREAS/academics");
    let attendance_path = args.vault_path.join("_AI/tracking/attendance.md");
    let classroom_path  = args.vault_path.join("_AI/inbox/classroom-data.json");
    let no_class_path   = args.vault_path.join("02_AREAS/academics/NoClassDays.md");
    let sleep_path      = args.vault_path.join("_AI/tracking/sleep-log.csv");
    let ics_path        = args.vault_path.join("_AI/inbox/schedule.ics");
    let exams_path      = args.vault_path.join("02_AREAS/academics/ExamSeasons.md");
    let org_dir         = args.vault_path.join("02_AREAS/org/COSS/events");

    let mut weekly_github = [0u32; 18];
    let mut weekly_todos  = [0u32; 18];
    let mut weekly_grades = [0u32; 18];
    let mut active_days: HashSet<NaiveDate> = HashSet::new();
    let mut error_message: Option<String> = None;

    let mut push_error = |msg: String| {
        if let Some(ref mut existing) = error_message {
            existing.push_str(&format!(" | {}", msg));
        } else {
            error_message = Some(msg);
        }
    };

    match read_github_contributions(&github_path) {
        Ok(metrics) => {
            for m in &metrics {
                active_days.insert(m.date);
                if let Some(w) = date_to_canonical_week(m.date) {
                    if m.value > 0 { weekly_github[(w - 1) as usize] += m.value; }
                }
            }
        }
        Err(e) => push_error(format!("GitHub: {}", e)),
    }

    match read_todo_completions(&todos_path) {
        Ok(metrics) => {
            for m in &metrics {
                active_days.insert(m.date);
                if let Some(w) = date_to_canonical_week(m.date) {
                    weekly_todos[(w - 1) as usize] += m.value;
                }
            }
        }
        Err(e) => push_error(format!("Todos: {}", e)),
    }

    match read_grade_returns(&grades_path, &academics_dir) {
        Ok(metrics) => {
            for m in metrics {
                if let Some(w) = date_to_canonical_week(m.date) {
                    weekly_grades[(w - 1) as usize] += m.value;
                }
            }
        }
        Err(e) => push_error(format!("Grades: {}", e)),
    }

    let attendance_status = match get_attendance_status(&attendance_path, &academics_dir) {
        Ok(s) => s,
        Err(e) => { push_error(format!("Attendance: {}", e)); vec![] }
    };

    let upcoming_deadlines = match read_classroom_deadlines(&classroom_path) {
        Ok(d) => d,
        Err(e) => { push_error(format!("Deadlines: {}", e)); vec![] }
    };

    let sleep_entries = match read_sleep_log(&sleep_path) {
        Ok(s) => s,
        Err(_) => vec![],
    };

    let holiday_days = load_holiday_days(&no_class_path);
    let semester_start = NaiveDate::parse_from_str(SEMESTER_START_DATE, "%Y-%m-%d")
        .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());
    let today = Local::now().date_naive();
    let streak = compute_streak(&active_days, &holiday_days, semester_start, today);

    let (best_github_week, best_github_week_num) = weekly_github
        .iter().enumerate()
        .max_by_key(|(_, v)| *v)
        .map(|(i, &v)| (v, i + 1))
        .unwrap_or((0, 0));

    let (best_todo_week, best_todo_week_num) = weekly_todos
        .iter().enumerate()
        .max_by_key(|(_, v)| *v)
        .map(|(i, &v)| (v, i + 1))
        .unwrap_or((0, 0));

    let bests = PersonalBests {
        best_github_week,
        best_github_week_num,
        best_todo_week,
        best_todo_week_num,
    };

    let sem_end = NaiveDate::from_ymd_opt(2026, 12, 7).unwrap();
    let class_schedule = match expand_schedule(&ics_path, semester_start, sem_end) {
        Ok(s) => s,
        Err(e) => { push_error(format!("Schedule ICS: {}", e)); vec![] }
    };

    let exam_events = match read_exam_events(&exams_path) {
        Ok(e) => e,
        Err(e) => { push_error(format!("Exams: {}", e)); vec![] }
    };

    let org_events = match read_org_events(&org_dir) {
        Ok(o) => o,
        Err(e) => { push_error(format!("Org: {}", e)); vec![] }
    };

    LoadedData {
        weekly_github,
        weekly_todos,
        weekly_grades,
        attendance_status,
        upcoming_deadlines,
        streak,
        sleep_entries,
        bests,
        class_schedule,
        exam_events,
        org_events,
        error_message,
        last_synced: Local::now().format("%H:%M:%S").to_string(),
    }
}

/// Detects if an exam overlaps with or is within 2 days of an org event.
fn has_exam_org_conflict(d: NaiveDate, exams: &[ExamEvent], orgs: &[OrgEvent]) -> bool {
    let has_exam_near = exams.iter().any(|e| {
        if let Some(ed) = e.date {
            (ed - d).num_days().abs() <= 2
        } else {
            false
        }
    });
    let has_org_near = orgs.iter().any(|o| (o.date - d).num_days().abs() <= 2);
    has_exam_near && has_org_near
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let initial_data = load_all_data(&args);

    let mut state = AppState {
        data: initial_data,
        active_tab: ActiveTab::WeekGrid,
        selected_date: Local::now().date_naive(),
    };

    // ── Channel infrastructure ──────────────────────────────────────────────────
    let (tx, rx): (Sender<AppEvent>, Receiver<AppEvent>) = unbounded();

    // 1. Spawning terminal key input thread
    let tx_keys = tx.clone();
    thread::spawn(move || {
        loop {
            if event::poll(StdDuration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    let _ = tx_keys.send(AppEvent::Key(key));
                }
            }
        }
    });

    // 2. Spawning ticker thread (for clean dynamic UI refresh)
    let tx_tick = tx.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(StdDuration::from_millis(1000));
            let _ = tx_tick.send(AppEvent::Tick);
        }
    });

    // 3. Spawning non-blocking file sync background thread (polls every 30s)
    let tx_sync = tx.clone();
    let sync_args = args.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(StdDuration::from_secs(30));
            let fresh_data = load_all_data(&sync_args);
            let _ = tx_sync.send(AppEvent::DataLoaded(fresh_data));
        }
    });

    // ── TUI setup ──────────────────────────────────────────────────────────────
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| ui(f, &state))?;
        
        match rx.recv() {
            Ok(AppEvent::Key(key)) => {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Tab => {
                        state.active_tab = match state.active_tab {
                            ActiveTab::WeekGrid => ActiveTab::MonthCalendar,
                            ActiveTab::MonthCalendar => ActiveTab::WeekGrid,
                        };
                    }
                    KeyCode::Left => {
                        if state.active_tab == ActiveTab::MonthCalendar {
                            state.selected_date = state.selected_date.pred_opt().unwrap_or(state.selected_date);
                        }
                    }
                    KeyCode::Right => {
                        if state.active_tab == ActiveTab::MonthCalendar {
                            state.selected_date = state.selected_date.succ_opt().unwrap_or(state.selected_date);
                        }
                    }
                    KeyCode::Up => {
                        if state.active_tab == ActiveTab::MonthCalendar {
                            state.selected_date = state.selected_date.checked_sub_days(Days::new(7)).unwrap_or(state.selected_date);
                        }
                    }
                    KeyCode::Down => {
                        if state.active_tab == ActiveTab::MonthCalendar {
                            state.selected_date = state.selected_date.checked_add_days(Days::new(7)).unwrap_or(state.selected_date);
                        }
                    }
                    _ => {}
                }
            }
            Ok(AppEvent::DataLoaded(fresh_data)) => {
                state.data = fresh_data;
            }
            Ok(AppEvent::Tick) => {
                // Just triggers redrawing to update clock / sync timing
            }
            Err(_) => break,
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui(f: &mut ratatui::Frame, state: &AppState) {
    let size = f.area();

    // Outer layout: Header / Body / Personal Bests / Status
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(12),    // Main body
            Constraint::Length(3),  // Personal bests
            Constraint::Length(3),  // Status / logs
        ])
        .split(size);

    // 1. Header with Tab indicators
    // Nerd Font icons: \uf06d = fire, \uf2dc = snowflake
    let streak_icon = if state.data.streak.current_streak > 0 { "\u{f06d}" } else { " " };
    let freeze_indicator = if !state.data.streak.freeze_days.is_empty() { " \u{f2dc} freeze" } else { "" };

    let tab_grid_style = if state.active_tab == ActiveTab::WeekGrid {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let tab_cal_style = if state.active_tab == ActiveTab::MonthCalendar {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let header_spans = vec![
        Span::styled(" \u{f02d} Academic Dashboard ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" \u{e0b1} "),
        Span::styled(" \u{f009} Week Grid ", tab_grid_style),
        Span::raw("   "),
        Span::styled(" \u{f073} Month Calendar ", tab_cal_style),
        Span::raw(" \u{e0b1} "),
        Span::raw(format!("{} {} day streak  best: {}{}{} ",
            streak_icon,
            state.data.streak.current_streak,
            state.data.streak.best_streak,
            freeze_indicator,
            if !state.data.streak.holiday_protected_days.is_empty() { " \u{f0c2} protected" } else { "" }
        )),
    ];

    let header = Paragraph::new(Line::from(header_spans))
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(header, outer[0]);

    // 2. Main body — 82% main area, 18% narrow sidebar
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(82), Constraint::Percentage(18)])
        .split(outer[1]);

    // Left pane: conditional on ActiveTab
    match state.active_tab {
        ActiveTab::WeekGrid => {
            let grid_rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Ratio(1, 3); 3])
                .split(body[0]);

            for row in 0..3usize {
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Ratio(1, 6); 6])
                    .split(grid_rows[row]);

                for col in 0..6usize {
                    let wi = row * 6 + col;
                    let week_num = wi + 1;
                    let git_val  = state.data.weekly_github[wi];
                    let todo_val = state.data.weekly_todos[wi];
                    let grad_val = state.data.weekly_grades[wi];

                    let has_activity = git_val > 0 || todo_val > 0 || grad_val > 0;
                    let title_style = if has_activity {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let content = vec![
                        Line::from(vec![
                            Span::raw("Git  "),
                            Span::styled(format!("{:>4}", git_val), Style::default().fg(Color::Green)),
                        ]),
                        Line::from(vec![
                            Span::raw("Todo "),
                            Span::styled(format!("{:>4}", todo_val), Style::default().fg(Color::Yellow)),
                        ]),
                        Line::from(vec![
                            Span::raw("Grad "),
                            Span::styled(format!("{:>4}", grad_val), Style::default().fg(Color::Magenta)),
                        ]),
                    ];

                    let block = Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" W{:02} ", week_num))
                        .title_style(title_style);

                    f.render_widget(Paragraph::new(content).block(block), cols[col]);
                }
            }
        }
        ActiveTab::MonthCalendar => {
            // 60% for the calendar grid, 40% for the day detail panel
            let cal_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(body[0]);

            draw_month_calendar(f, state, cal_split[0]);
            draw_day_details(f, state, cal_split[1]);
        }
    }

    // Right Sidebar
    let sidebar = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(40),
            Constraint::Percentage(25),
        ])
        .split(body[1]);

    // Attendance
    let mut att_lines = Vec::new();
    if state.data.attendance_status.is_empty() {
        att_lines.push(Line::from(Span::styled("No absences logged", Style::default().fg(Color::Green))));
    } else {
        for s in &state.data.attendance_status {
            let lec_pct = if s.lecture_limit > 0 { s.lecture_absences as f32 / s.lecture_limit as f32 } else { 0.0 };
            let lec_col = if lec_pct >= 0.75 { Color::Red } else if lec_pct >= 0.5 { Color::Yellow } else { Color::Green };
            att_lines.push(Line::from(vec![
                Span::styled(format!("{:<8}", &s.course[..s.course.len().min(8)]), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" L:"),
                Span::styled(format!("{}/{}", s.lecture_absences, s.lecture_limit), Style::default().fg(lec_col)),
                Span::raw("  lb:"),
                Span::styled(format!("{}/{}", s.lab_absences, s.lab_limit), Style::default().fg(Color::Gray)),
            ]));
        }
    }
    f.render_widget(
        Paragraph::new(att_lines).block(Block::default().borders(Borders::ALL).title("Attendance")),
        sidebar[0],
    );

    // Deadlines
    let now_manila = Local::now().with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap());
    let mut dl_lines = Vec::new();
    let pending: Vec<&Deadline> = state.data.upcoming_deadlines
        .iter()
        .filter(|d| !d.submitted && d.submission_state != "returned")
        .take(5)
        .collect();

    if pending.is_empty() {
        dl_lines.push(Line::from(Span::styled("\u{f058} All clear!", Style::default().fg(Color::Green))));
    } else {
        for d in pending {
            let days_left = d.due.signed_duration_since(now_manila).num_days();
            let col = if days_left < 0 { Color::Red } else if days_left < 3 { Color::LightRed } else if days_left < 7 { Color::Yellow } else { Color::Gray };
            let course_short = d.course.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            let title_short = if d.title.len() > 14 { format!("{}…", &d.title[..13]) } else { d.title.clone() };
            dl_lines.push(Line::from(vec![
                Span::styled(format!("[{:<7}] ", course_short), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:<15} ", title_short), Style::default()),
                Span::styled(format!("{}d", days_left), Style::default().fg(col)),
            ]));
        }
    }
    f.render_widget(
        Paragraph::new(dl_lines).block(Block::default().borders(Borders::ALL).title("Deadlines")),
        sidebar[1],
    );

    // Sleep
    let last_sleep: Vec<&SleepEntry> = state.data.sleep_entries.iter().rev().take(3).collect();
    let mut sleep_lines = Vec::new();
    if last_sleep.is_empty() {
        sleep_lines.push(Line::from(Span::raw("No sleep data")));
    } else {
        for e in last_sleep.into_iter().rev() {
            let wake = e.wake_time.map(|t| format!("{}", t.format("%H:%M"))).unwrap_or_else(|| "??:??".to_string());
            let bed  = e.bed_time .map(|t| format!("{}", t.format("%H:%M"))).unwrap_or_else(|| "??:??".to_string());
            let dur  = e.duration_hours().map(|h| format!("{:.1}h", h)).unwrap_or_else(|| "?".to_string());
            let col  = e.duration_hours().map(|h| if h < 6.0 { Color::Red } else if h < 7.5 { Color::Yellow } else { Color::Green }).unwrap_or(Color::Gray);
            sleep_lines.push(Line::from(vec![
                Span::styled(format!("{} ", e.date.format("%m-%d")), Style::default().fg(Color::DarkGray)),
                Span::raw(format!("↑{} ↓{} ", wake, bed)),
                Span::styled(dur, Style::default().fg(col)),
            ]));
        }
    }
    f.render_widget(
        Paragraph::new(sleep_lines).block(Block::default().borders(Borders::ALL).title("Sleep")),
        sidebar[2],
    );

    // 3. Personal Bests — \uf091 = trophy, \uf06d = fire, \uf09b = github, \uf0ae = tasks
    let bests_text = Line::from(vec![
        Span::styled("\u{f091} PERSONAL BESTS ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("  \u{f06d} streak: "),
        Span::styled(format!("{} days", state.data.streak.best_streak), Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
        Span::raw("  \u{e0b1}  \u{f09b} git: "),
        Span::styled(format!("{} commits (W{})", state.data.bests.best_github_week, state.data.bests.best_github_week_num), Style::default().fg(Color::Green)),
        Span::raw("  \u{e0b1}  \u{f0ae} todos: "),
        Span::styled(format!("{} done (W{})", state.data.bests.best_todo_week, state.data.bests.best_todo_week_num), Style::default().fg(Color::Yellow)),
    ]);
    f.render_widget(
        Paragraph::new(bests_text).block(Block::default().borders(Borders::ALL).title("\u{f091} Hall of Fame")),
        outer[2],
    );

    // 4. Status Bar — \uf071 = warning, \uf058 = check circle, \uf017 = clock
    let status_text = if let Some(ref err) = state.data.error_message {
        Line::from(vec![Span::styled("\u{f071} ", Style::default().fg(Color::Red)), Span::raw(err)])
    } else {
        Line::from(vec![
            Span::styled("\u{f058} HEALTHY", Style::default().fg(Color::Green)),
            Span::raw(format!("  \u{f017} Sync: {}  \u{e0b1}  Tab: Toggle View  \u{e0b1}  Arrows: Select Day  \u{e0b1}  q: Exit", state.data.last_synced)),
        ])
    };
    f.render_widget(
        Paragraph::new(status_text).block(Block::default().borders(Borders::ALL).title("\u{f108} Logs")),
        outer[3],
    );
}

fn draw_month_calendar(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect) {
    let sel = state.selected_date;
    let year = sel.year();
    let month = sel.month();
    
    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };
    let last_of_month = next_month.pred_opt().unwrap();
    
    let start_col = first_of_month.weekday().num_days_from_sunday();

    let calendar_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
        ])
        .split(area);

    // Render weekday headers using the same 7-column ratio layout as the cells
    // so labels are guaranteed to align at any terminal width.
    let header_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 7); 7])
        .split(calendar_rows[0]);
    for (i, day_name) in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].iter().enumerate() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {}", day_name),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ))),
            header_cols[i],
        );
    }

    let mut current_date = first_of_month;
    let mut grid_index = 0;
    
    for r in 0..6 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 7); 7])
            .split(calendar_rows[r + 1]);

        for c in 0..7 {
            if grid_index < start_col || current_date > last_of_month {
                f.render_widget(
                    Paragraph::new("").block(Block::default().borders(Borders::NONE)),
                    cols[c],
                );
            } else {
                let has_classes = state.data.class_schedule.iter().any(|b| b.date == current_date);
                let has_exams = state.data.exam_events.iter().any(|e| e.date == Some(current_date));
                let has_org = state.data.org_events.iter().any(|o| o.date == current_date);
                let has_deadline = state.data.upcoming_deadlines.iter().any(|d| d.due.date_naive() == current_date);
                
                // Highlight conflict day (Exam + Org event near each other)
                let is_conflict = has_exam_org_conflict(current_date, &state.data.exam_events, &state.data.org_events);

                let today = Local::now().date_naive();
                let is_weekend = matches!(current_date.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun);
                let has_any_event = has_classes || has_exams || has_org || has_deadline;

                let mut cell_style = Style::default();
                if current_date == sel {
                    cell_style = cell_style.bg(Color::Blue).fg(Color::White);
                } else if current_date == today {
                    cell_style = cell_style.bg(Color::Cyan).fg(Color::Black);
                } else if is_conflict {
                    cell_style = cell_style.fg(Color::Red);
                }

                // Determine contrasting style for indicators when cell has a filled background
                let is_filled_bg = current_date == sel || current_date == today;
                let get_indicator_style = |default_color: Color| {
                    if current_date == sel {
                        Style::default().fg(Color::White) // white indicators on blue background
                    } else if current_date == today {
                        Style::default().fg(Color::Black) // black indicators on cyan background
                    } else {
                        Style::default().fg(default_color)
                    }
                };

                let mut indicators = Vec::new();
                if is_conflict {
                    indicators.push(Span::styled(" \u{f071}", get_indicator_style(Color::Red)));
                } else {
                    if has_exams {
                        indicators.push(Span::styled(" \u{f040}", get_indicator_style(Color::Red)));
                    }
                    if has_deadline {
                        indicators.push(Span::styled(" \u{f017}", get_indicator_style(Color::Cyan)));
                    }
                    if has_classes {
                        indicators.push(Span::styled(" \u{f19d}", get_indicator_style(Color::Blue)));
                    }
                    if has_org {
                        indicators.push(Span::styled(" \u{f0c0}", get_indicator_style(Color::Yellow)));
                    }
                }

                let title = format!(" {:02}", current_date.day());
                let border_color = if is_conflict && current_date != sel {
                    Color::Red
                } else if current_date == sel {
                    Color::Yellow
                } else if current_date == today {
                    Color::Cyan          // today always pops, even unselected
                } else if has_any_event {
                    Color::White         // days with something on them stand out
                } else if is_weekend {
                    Color::Blue          // weekends subtly tinted
                } else if current_date < today {
                    Color::DarkGray      // past: fade out
                } else {
                    Color::Gray          // future empty: neutral
                };
                let cell_block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title(title);

                f.render_widget(
                    Paragraph::new(Line::from(indicators))
                        .block(cell_block)
                        .style(cell_style),
                    cols[c],
                );

                current_date = current_date.succ_opt().unwrap_or(current_date);
            }
            grid_index += 1;
        }
    }
}

fn draw_day_details(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect) {
    let sel = state.selected_date;
    let mut lines = Vec::new();

    // \uf073 = calendar
    lines.push(Line::from(vec![
        Span::styled("\u{f073}  ", Style::default().fg(Color::Yellow)),
        Span::styled(format!("{}", sel.format("%A, %B %d, %Y")), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from("─".repeat(area.width as usize)));

    // Warning banner for conflicts
    // \uf071 = warning triangle
    if has_exam_org_conflict(sel, &state.data.exam_events, &state.data.org_events) {
        lines.push(Line::from(Span::styled(
            "\u{f071}  CONFLICT: Org event clashes with exam prep!",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from("─".repeat(area.width as usize)));
    }

    // Classes
    let day_classes: Vec<&ClassBlock> = state.data.class_schedule.iter()
        .filter(|b| b.date == sel)
        .collect();

    // \uf19d = graduation cap
    lines.push(Line::from(vec![
        Span::styled("\u{f19d}  ", Style::default().fg(Color::Blue)),
        Span::styled("Classes", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
    ]));
    if day_classes.is_empty() {
        lines.push(Line::from("  No classes scheduled"));
    } else {
        for c in day_classes {
            lines.push(Line::from(vec![
                Span::raw("  \u{f101} "),
                Span::styled(format!("{}-{} ", c.start_time.format("%H:%M"), c.end_time.format("%H:%M")), Style::default().fg(Color::DarkGray)),
                Span::styled(c.summary.clone(), Style::default().fg(Color::White)),
                Span::raw(" @ "),
                Span::styled(c.location.clone(), Style::default().fg(Color::Gray)),
            ]));
        }
    }
    lines.push(Line::from(""));

    // Exams
    let day_exams: Vec<&ExamEvent> = state.data.exam_events.iter()
        .filter(|e| e.date == Some(sel))
        .collect();

    // \uf040 = pencil/exam
    lines.push(Line::from(vec![
        Span::styled("\u{f040}  ", Style::default().fg(Color::Red)),
        Span::styled("Exams", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
    ]));
    if day_exams.is_empty() {
        lines.push(Line::from("  No exams scheduled"));
    } else {
        for e in day_exams {
            lines.push(Line::from(vec![
                Span::raw("  \u{f101} "),
                Span::styled(format!("{} ", e.course), Style::default().fg(Color::Cyan)),
                Span::styled(e.label.clone(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]));
        }
    }
    lines.push(Line::from(""));

    // Org events
    let day_org: Vec<&OrgEvent> = state.data.org_events.iter()
        .filter(|o| o.date == sel)
        .collect();

    // \uf0c0 = users/org
    lines.push(Line::from(vec![
        Span::styled("\u{f0c0}  ", Style::default().fg(Color::Yellow)),
        Span::styled("Org", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]));
    if day_org.is_empty() {
        lines.push(Line::from("  No org events"));
    } else {
        for o in day_org {
            lines.push(Line::from(vec![
                Span::raw("  \u{f101} "),
                Span::styled(o.label.clone(), Style::default().fg(Color::Yellow)),
                Span::raw(format!(" ({})", o.source)),
            ]));
        }
    }
    lines.push(Line::from(""));

    // Deadlines
    let day_deadlines: Vec<&Deadline> = state.data.upcoming_deadlines.iter()
        .filter(|d| d.due.date_naive() == sel)
        .collect();

    // \uf0ae = tasks list, \uf058 = check, \uf111 = circle dot
    lines.push(Line::from(vec![
        Span::styled("\u{f0ae}  ", Style::default().fg(Color::Cyan)),
        Span::styled("Deadlines", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]));
    if day_deadlines.is_empty() {
        lines.push(Line::from("  No deadlines due"));
    } else {
        for d in day_deadlines {
            let status_span = if d.submitted {
                Span::styled(" \u{f058}", Style::default().fg(Color::Green))
            } else {
                Span::styled(" \u{f111}", Style::default().fg(Color::Red))
            };
            lines.push(Line::from(vec![
                Span::raw("  \u{f101} "),
                Span::styled(format!("{} ", d.course), Style::default().fg(Color::Cyan)),
                Span::styled(d.title.clone(), Style::default()),
                status_span,
            ]));
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Day Information ")),
        area,
    );
}
