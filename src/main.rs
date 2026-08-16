pub mod data;

use std::path::PathBuf;
use clap::Parser;
use chrono::Local;
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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "/home/simone/smon-os")]
    vault_path: PathBuf,
}

struct AppState {
    weekly_github: [u32; 18],
    weekly_todos: [u32; 18],
    error_message: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Paths
    let github_path = args.vault_path.join("_AI/tracking/github-contributions.csv");
    let todos_path = args.vault_path.join("_AI/todos/todos.md");

    // Load data
    let mut weekly_github = [0u32; 18];
    let mut weekly_todos = [0u32; 18];
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

    let state = AppState {
        weekly_github,
        weekly_todos,
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
            Constraint::Min(10),   // Week grid
            Constraint::Length(3), // Status/Errors
        ])
        .split(size);

    // 1. Header
    let header_text = format!(
        " Academic Dashboard | Current Time: {} | Manila timezone",
        Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(header, chunks[0]);

    // 2. Week Grid (3 rows, 6 columns for 18 weeks)
    let grid_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(chunks[1]);

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

            let content = vec![
                Line::from(vec![
                    Span::raw("Git:  "),
                    Span::styled(format!("{:>3}", git_val), Style::default().fg(Color::Green)),
                    Span::raw(" commits"),
                ]),
                Line::from(vec![
                    Span::raw("Todo: "),
                    Span::styled(format!("{:>3}", todo_val), Style::default().fg(Color::Yellow)),
                    Span::raw(" done"),
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
