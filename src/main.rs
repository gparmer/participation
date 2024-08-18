// Mostly an example taken from https://github.com/ratatui-org/ratatui/blob/main/examples/user_input.rs

use std::{error::Error, io};

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use ratatui::{
    backend::{Backend, CrosstermBackend},
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Layout, Position},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, List, ListItem, Paragraph},
    Frame, Terminal,
};

enum InputMode {
    Query,
    Searching,
}

#[derive(Clone)]
struct Student {
    name: String,
    participation_score: usize,
}

impl Student {
    fn new(name: String, participation_score: usize) -> Student {
        Student {
            name,
            participation_score,
        }
    }
}

enum SelectionDirection {
    Up,
    Down,
}

/// App holds the state of the application
struct App {
    /// Current value of the input box
    input: String,
    /// Position of cursor in the editor area.
    character_index: usize,
    /// Current input mode
    input_mode: InputMode,
    /// The entry that is selected
    selection: usize,
    /// History of recorded messages
    students: Vec<Student>,
}

impl App {
    const fn new() -> Self {
        Self {
            input: String::new(),
            input_mode: InputMode::Query,
            students: Vec::new(),
            character_index: 0,
            selection: 0,
        }
    }

    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    fn move_selection_up(&mut self) {
        if self.selection > 0 {
            self.selection = self.selection - 1;
        };
    }

    fn move_selection_down(&mut self) {
	self.selection = self.selection + 1;
    }

    fn selection_reset(&mut self) {
	self.selection = 0;
    }

    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.input.insert(index, new_char);
        self.move_cursor_right();
	self.selection_reset();
    }

    /// Returns the byte index based on the character position.
    ///
    /// Since each character in a string can be contain multiple bytes, it's necessary to calculate
    /// the byte index based on the index of the character.
    fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.character_index)
            .unwrap_or(self.input.len())
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            // Method "remove" is not used on the saved text for deleting the selected char.
            // Reason: Using remove on String works on bytes instead of the chars.
            // Using remove would require special care because of char boundaries.

            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            // Getting all characters before the selected character.
            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            // Getting all characters after selected character.
            let after_char_to_delete = self.input.chars().skip(current_index);

            // Put all characters together except the selected one.
            // By leaving the selected one out, it is forgotten and therefore deleted.
            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
	self.selection_reset();
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    fn reset_cursor(&mut self) {
        self.character_index = 0;
    }

    fn submit_message(&mut self) {
        self.students.push(Student::new(self.input.clone(), 0));
        self.input.clear();
        self.reset_cursor();
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = App::new();
    let res = run_app(&mut terminal, app);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{err:?}");
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &app))?;

        if let Event::Key(key) = event::read()? {
            match app.input_mode {
                InputMode::Query => match key.code {
                    KeyCode::Char('e') => {
                        app.input_mode = InputMode::Searching;
                    }
                    KeyCode::Char('q') => {
                        return Ok(());
                    }
                    _ => {}
                },
                InputMode::Searching if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Enter => app.submit_message(),
                    KeyCode::Char(to_insert) => {
                        app.enter_char(to_insert);
                    }
                    KeyCode::Backspace => {
                        app.delete_char();
                    }
                    KeyCode::Left => {
                        app.move_cursor_left();
                    }
                    KeyCode::Right => {
                        app.move_cursor_right();
                    }
                    KeyCode::Down => {
                        app.move_selection_down();
                    }
                    KeyCode::Up => {
                        app.move_selection_up();
                    }
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Query;
                    }
                    _ => {}
                },
                InputMode::Searching => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let vertical = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(1),
    ]);
    let [help_area, input_area, messages_area] = vertical.areas(f.area());

    let (msg, style) = match app.input_mode {
        InputMode::Query => (
            vec![
                "Press ".into(),
                "q".bold(),
                " to exit, ".into(),
                "e".bold(),
                " to start editing.".bold(),
            ],
            Style::default().add_modifier(Modifier::RAPID_BLINK),
        ),
        InputMode::Searching => (
            vec![
                "Press ".into(),
                "Esc".bold(),
                " to stop editing, ".into(),
                "Enter".bold(),
                " to record the message".into(),
            ],
            Style::default(),
        ),
    };
    let text = Text::from(Line::from(msg)).patch_style(style);
    let help_message = Paragraph::new(text);
    f.render_widget(help_message, help_area);

    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Query => Style::default(),
            InputMode::Searching => Style::default().fg(Color::Green),
        })
        .block(Block::bordered().title("Query"));
    f.render_widget(input, input_area);
    match app.input_mode {
        // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
        InputMode::Query => {}

        // Make the cursor visible and ask ratatui to put it at the specified coordinates after
        // rendering
        #[allow(clippy::cast_possible_truncation)]
        InputMode::Searching => f.set_cursor_position(Position::new(
            // Draw the cursor at the current position in the input field.
            // This position is can be controlled via the left and right arrow key
            input_area.x + app.character_index as u16 + 1,
            // Move one line down, from the border to the input line
            input_area.y + 1,
        )),
    }

    let output: Vec<Student> = if app.input.len() != 0 {
        let matcher = SkimMatcherV2::default();
        let mut matched: Vec<(Student, i64)> = app
            .students
            .iter()
            .filter_map(|entry| {
                if let Some(score) =
                    matcher.fuzzy_match(&entry.name.to_lowercase(), &app.input.to_lowercase())
                {
                    Some((entry.clone(), score))
                } else {
                    None
                }
            })
            .collect();
        matched.sort_by(|(_, a), (_, b)| a.cmp(&b));
        matched.into_iter().map(|(s, _)| s).collect()
    } else {
        let mut out: Vec<(Student, i64)> = app
            .students
            .iter()
            .map(|student| (student.clone(), 0))
            .collect();
        out.sort_by(|(_, a), (_, b)| a.cmp(&b));
        out.into_iter().map(|(s, _)| s).collect()
    };

    let messages: Vec<ListItem> = output
        .iter()
        .enumerate()
        .map(|(i, student)| {
            let content = if i == app.selection {
                Line::from(Span::styled(
                    format!("[{:6}] {}", student.participation_score, student.name),
                    Style::default().bg(Color::Green).fg(Color::Black),
                ))
            } else {
                Line::from(Span::raw(format!(
                    "[{:6}] {}",
                    student.participation_score, student.name
                )))
            };
            ListItem::new(content)
        })
        .collect();
    let messages = List::new(messages).block(Block::bordered().title("Students"));
    f.render_widget(messages, messages_area);
}
