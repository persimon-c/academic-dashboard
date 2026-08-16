pub mod data;

use std::path::PathBuf;
use clap::Parser;
use chrono::{Local, FixedOffset};
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

use crate::data::calendar::date_to_canonical_week;
use crate::data::github::read_github_contributions;
use crate::data::todos::read_todo_completions;
use crate::data::grades::read_grade_returns;
use crate::data::attendance::{get_attendance_status, CourseAttendanceStatus};
use crate::data::deadlines::{read_classroom_deadlines, Deadline};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "/home/simone/smon-os")]
    vault_path: PathBuf,
}

struct AppState {
    weekly_github: [u32; 18],
    weekly_todos: [u32; 18],
    weekly_grades: [u32; 18],
    attendance_status: Vec<CourseAttendanceStatus>,
    upcoming_deadlines: Vec<Deadline>,
    error_message: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Paths
    let github_path = args.vault_path.join("_AI/tracking/github-contributions.csv");
    let todos_path = args.vault_path.join("_AI/todos/todos.md");
    let grades_path = args.vault_path.join("_AI/tracking/grade-returns.json");
    let academics_dir = args.vault_path.join("02_AREAS/academics");
    let attendance_path = args.vault_path.join("_AI/tracking/attendance.md");
    let classroom_path = args.vault_path.join("_AI/inbox/classroom-data.json");

    // Load data
    let mut weekly_github = [0u32; 18];
    let mut weekly_todos = [0u32; 18];
    let mut weekly_grades = [0u32; 18];
    let mut attendance_status = Vec::new();
    let mut upcoming_deadlines = Vec::new();
    let mut error_message = None;

    match read_github_contributions(&github_path) {
        Ok(metrics) => {
            for m in metrics {
                if let Some(w) = date_to_canonical_week(m.date) {
                    if w >= 1 && w <= 18 {
                        weekly_github[(w - 1) as usize] += m.value;
                    }
                }
            }
        }
        Err(e) => {
            error_message = Some(format!("GitHub error: {}", e));
        }
    }

    match read_todo_completions(&todos_path) {
        Ok(metrics) => {
            for m in metrics {
                if let Some(w) = date_to_canonical_week(m.date) {
                    if w >= 1 && w <= 18 {
                        weekly_todos[(w - 1) as usize] += m.value;
                    }
                }
            }
        }
        Err(e) => {
            let msg = format!("Todos error: {}", e);
            if let Some(ref mut existing) = error_message {
                existing.push_str(&format!(" | {}", msg));
            } else {
                error_message = Some(msg);
            }
        }
    }

    match read_grade_returns(&grades_path, &academics_dir) {
        Ok(metrics) => {
            for m in metrics {
                if let Some(w) = date_to_canonical_week(m.date) {
                    if w >= 1 && w <= 18 {
                        weekly_grades[(w - 1) as usize] += m.value;
                    }
                }
            }
        }
        Err(e) => {
            let msg = format!("Grades error: {}", e);
            if let Some(ref mut existing) = error_message {
                existing.push_str(&format!(" | {}", msg));
            } else {
                error_message = Some(msg);
            }
        }
    }

    match get_attendance_status(&attendance_path, &academics_dir) {
        Ok(status) => {
            attendance_status = status;
        }
        Err(e) => {
            let msg = format!("Attendance error: {}", e);
            if let Some(ref mut existing) = error_message {
                existing.push_str(&format!(" | {}", msg));
            } else {
                error_message = Some(msg);
            }
        }
    }

    match read_classroom_deadlines(&classroom_path) {
        Ok(deadlines) => {
            upcoming_deadlines = deadlines;
        }
        Err(e) => {
            let msg = format!("Deadlines error: {}", e);
            if let Some(ref mut existing) = error_message {
                existing.push_str(&format!(" | {}", msg));
            } else {
                error_message = Some(msg);
            }
        }
    }

    let state = AppState {
        weekly_github,
        weekly_todos,
        weekly_grades,
        attendance_status,
        upcoming_deadlines,
        error_message,
    };

    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, state);

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

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
                if key.code == KeyCode::Char('q') {
                    return Ok(());
                }
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, state: &AppState) {
    let size = f.area();

    // Vertical layout: Header, main content, status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Main dashboard content
            Constraint::Length(3), // Status/Errors
        ])
        .split(size);

    // 1. Header
    let header_text = format!(
        " Academic Dashboard | Current Time: {} | Manila Timezone (UTC+8)",
        Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(header, chunks[0]);

    // Split main content area into Left (Week Grid) and Right (Sidebar: Attendance & Deadlines)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(70), // Left: Week Grid
            Constraint::Percentage(30), // Right: Sidebar
        ])
        .split(chunks[1]);

    // 2a. Week Grid (3 rows, 6 columns for 18 weeks)
    let grid_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(main_chunks[0]);

    for row in 0..3 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
            ])
            .split(grid_chunks[row]);

        for col in 0..6 {
            let week_idx = row * 6 + col;
            let week_num = week_idx + 1;

            let git_val = state.weekly_github[week_idx as usize];
            let todo_val = state.weekly_todos[week_idx as usize];
            let grade_val = state.weekly_grades[week_idx as usize];

            let content = vec![
                Line::from(vec![
                    Span::raw("Git:  "),
                    Span::styled(format!("{:>3}", git_val), Style::default().fg(Color::Green)),
                ]),
                Line::from(vec![
                    Span::raw("Todo: "),
                    Span::styled(format!("{:>3}", todo_val), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(vec![
                    Span::raw("Grad: "),
                    Span::styled(format!("{:>3}", grade_val), Style::default().fg(Color::Magenta)),
                ]),
            ];

            let block = Block::default()
                .borders(Borders::ALL)
                .title(format!("Week {}", week_num))
                .title_style(Style::default().add_modifier(Modifier::BOLD));

            let paragraph = Paragraph::new(content).block(block);
            f.render_widget(paragraph, cols[col]);
        }
    }

    // 2b. Right Sidebar (Attendance and Upcoming Deadlines)
    let sidebar_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40), // Attendance status
            Constraint::Percentage(60), // Upcoming Deadlines
        ])
        .split(main_chunks[1]);

    // Render Attendance Gauge
    let mut attendance_lines = Vec::new();
    if state.attendance_status.is_empty() {
        attendance_lines.push(Line::from(Span::raw("No attendance logged")));
    } else {
        for status in &state.attendance_status {
            // Lecture Absences
            let lec_percentage = if status.lecture_limit > 0 {
                status.lecture_absences as f32 / status.lecture_limit as f32
            } else {
                0.0
            };
            let lec_color = if lec_percentage >= 0.75 {
                Color::Red
            } else if lec_percentage >= 0.5 {
                Color::LightYellow
            } else {
                Color::Green
            };

            attendance_lines.push(Line::from(vec![
                Span::styled(format!("{:<8} ", status.course), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("Lec: "),
                Span::styled(format!("{}/{}", status.lecture_absences, status.lecture_limit), Style::default().fg(lec_color)),
            ]));

            // Lab Absences (if applicable)
            if status.lab_limit > 0 {
                let lab_percentage = status.lab_absences as f32 / status.lab_limit as f32;
                let lab_color = if lab_percentage >= 0.75 {
                    Color::Red
                } else if lab_percentage >= 0.5 {
                    Color::LightYellow
                } else {
                    Color::Green
                };
                attendance_lines.push(Line::from(vec![
                    Span::raw("         Lab: "),
                    Span::styled(format!("{}/{}", status.lab_absences, status.lab_limit), Style::default().fg(lab_color)),
                ]));
            }
        }
    }

    let attendance_panel = Paragraph::new(attendance_lines)
        .block(Block::default().borders(Borders::ALL).title("Attendance (Abs/Max)"))
        .style(Style::default());
    f.render_widget(attendance_panel, sidebar_chunks[0]);

    // Render Upcoming Deadlines List
    let mut deadline_lines = Vec::new();
    let now_manila = Local::now().with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap());
    
    let active_deadlines: Vec<&Deadline> = state.upcoming_deadlines
        .iter()
        .filter(|d| !d.submitted && d.submission_state != "returned")
        .take(5)
        .collect();

    if active_deadlines.is_empty() {
        deadline_lines.push(Line::from(Span::raw("No pending deadlines! 🎉")));
    } else {
        for d in active_deadlines {
            let due_date = d.due.format("%m-%d").to_string();
            let due_text = format!("(due {})", due_date);
            let time_left = d.due.signed_duration_since(now_manila);
            let color = if time_left.num_days() < 2 {
                Color::Red
            } else if time_left.num_days() < 4 {
                Color::Yellow
            } else {
                Color::Gray
            };

            let title_cropped = if d.title.len() > 16 {
                format!("{}...", &d.title[..13])
            } else {
                d.title.clone()
            };

            deadline_lines.push(Line::from(vec![
                Span::styled(format!("[{}] ", d.course.split(' ').next().unwrap_or("")), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:<16} ", title_cropped), Style::default().add_modifier(Modifier::DIM)),
                Span::styled(due_text, Style::default().fg(color)),
            ]));
        }
    }

    let deadlines_panel = Paragraph::new(deadline_lines)
        .block(Block::default().borders(Borders::ALL).title("Upcoming Deadlines"))
        .style(Style::default());
    f.render_widget(deadlines_panel, sidebar_chunks[1]);

    // 3. Status Bar / Errors
    let status_text = if let Some(ref err) = state.error_message {
        Line::from(vec![
            Span::styled("WARNING: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(err),
        ])
    } else {
        Line::from(vec![
            Span::styled("SYSTEM HEALTHY", Style::default().fg(Color::Green)),
            Span::raw(" | Press 'q' to exit"),
        ])
    };

    let status_bar = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Logs"))
        .style(Style::default());
    f.render_widget(status_bar, chunks[2]);
}
