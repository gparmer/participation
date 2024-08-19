// Mostly an example taken from https://github.com/ratatui-org/ratatui/blob/main/examples/user_input.rs

use std::{cmp, env, ffi::OsString, fmt, fs::File, io};

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use anyhow;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use rand::prelude::*;

use itertools::Itertools;

use ratatui::{
    backend::{Backend, CrosstermBackend},
    crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
            KeyModifiers,
        },
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Layout, Position},
    prelude::{Alignment, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Clear, List, ListItem, Padding, Paragraph},
    Frame, Terminal,
};

enum InputMode {
    Command,
    Searching,
    Student,
}

#[derive(Clone)]
enum DisplayMode {
    Command,
    Searching,
}

type StudentKey = String;
#[derive(Debug, Clone, Deserialize, Serialize)]
struct Student {
    name: String,
    email: StudentKey,
    participation_score: usize,
    deferrals: usize,
    absent: usize,
    #[serde(skip_serializing, default)]
    answered_today: usize,
    #[serde(skip_serializing, default)]
    color: usize, // the offset into COLORS
}

const COLORS: &str = "🔴🟠🟡🟢🔵";

impl fmt::Display for Student {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let flames = "🔥".to_string().repeat(self.answered_today);
        let cs: Vec<_> = COLORS.chars().collect();
        write!(
            f,
            "{}{:3} {} {}",
            cs[self.color], self.participation_score, flames, self.name
        )
    }
}

/// App holds the state of the application
struct App {
    /// Backing file containing all of the students
    db: OsString,
    /// Current value of the input box
    input: String,
    /// Position of cursor in the editor area.
    character_index: usize,
    /// How the main screen should render
    display_mode: DisplayMode,
    /// Display the selected student in a popout
    student_display: Option<Student>,
    /// All students, indexed by github id
    students: HashMap<StudentKey, Student>,
    /// The order to display students outside of search mode
    order: Vec<StudentKey>,
    /// The filtered, sorted view of students
    view: Vec<StudentKey>,
    /// The offset of the selected entry into the view
    selection: Option<usize>,
}

fn deserialize_file(file_path: &OsString) -> anyhow::Result<HashMap<StudentKey, Student>> {
    let file = File::open(file_path)?;

    let mut f = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .comment(Some(b'#'))
        .flexible(true)
        .from_reader(file);

    let mut students = HashMap::new();
    for s_rec in f.deserialize() {
        let s: Student = s_rec?;
        let email = s.email.trim().to_string();

        students.insert(
            email.clone(),
            Student {
                name: s.name.trim().to_string(),
                email,
                participation_score: s.participation_score,
                deferrals: s.deferrals,
                absent: s.absent,
                answered_today: 0,
                color: 0,
            },
        );
    }

    Ok(students)
}

impl App {
    fn new(db: OsString) -> anyhow::Result<Self> {
        let students = deserialize_file(&db)?;

        let mut s = Self {
            db,
            input: String::new(),
            display_mode: DisplayMode::Command,
            student_display: None,
            students,
            character_index: 0,
            selection: None,
            view: Vec::new(),
            order: Vec::new(),
        };
        s.randomize();
        Ok(s)
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
        if self.selection == None {
            return;
        }
        let sel = self.selection.unwrap();
        if sel > 0 {
            self.selection = Some(sel - 1);
        }
    }

    fn move_selection_down(&mut self) {
        if self.selection == None {
            return;
        }
        let sel = self.selection.unwrap();
        if sel < self.view.len() - 1 {
            self.selection = Some(sel + 1);
        }
    }

    fn selection_reset(&mut self) {
        if self.view.len() > 0 {
            self.selection = Some(0);
        } else {
            self.selection = None;
        }
    }

    fn students_view(&self) -> &Vec<StudentKey> {
        &self.view
    }

    fn selected_student(&self) -> Option<&Student> {
        let sel = self.selection?;
        assert!(self.view.len() >= sel);
        Some(
            &self
                .students
                .get(&self.view[sel])
                .expect("View has a stale student not in the student db."),
        )
    }

    fn update_student_view(&mut self) {
        let view = if self.input.len() != 0 {
            // If there's an active search term, use fuzzy matching
            let matcher = SkimMatcherV2::default();
            let mut matched: Vec<(Student, i64)> = self
                .students
                .iter()
                .filter_map(|(_, entry)| {
                    if let Some(score) =
                        matcher.fuzzy_match(&entry.name.to_lowercase(), &self.input.to_lowercase())
                    {
                        Some((entry.clone(), score))
                    } else {
                        None
                    }
                })
                .collect();
            matched.sort_by(|(_, a), (_, b)| b.cmp(&a));
            matched.into_iter().map(|(s, _)| s.email).collect()
        } else {
            // Otherwise just the order is random, biased by
            // participation score, see `randomize` below.
            self.order
                .iter()
                .map(|key| {
                    self.students
                        .get(key)
                        .expect("The order is inconsistent and has a student not in the db.")
                        .email
                        .clone()
                })
                .collect()
        };

        self.view = view;
        self.selection_reset();
    }

    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.input.insert(index, new_char);
        self.move_cursor_right();
        self.update_student_view();
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
        self.update_student_view();
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    fn reset_cursor(&mut self) {
        self.character_index = 0;
    }

    fn display_selected_student(&mut self) {
        self.student_display = self.selected_student().map(|s| s.clone());
    }

    fn input_clear(&mut self) {
        self.input.clear();
        self.reset_cursor();
        self.update_student_view();
    }

    fn input_mode(&self) -> InputMode {
        if self.student_display.is_some() {
            return InputMode::Student;
        }
        match self.display_mode {
            DisplayMode::Command => InputMode::Command,
            DisplayMode::Searching => InputMode::Searching,
        }
    }

    fn student_escape(&mut self) {
        self.student_display = None;
    }

    fn student_absent(&mut self) {
        self.student_escape();
    }

    fn student_defer(&mut self) {
        self.student_escape();
    }

    fn student_answer(&mut self) {
        assert!(self.student_display.is_some());
        let s = self.student_display.as_ref().unwrap();

        let s = self
            .students
            .get_mut(&s.email)
            .expect("Student database became inconsistent with active student");
        s.participation_score += 1;
        s.answered_today += 1;
        self.update_data();
        self.student_escape();
    }

    // Brutally inefficient, but luckily my classes have only ~70
    // students!
    fn randomize(&mut self) {
        let (max, min) = self
            .students
            .iter()
            .fold((0, std::usize::MAX), |(max, min), (_, s)| {
                (
                    cmp::max(max, s.participation_score),
                    cmp::min(min, s.participation_score),
                )
            });

        let mut bag = Vec::new();
        let norm = max - min;
        for (_, s) in self.students.iter_mut() {
            // Normalize and scale the participation scores
            let chances: usize = norm - (s.participation_score - min);
            // And add a corresponding number of tokens
            for _ in 0..=chances {
                bag.push(s.email.clone());
            }

	    // Since we're looking at the student, lets compute their
	    // circle's color
	    let color: usize = (((s.participation_score - min) as f64 / norm as f64) * 4.0).round() as usize;
	    s.color = color;
	}
        let mut rng = rand::thread_rng();
        bag.shuffle(&mut rng);

        let mut order = Vec::new();
        for k in &bag {
            if order.iter().find(|i| *i == k).is_none() {
                order.push(k.clone());
            }
        }
        assert!(self.students.len() == order.len());

        self.order = order;
        self.update_student_view();
        self.selection_reset();
    }

    // The data has been updated, so we need to update all
    // corresponding data-structures, and the db.
    fn update_data(&mut self) {
        self.randomize();
        // TODO: write back to the DB.
    }
}

fn main() -> anyhow::Result<()> {
    //Result<(), Box<dyn Error>> {
    if env::args_os().len() != 2 {
        println!("Usage: {} student_list.csv\nwhere the csv file is tab-delimited and can have arbitrary names.", env::args_os().nth(0).unwrap().to_str().unwrap());
        return Err(anyhow::anyhow!("Incorrect number of arguments"));
    }
    let file_path = env::args_os()
        .nth(1)
        .ok_or(anyhow::anyhow!("Argument {} not provided.", 1))?;

    let app = App::new(file_path)?;

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
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
            match app.input_mode() {
                InputMode::Command => match key.code {
                    KeyCode::Char('s') | KeyCode::Char('/') => {
                        app.display_mode = DisplayMode::Searching;
                    }
                    KeyCode::Char('r') => {
                        app.randomize();
                    }
                    KeyCode::Char('q') => {
                        return Ok(());
                    }
                    KeyCode::Down => {
                        app.move_selection_down();
                    }
                    KeyCode::Up => {
                        app.move_selection_up();
                    }
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.move_selection_up();
                    }
                    KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.move_selection_down();
                    }
                    KeyCode::Enter => {
                        app.display_selected_student();
                    }
                    _ => {}
                },
                InputMode::Searching if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Enter => {
                        app.display_selected_student();
                    }
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.move_selection_up();
                    }
                    KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.move_selection_down();
                    }
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
                        app.display_mode = DisplayMode::Command;
                        app.input_clear();
                    }
                    _ => {}
                },
                InputMode::Searching => {}
                InputMode::Student => match key.code {
                    // If student defers/delays
                    KeyCode::Char('d') => {
                        app.student_defer();
                    }
                    // If student is absent, or provides no answer
                    KeyCode::Char('n') => {
                        app.student_absent();
                    }
                    // If student answers like a boss
                    KeyCode::Char('a') => {
                        app.student_answer();
                    }
                    KeyCode::Esc => {
                        app.student_escape();
                    }
                    _ => {}
                },
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
    let area = f.area();
    let [help_area, input_area, students_area] = vertical.areas(area);

    let (msg, style) = match app.input_mode() {
        InputMode::Command => (
            vec![
                "q".bold(),
                " = quit, ".into(),
                "r".bold(),
                " = randomize (biased), ".into(),
                "s".bold(),
                " = search, ".into(),
                "↑".bold(),
                " and ".into(),
                "↓".bold(),
                " = navigate students.".into(),
            ],
            Style::default(),
        ),
        InputMode::Searching => (
            vec![
                "Esc".bold(),
                " = go back, ".into(),
                "Enter".bold(),
                " = select a student, ".into(),
                "↑".bold(),
                " and ".into(),
                "↓".bold(),
                " = navigate students.".into(),
            ],
            Style::default(),
        ),
        InputMode::Student => (
            vec![
                "Esc".bold(),
                " to go back, ".into(),
                "a".bold(),
                " = answer, ".into(),
                "n".bold(),
                " = absent or no answer ".into(),
                "d".bold(),
                " = defer.".into(),
            ],
            Style::default(),
        ),
    };
    let text = Text::from(Line::from(msg)).patch_style(style);
    let help_message = Paragraph::new(text);
    f.render_widget(help_message, help_area);

    let input = Paragraph::new(app.input.as_str())
        .style(match app.display_mode {
            DisplayMode::Searching => Style::default().fg(Color::Green),
            _ => Style::default(),
        })
        .block(Block::bordered().title("Query"));
    f.render_widget(input, input_area);
    match app.display_mode {
        // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
        DisplayMode::Command => {}

        // Make the cursor visible and ask ratatui to put it at the specified coordinates after
        // rendering
        #[allow(clippy::cast_possible_truncation)]
        DisplayMode::Searching => f.set_cursor_position(Position::new(
            // Draw the cursor at the current position in the input field.
            // This position is can be controlled via the left and right arrow key
            input_area.x + app.character_index as u16 + 1,
            // Move one line down, from the border to the input line
            input_area.y + 1,
        )),
    };
    let students: Vec<ListItem> = app
        .students_view()
        .iter()
        .enumerate()
        .map(|(i, key)| {
            let s = app
                .students
                .get(key)
                .expect("View has inconsistent name with the student db.");
            let content = if app.selection.is_some() && i == app.selection.unwrap() {
                Line::from(Span::styled(
                    format!("{s}"),
                    Style::default().bg(Color::Green).fg(Color::Black),
                ))
            } else {
                Line::from(Span::raw(format!("{}", s)))
            };
            ListItem::new(content)
        })
        .collect();
    let students = List::new(students).block(
        Block::bordered()
            .title("Students")
            .padding(Padding::new(2, 2, 1, 1)),
    );
    f.render_widget(students, students_area);

    if let Some(s) = &app.student_display {
        let area = centered_rect(60, 20, area);
        let block = Paragraph::new(format!("{s}"))
            .style(Style::default())
            .alignment(Alignment::Center)
            .block(Block::bordered().title("Student").padding(Padding::new(
                0,
                0,
                area.height / 2 - 1,
                0,
            )))
            .style(
                Style::default()
                    .bg(Color::Gray)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_widget(Clear, area); //this clears out the background
        f.render_widget(block, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
