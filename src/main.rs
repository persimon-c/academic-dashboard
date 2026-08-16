pub mod data;

use std::collections::HashSet;
use std::path::PathBuf;
use clap::Parser;
use chrono::{Local, FixedOffset, NaiveDate, Days, Datelike};
use crossterm::event::{self, Event, KeyCode};
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

#[derive(Parser, Debug)]
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

struct AppState {
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
    active_tab: ActiveTab,
    selected_date: NaiveDate,
    error_message: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Paths
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

    // ── Load data ──────────────────────────────────────────────────────────────
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

    // ── Streak ─────────────────────────────────────────────────────────────────
    let holiday_days = load_holiday_days(&no_class_path);
    let semester_start = NaiveDate::parse_from_str(SEMESTER_START_DATE, "%Y-%m-%d")
        .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());
    let today = Local::now().date_naive();
    let streak = compute_streak(&active_days, &holiday_days, semester_start, today);

    // ── Personal Bests ─────────────────────────────────────────────────────────
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

    // ── Class Schedule ─────────────────────────────────────────────────────────
    // Expand class occurrences for the semester bounds (August 3 to December 7, 2026)
    let sem_end = NaiveDate::from_ymd_opt(2026, 12, 7).unwrap();
    let class_schedule = match expand_schedule(&ics_path, semester_start, sem_end) {
        Ok(s) => s,
        Err(e) => { push_error(format!("Schedule ICS: {}", e)); vec![] }
    };

    // ── Exams ──────────────────────────────────────────────────────────────────
    let exam_events = match read_exam_events(&exams_path) {
        Ok(e) => e,
        Err(e) => { push_error(format!("Exams: {}", e)); vec![] }
    };

    // ── Org events ─────────────────────────────────────────────────────────────
    let org_events = match read_org_events(&org_dir) {
        Ok(o) => o,
        Err(e) => { push_error(format!("Org: {}", e)); vec![] }
    };

    let state = AppState {
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
        active_tab: ActiveTab::WeekGrid,
        selected_date: today,
        error_message,
    };

    // ── TUI setup ──────────────────────────────────────────────────────────────
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, state);

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res { eprintln!("Error: {:?}", err); }
    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut state: AppState,
) -> std::io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &state))?;
        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
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
        }
    }
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
    let streak_icon = if state.streak.current_streak > 0 { "🔥" } else { "  " };
    let freeze_indicator = if !state.streak.freeze_days.is_empty() { " ❄" } else { "" };
    
    let tab_grid_style = if state.active_tab == ActiveTab::WeekGrid {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let tab_cal_style = if state.active_tab == ActiveTab::MonthCalendar {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let header_spans = vec![
        Span::styled(" Academic Dashboard ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" │ "),
        Span::styled(" [1] Week Grid ", tab_grid_style),
        Span::raw("   "),
        Span::styled(" [2] Month Calendar ", tab_cal_style),
        Span::raw("  (Tab to Toggle) "),
        Span::raw(" │ "),
        Span::raw(format!("{} {}{} day streak (best: {}){} ", 
            streak_icon, 
            state.streak.current_streak, 
            state.streak.best_streak, 
            freeze_indicator,
            if !state.streak.holiday_protected_days.is_empty() { " [protected]" } else { "" }
        )),
    ];

    let header = Paragraph::new(Line::from(header_spans))
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(header, outer[0]);

    // 2. Main body
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(outer[1]);

    // Left pane: conditional on ActiveTab
    match state.active_tab {
        ActiveTab::WeekGrid => {
            // Week Grid (3 rows × 6 cols = 18 weeks)
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
                    let git_val  = state.weekly_github[wi];
                    let todo_val = state.weekly_todos[wi];
                    let grad_val = state.weekly_grades[wi];

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
            // Split Calendar area into Calendar Grid (60%) and Selected Day Details (40%)
            let cal_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(body[0]);

            // Draw month grid
            draw_month_calendar(f, state, cal_split[0]);
            
            // Draw details of the selected date
            draw_day_details(f, state, cal_split[1]);
        }
    }

    // Right Sidebar: Attendance + Deadlines + Sleep
    let sidebar = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35), // Attendance
            Constraint::Percentage(40), // Deadlines
            Constraint::Percentage(25), // Sleep
        ])
        .split(body[1]);

    // Attendance
    let mut att_lines = Vec::new();
    if state.attendance_status.is_empty() {
        att_lines.push(Line::from(Span::styled("No absences logged", Style::default().fg(Color::Green))));
    } else {
        for s in &state.attendance_status {
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
    let pending: Vec<&Deadline> = state.upcoming_deadlines
        .iter()
        .filter(|d| !d.submitted && d.submission_state != "returned")
        .take(5)
        .collect();

    if pending.is_empty() {
        dl_lines.push(Line::from(Span::styled("No pending deadlines 🎉", Style::default().fg(Color::Green))));
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
    let last_sleep: Vec<&SleepEntry> = state.sleep_entries.iter().rev().take(3).collect();
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

    // 3. Personal Bests
    let bests_text = Line::from(vec![
        Span::styled("🏆 PERSONAL BESTS  ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("  🔥 Best streak: "),
        Span::styled(format!("{} days", state.streak.best_streak), Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
        Span::raw("  │  🟢 Best git week: "),
        Span::styled(format!("{} commits (W{})", state.bests.best_github_week, state.bests.best_github_week_num), Style::default().fg(Color::Green)),
        Span::raw("  │  ✅ Best todo week: "),
        Span::styled(format!("{} done (W{})", state.bests.best_todo_week, state.bests.best_todo_week_num), Style::default().fg(Color::Yellow)),
    ]);
    f.render_widget(
        Paragraph::new(bests_text).block(Block::default().borders(Borders::ALL).title("Hall of Fame")),
        outer[2],
    );

    // 4. Status Bar
    let status_text = if let Some(ref err) = state.error_message {
        Line::from(vec![Span::styled("⚠ ", Style::default().fg(Color::Red)), Span::raw(err)])
    } else {
        Line::from(vec![
            Span::styled("● SYSTEM HEALTHY", Style::default().fg(Color::Green)),
            Span::raw("  Tab: Toggle View  │  Arrows: Move Selection  │  Press 'q' to Exit"),
        ])
    };
    f.render_widget(
        Paragraph::new(status_text).block(Block::default().borders(Borders::ALL).title("Logs")),
        outer[3],
    );
}

/// Render a monthly calendar calendar grid based on the selected day
fn draw_month_calendar(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect) {
    let sel = state.selected_date;
    // Determine the month bounds
    let year = sel.year();
    let month = sel.month();
    
    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };
    let last_of_month = next_month.pred_opt().unwrap();
    
    // Day of week of the first day (0-indexed starting at Monday)
    let start_col = first_of_month.weekday().num_days_from_monday();

    // Render calendar grid layout (6 rows, 7 columns)
    let calendar_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Weekday headers
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
        ])
        .split(area);

    // Render Weekday Header labels
    let days_header = Line::from(vec![
        Span::raw(" Mon   Tue   Wed   Thu   Fri   Sat   Sun "),
    ]);
    f.render_widget(Paragraph::new(days_header), calendar_rows[0]);

    let mut current_date = first_of_month;
    let mut grid_index = 0;
    
    // We walk up to 42 spots (6 rows * 7 cols)
    for r in 0..6 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 7); 7])
            .split(calendar_rows[r + 1]);

        for c in 0..7 {
            if grid_index < start_col || current_date > last_of_month {
                // Empty boundary cell
                f.render_widget(
                    Paragraph::new("").block(Block::default().borders(Borders::NONE)),
                    cols[c],
                );
            } else {
                // This is a calendar day. Let's see what events fall on this day
                let has_classes = state.class_schedule.iter().any(|b| b.date == current_date);
                let has_exams = state.exam_events.iter().any(|e| e.date == Some(current_date));
                let has_org = state.org_events.iter().any(|o| o.date == current_date);
                let has_deadline = state.upcoming_deadlines.iter().any(|d| d.due.date_naive() == current_date);

                // Construct text labels inside the day cell
                let mut cell_style = Style::default();
                if current_date == sel {
                    // Highlight selected date
                    cell_style = cell_style.bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD);
                } else if current_date == Local::now().date_naive() {
                    // Highlight today
                    cell_style = cell_style.fg(Color::Yellow).add_modifier(Modifier::UNDERLINED);
                }

                let mut indicators = Vec::new();
                if has_exams {
                    indicators.push(Span::styled(" [E]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
                }
                if has_deadline {
                    indicators.push(Span::styled(" [D]", Style::default().fg(Color::Cyan)));
                }
                if has_classes {
                    indicators.push(Span::styled(" [C]", Style::default().fg(Color::Blue)));
                }
                if has_org {
                    indicators.push(Span::styled(" [O]", Style::default().fg(Color::Yellow)));
                }

                let title = format!(" {:02}", current_date.day());
                let cell_block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(if current_date == sel { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::DarkGray) })
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

/// Render details for the selected day in a dedicated side panel
fn draw_day_details(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect) {
    let sel = state.selected_date;
    let mut lines = Vec::new();

    // 1. Header of details panel
    lines.push(Line::from(vec![
        Span::styled(format!("📅 DETAIL VIEW: {} ", sel.format("%A, %B %d, %Y")), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from("─".repeat(area.width as usize)));

    // 2. Add classes on this day
    let day_classes: Vec<&ClassBlock> = state.class_schedule.iter()
        .filter(|b| b.date == sel)
        .collect();

    lines.push(Line::from(Span::styled("🏫 Scheduled Classes:", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD))));
    if day_classes.is_empty() {
        lines.push(Line::from("  No classes scheduled"));
    } else {
        for c in day_classes {
            lines.push(Line::from(vec![
                Span::raw("  ● "),
                Span::styled(format!("{}-{} ", c.start_time.format("%H:%M"), c.end_time.format("%H:%M")), Style::default().fg(Color::DarkGray)),
                Span::styled(c.summary.clone(), Style::default().fg(Color::White)),
                Span::raw(" @ "),
                Span::styled(c.location.clone(), Style::default().fg(Color::Gray)),
            ]));
        }
    }
    lines.push(Line::from(""));

    // 3. Add exam events on this day
    let day_exams: Vec<&ExamEvent> = state.exam_events.iter()
        .filter(|e| e.date == Some(sel))
        .collect();

    lines.push(Line::from(Span::styled("📝 Exam Events:", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))));
    if day_exams.is_empty() {
        lines.push(Line::from("  No exams scheduled"));
    } else {
        for e in day_exams {
            lines.push(Line::from(vec![
                Span::raw("  🔥 "),
                Span::styled(format!("[{}] ", e.course), Style::default().fg(Color::Cyan)),
                Span::styled(e.label.clone(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]));
        }
    }
    lines.push(Line::from(""));

    // 4. Add org events on this day
    let day_org: Vec<&OrgEvent> = state.org_events.iter()
        .filter(|o| o.date == sel)
        .collect();

    lines.push(Line::from(Span::styled("👥 Org Commitments [ORG]:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    if day_org.is_empty() {
        lines.push(Line::from("  No org events scheduled"));
    } else {
        for o in day_org {
            lines.push(Line::from(vec![
                Span::raw("  👥 "),
                Span::styled(o.label.clone(), Style::default().fg(Color::Yellow)),
                Span::raw(format!(" (source: {})", o.source)),
            ]));
        }
    }
    lines.push(Line::from(""));

    // 5. Add deadlines due on this day
    let day_deadlines: Vec<&Deadline> = state.upcoming_deadlines.iter()
        .filter(|d| d.due.date_naive() == sel)
        .collect();

    lines.push(Line::from(Span::styled("⏳ Classroom Deadlines:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    if day_deadlines.is_empty() {
        lines.push(Line::from("  No deadlines due"));
    } else {
        for d in day_deadlines {
            let status_span = if d.submitted {
                Span::styled(" [Done]", Style::default().fg(Color::Green))
            } else {
                Span::styled(" [Pending]", Style::default().fg(Color::Red))
            };
            lines.push(Line::from(vec![
                Span::raw("  ⏳ "),
                Span::styled(format!("[{}] ", d.course), Style::default().fg(Color::Cyan)),
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
