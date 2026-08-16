pub mod data;

use std::collections::HashSet;
use std::path::PathBuf;
use clap::Parser;
use chrono::{Local, FixedOffset, NaiveDate};
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

struct AppState {
    weekly_github: [u32; 18],
    weekly_todos: [u32; 18],
    weekly_grades: [u32; 18],
    attendance_status: Vec<CourseAttendanceStatus>,
    upcoming_deadlines: Vec<Deadline>,
    streak: StreakResult,
    sleep_entries: Vec<SleepEntry>,
    bests: PersonalBests,
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
        Err(_) => vec![], // sleep log missing is non-fatal
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

    let state = AppState {
        weekly_github,
        weekly_todos,
        weekly_grades,
        attendance_status,
        upcoming_deadlines,
        streak,
        sleep_entries,
        bests,
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
    state: AppState,
) -> std::io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &state))?;
        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') { return Ok(()); }
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, state: &AppState) {
    let size = f.area();

    // ── Outer layout: Header / Body / Personal Bests / Status ─────────────────
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(12),    // Main body
            Constraint::Length(3),  // Personal bests row
            Constraint::Length(3),  // Status / errors
        ])
        .split(size);

    // 1. Header ──────────────────────────────────────────────────────────────
    let streak_icon = if state.streak.current_streak > 0 { "🔥" } else { "  " };
    let freeze_indicator = if !state.streak.freeze_days.is_empty() { " ❄" } else { "" };
    let header_text = format!(
        " Academic Dashboard  │  {}  {}{} day streak  (best: {})  │  {}",
        Local::now().format("%Y-%m-%d %H:%M"),
        streak_icon,
        state.streak.current_streak,
        state.streak.best_streak,
        freeze_indicator,
    );
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(header, outer[0]);

    // 2. Main body: 70% Week Grid  |  30% Sidebar ────────────────────────────
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(outer[1]);

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

            // Dim past weeks with no activity
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

    // Sidebar: Attendance + Deadlines + Sleep ─────────────────────────────────
    let sidebar = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35), // attendance
            Constraint::Percentage(40), // deadlines
            Constraint::Percentage(25), // sleep (last 3 entries)
        ])
        .split(body[1]);

    // Attendance gauge
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
        Paragraph::new(att_lines)
            .block(Block::default().borders(Borders::ALL).title("Attendance")),
        sidebar[0],
    );

    // Upcoming deadlines
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
        Paragraph::new(dl_lines)
            .block(Block::default().borders(Borders::ALL).title("Deadlines")),
        sidebar[1],
    );

    // Sleep — last 3 entries
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
        Paragraph::new(sleep_lines)
            .block(Block::default().borders(Borders::ALL).title("Sleep")),
        sidebar[2],
    );

    // 3. Personal Bests row ───────────────────────────────────────────────────
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
        Paragraph::new(bests_text)
            .block(Block::default().borders(Borders::ALL).title("Hall of Fame")),
        outer[2],
    );

    // 4. Status bar ────────────────────────────────────────────────────────────
    let status_text = if let Some(ref err) = state.error_message {
        Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(Color::Red)),
            Span::raw(err),
        ])
    } else {
        Line::from(vec![
            Span::styled("● SYSTEM HEALTHY", Style::default().fg(Color::Green)),
            Span::raw("  Press 'q' to exit"),
        ])
    };
    f.render_widget(
        Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL).title("Logs")),
        outer[3],
    );
}
