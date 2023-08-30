use std::{
    cmp::max,
    error::Error,
    fs::read_dir,
    io::{self, Stdout},
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::{Constraint, CrosstermBackend, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut terminal = setup_terminal()?;
    let state = State {
        input: "".to_string(),
        mode: Mode::Normal,
        left_path: "~/".to_string(),
        right_path: "~/".to_string(),
        left_contents: None,
        right_contents: None,
        fzf_suggestions: None,
        left_list_state: ListState::default(),
        right_list_state: ListState::default(),
        fzf_list_state: ListState::default(),
    };
    run(&mut terminal, state).await?;
    restore_terminal(&mut terminal)?;
    Ok(())
}

#[derive(Debug, Copy, Clone)]
enum Field {
    LeftPath,
    RightPath,
}

#[derive(Debug, Copy, Clone)]
enum Mode {
    Normal,
    Edit(Field),
}

#[derive(Debug, Clone)]
struct State {
    mode: Mode,
    left_path: String,
    right_path: String,
    fzf_suggestions: Option<Vec<String>>,
    left_contents: Option<Vec<PathBuf>>,
    right_contents: Option<Vec<PathBuf>>,
    input: String,
    left_list_state: ListState,
    right_list_state: ListState,
    fzf_list_state: ListState,
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>, Box<dyn Error>> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<(), Box<dyn Error>> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen,)?;
    Ok(terminal.show_cursor()?)
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut state: State,
) -> Result<(), Box<dyn Error>> {
    Ok(loop {
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match state.mode {
                    Mode::Normal => {
                        match key.code {
                            KeyCode::Char('q') => {
                                break;
                            }
                            KeyCode::Char('H') => {
                                state.mode = Mode::Edit(Field::LeftPath);
                            }
                            KeyCode::Char('L') => {
                                state.mode = Mode::Edit(Field::RightPath);
                            }
                            _ => {}
                        };
                    }
                    Mode::Edit(field) => {
                        match key.code {
                            KeyCode::Esc => {
                                state.mode = Mode::Normal;
                            }
                            KeyCode::Up => match state.fzf_list_state.selected() {
                                Some(v) => {
                                    if v > 0 {
                                        state.fzf_list_state.select(Some(v - 1));
                                    } else {
                                        state.fzf_list_state.select(None);
                                    }
                                }
                                None => {}
                            },
                            KeyCode::Down => match state.fzf_list_state.selected() {
                                Some(v) => {
                                    state.fzf_list_state.select(Some(v + 1));
                                }
                                None => {
                                    state
                                        .fzf_list_state
                                        .select(Some(state.fzf_list_state.selected().unwrap_or(0)));
                                }
                            },
                            KeyCode::Tab => {}
                            KeyCode::Char(key) => {
                                state.input = format!("{}{}", state.input, key);
                                update_fzf(state.input.clone(), &mut state).await;
                            }
                            KeyCode::Enter => {
                                match field {
                                    Field::LeftPath => {
                                        state.left_path = state.input.clone();
                                        let content = read_path_content(PathBuf::from(
                                            state.left_path.clone(),
                                        ));
                                        state.left_contents = Some(content);
                                    }
                                    Field::RightPath => {
                                        state.right_path = state.input;
                                        let content = read_path_content(PathBuf::from(
                                            state.right_path.clone(),
                                        ));
                                        state.right_contents = Some(content);
                                    }
                                }
                                //todo: check that the path is a folder

                                state.input = "".to_string();
                                state.mode = Mode::Normal;
                            }
                            KeyCode::Backspace => {
                                state.input.pop();
                                update_fzf(state.input.clone(), &mut state).await;
                            }
                            _ => {}
                        };
                    }
                };
            }
        }

        draw(terminal, &mut state);
    })
}

async fn update_fzf(input: String, state: &mut State) -> () {
    let result = run_fzf_query(input.as_str()).await;
    match result {
        Ok(v) => state.fzf_suggestions = Some(v),
        Err(_) => {}
    }
}

fn read_path_content(path: impl AsRef<Path>) -> Vec<PathBuf> {
    let Ok(entries) = read_dir(path) else { return vec![] };
    entries
        .flatten()
        .flat_map(|entry| {
            let Ok(meta) = entry.metadata() else { return vec![] };
            if meta.is_file() {
                return vec![entry.path()];
            }
            vec![]
        })
        .collect()
}

fn draw(terminal: &mut Terminal<CrosstermBackend<Stdout>>, state: &mut State) -> () {
    terminal
        .draw(|frame| match state.mode {
            Mode::Normal => {
                let size = frame.size();
                let sides_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .margin(2)
                    .constraints([Constraint::Percentage(50), Constraint::Min(5)].as_ref())
                    .split(size);

                let left_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(0)
                    .constraints([Constraint::Length(2), Constraint::Min(5)].as_ref())
                    .split(sides_chunks[0]);

                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(0)
                    .constraints([Constraint::Length(2), Constraint::Min(5)].as_ref())
                    .split(sides_chunks[1]);

                let left_contents = match state.left_contents.clone() {
                    Some(v) => v,
                    None => vec![],
                };

                let lists_items: Vec<_> = left_contents
                    .iter()
                    .map(|i| {
                        ListItem::new(Line::from(vec![Span::styled(
                            i.to_str().unwrap_or(""),
                            Style::default(),
                        )]))
                    })
                    .collect();

                let lists_ui = List::new(lists_items)
                    .block(Block::default().title("List").borders(Borders::ALL))
                    .style(Style::default().fg(Color::White))
                    .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
                    .highlight_symbol(">>");

                let left_path = match state.mode {
                    Mode::Edit(Field::LeftPath) => state.input.clone(),
                    _ => state.left_path.clone(),
                };
                let left_path_text = Paragraph::new(left_path);

                frame.render_widget(left_path_text.clone(), left_chunks[0]);
                frame.render_widget(lists_ui, left_chunks[1]);

                frame.render_widget(left_path_text, right_chunks[0]);
            }
            Mode::Edit(field) => {
                let size = frame.size();

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(2)
                    .constraints([Constraint::Length(4), Constraint::Min(5)].as_ref())
                    .split(size);

                //fzf_suggestions

                let suggestions = state.fzf_suggestions.clone().unwrap_or(vec![]);

                let list_items: Vec<_> = suggestions
                    .iter()
                    .map(|i| ListItem::new(Line::from(vec![Span::styled(i, Style::default())])))
                    .collect();

                let lists_ui = List::new(list_items)
                    .block(Block::default().title("List").borders(Borders::ALL))
                    .style(Style::default().fg(Color::White))
                    .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
                    .highlight_symbol(">>");

                let paragraph = Paragraph::new(state.input.clone())
                    .block(Block::default().title("path").borders(Borders::ALL));

                frame.render_widget(paragraph, chunks[0]);
                frame.render_stateful_widget(lists_ui, chunks[1], &mut state.fzf_list_state);
            }
        })
        .ok();
}

async fn run_fzf_query(query: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let command = format!(
        "find \"{}\" -maxdepth 3 -type d -print | fzf -f {}",
        query, query
    );
    let fzf_output = Command::new("sh").arg("-c").arg(&command).output().await?;

    // match fzf_output {
    //     Ok(_) => {
    //         panic!("ok: {:?}", fzf_output);
    //     },
    //     Err(_) => {panic!("{:?}", fzf_output);}
    // };

    match fzf_output.status.success() {
        true => {
            let output_str = String::from_utf8_lossy(&fzf_output.stdout);
            Ok(output_str
                .to_string()
                .lines()
                .map(|l| l.to_string())
                .collect())
        }
        false => Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to run fzf query: {}", query),
        ))),
    }
}
