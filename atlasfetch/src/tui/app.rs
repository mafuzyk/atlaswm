// TUI application — `atlasfetch setup`
//
// The interface is split into two areas: a settings panel (left ~55%) and a
// live preview (right ~45%). A step bar at the top shows progress through
// the setup flow. Each step modifies a specific part of the configuration,
// and the preview re-renders on every change.
//
// Steps:
//   1. Welcome — brief intro, press any key to start
//   2. Theme — pick a color palette from the preset list
//   3. ASCII — choose built-in logo, custom file, paste, or disable
//   4. Layout — choose Centered / Compact / Wide / Minimal / Balanced
//   5. Panels — enable/disable fields and reorder
//   6. Summary — review all choices and save

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as TuiColor, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph,
};
use ratatui::Frame;
use ratatui::{prelude::*, Terminal};
use std::io;

use crate::ascii;
use crate::config::{self, Config};
use crate::info;
use crate::layout::AppLayout;
use crate::render::{self};
use crate::theme::{self, Color};

// ── Step enum ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Welcome = 0,
    Theme = 1,
    Ascii = 2,
    Layout = 3,
    Panels = 4,
    Summary = 5,
}

impl Step {
    fn all() -> [Step; 6] {
        [Step::Welcome, Step::Theme, Step::Ascii, Step::Layout, Step::Panels, Step::Summary]
    }
    fn label(&self) -> &'static str {
        match self {
            Step::Welcome => "Welcome",
            Step::Theme => "Theme",
            Step::Ascii => "ASCII",
            Step::Layout => "Layout",
            Step::Panels => "Panels",
            Step::Summary => "Summary",
        }
    }
    fn index(&self) -> usize {
        *self as usize
    }
    fn next(&self) -> Option<Step> {
        match self {
            Step::Welcome => Some(Step::Theme),
            Step::Theme => Some(Step::Ascii),
            Step::Ascii => Some(Step::Layout),
            Step::Layout => Some(Step::Panels),
            Step::Panels => Some(Step::Summary),
            Step::Summary => None,
        }
    }
    fn prev(&self) -> Option<Step> {
        match self {
            Step::Welcome => None,
            Step::Theme => Some(Step::Welcome),
            Step::Ascii => Some(Step::Theme),
            Step::Layout => Some(Step::Ascii),
            Step::Panels => Some(Step::Layout),
            Step::Summary => Some(Step::Panels),
        }
    }
}

// ── ASCII source ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AsciiSource {
    Builtin(String),     // key name
    CustomFile,          // user-provided path
    Pasted(String),      // pasted ASCII art
    Disabled,            // no ASCII
}

// ── App state ────────────────────────────────────────────────────────────

pub struct App {
    // Configuration being edited
    pub cfg: Config,
    // Current step
    pub step: Step,
    // Should quit
    pub quit: bool,
    // Available logo keys
    pub logo_keys: Vec<String>,
    // Current ASCII art (for preview)
    pub current_ascii: String,
    // ASCII source for the setup flow
    pub ascii_source: AsciiSource,
    // Custom ASCII file path input
    pub custom_ascii_path: String,
    // Pasted ASCII input
    pub pasted_ascii: String,
    // Theme list state
    pub theme_list_state: ListState,
    // Logo list state (for builtin selection)
    pub logo_list_state: ListState,
    // Layout list state
    pub layout_list_state: ListState,
    // Panel list states (left and right)
    pub panel_left_state: ListState,
    pub panel_right_state: ListState,
    // Which panel side is focused
    pub panel_focus: PanelFocus,
    // Input mode for custom fields
    pub input_mode: InputMode,
    // Error message to display
    pub error_message: Option<String>,
    // Success message
    pub saved: bool,
    // Terminal dimensions
    pub term_width: u16,
    pub term_height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelFocus {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Normal,
    EditingCustomPath,
    EditingPastedAscii,
}

// ── Run ──────────────────────────────────────────────────────────────────

pub fn run(cfg: &mut Config) -> Result<()> {
    // Ensure logos are available
    ascii::ensure_logos()?;

    // Enter alternate screen
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Gather available logos
    let logo_keys = ascii::available_logos()?;

    // Load current ASCII
    let current_ascii = ascii::load(cfg).unwrap_or_default();

    // Initial layout state
    let mut theme_list_state = ListState::default();
    theme_list_state.select(Some(0));

    let mut logo_list_state = ListState::default();
    if let Some(idx) = logo_keys.iter().position(|k| k == &cfg.logo.key) {
        logo_list_state.select(Some(idx));
    } else {
        logo_list_state.select(Some(0));
    }

    let mut layout_list_state = ListState::default();
    layout_list_state.select(Some(0));

    let mut panel_left_state = ListState::default();
    panel_left_state.select(Some(0));
    let mut panel_right_state = ListState::default();
    panel_right_state.select(Some(0));

    let ascii_source = if cfg.logo.key.is_empty() {
        AsciiSource::Disabled
    } else {
        AsciiSource::Builtin(cfg.logo.key.clone())
    };

    let mut app = App {
        cfg: cfg.clone(),
        step: Step::Welcome,
        quit: false,
        logo_keys,
        current_ascii,
        ascii_source,
        custom_ascii_path: String::new(),
        pasted_ascii: String::new(),
        theme_list_state,
        logo_list_state,
        layout_list_state,
        panel_left_state,
        panel_right_state,
        panel_focus: PanelFocus::Left,
        input_mode: InputMode::Normal,
        error_message: None,
        saved: false,
        term_width: 80,
        term_height: 24,
    };

    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    let _ = terminal::disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);

    // If saved, update the caller's config
    if app.saved {
        *cfg = app.cfg;
    }

    res
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    // Enable mouse capture
    execute!(io::stdout(), crossterm::event::EnableMouseCapture)?;

    loop {
        // Update terminal dimensions
        if let Ok((w, h)) = terminal::size() {
            app.term_width = w;
            app.term_height = h;
        }

        // Re-render
        terminal.draw(|f| ui(f, app))?;

        // Handle input
        if app.quit {
            break;
        }

        if !handle_event(app)? {
            break;
        }
    }

    // Disable mouse capture
    let _ = execute!(io::stdout(), crossterm::event::DisableMouseCapture);

    Ok(())
}

// ── Event handling ───────────────────────────────────────────────────────

fn handle_event(app: &mut App) -> Result<bool> {
    if !event::poll(std::time::Duration::from_millis(100))? {
        return Ok(true);
    }
    let event = event::read()?;

    match event {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return Ok(true);
            }
            match app.input_mode {
                InputMode::EditingCustomPath => {
                    match key.code {
                        KeyCode::Enter => {
                            // Validate and load custom ASCII
                            let path = app.custom_ascii_path.trim().to_string();
                            if path.is_empty() {
                                app.error_message = Some("Path cannot be empty.".into());
                            } else {
                                let expanded = shellexpand(&path);
                                match std::fs::read_to_string(&expanded) {
                                    Ok(content) if !content.trim().is_empty() => {
                                        app.current_ascii = content.trim_end().to_string();
                                        app.ascii_source = AsciiSource::CustomFile;
                                        app.cfg.logo.key = String::new();
                                        app.cfg.logo.path = path;
                                        app.error_message = None;
                                        app.input_mode = InputMode::Normal;
                                    }
                                    Ok(_) => {
                                        app.error_message = Some("File is empty.".into());
                                    }
                                    Err(e) => {
                                        app.error_message = Some(format!("Cannot read file: {}", e));
                                    }
                                }
                            }
                        }
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                            app.error_message = None;
                        }
                        KeyCode::Backspace => {
                            app.custom_ascii_path.pop();
                        }
                        KeyCode::Char(c) => {
                            app.custom_ascii_path.push(c);
                        }
                        _ => {}
                    }
                    return Ok(true);
                }
                InputMode::EditingPastedAscii => {
                    match key.code {
                        KeyCode::Enter => {
                            // Save pasted ASCII
                            let content = app.pasted_ascii.trim().to_string();
                            if content.is_empty() {
                                app.error_message = Some("Pasted content is empty.".into());
                            } else {
                                app.current_ascii = content;
                                app.ascii_source = AsciiSource::Pasted(app.current_ascii.clone());
                                app.cfg.logo.key = String::new();
                                app.error_message = None;
                                app.input_mode = InputMode::Normal;
                            }
                        }
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                            app.error_message = None;
                        }
                        KeyCode::Backspace => {
                            app.pasted_ascii.pop();
                        }
                        KeyCode::Char(c) => {
                            app.pasted_ascii.push(c);
                        }
                        _ => {}
                    }
                    return Ok(true);
                }
                InputMode::Normal => {
                    match app.step {
                        Step::Welcome => {
                            match key.code {
                                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Tab => {
                                    app.step = Step::Theme;
                                    app.theme_list_state.select(Some(0));
                                }
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    app.quit = true;
                                }
                                _ => {}
                            }
                        }
                        Step::Theme => {
                            match key.code {
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let themes = theme::all_themes();
                                    let i = app.theme_list_state.selected().unwrap_or(0);
                                    if i + 1 < themes.len() {
                                        app.theme_list_state.select(Some(i + 1));
                                        apply_theme(app, i + 1);
                                    }
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    let i = app.theme_list_state.selected().unwrap_or(0);
                                    if i > 0 {
                                        app.theme_list_state.select(Some(i - 1));
                                        apply_theme(app, i - 1);
                                    }
                                }
                                KeyCode::Enter | KeyCode::Tab | KeyCode::Right => {
                                    if let Some(next) = app.step.next() {
                                        app.step = next;
                                    }
                                }
                                KeyCode::Left => {
                                    if let Some(prev) = app.step.prev() {
                                        app.step = prev;
                                    }
                                }
                                KeyCode::Esc => { app.quit = true; }
                                _ => {}
                            }
                        }
                        Step::Ascii => {
                            match key.code {
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let len = app.logo_keys.len().max(1);
                                    let i = app.logo_list_state.selected().unwrap_or(0);
                                    if i + 1 < len {
                                        app.logo_list_state.select(Some(i + 1));
                                        select_logo(app, i + 1);
                                    }
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    let i = app.logo_list_state.selected().unwrap_or(0);
                                    if i > 0 {
                                        app.logo_list_state.select(Some(i - 1));
                                        select_logo(app, i - 1);
                                    }
                                }
                                KeyCode::Char('c') => {
                                    // Custom file
                                    app.input_mode = InputMode::EditingCustomPath;
                                    app.custom_ascii_path.clear();
                                    app.error_message = None;
                                }
                                KeyCode::Char('p') => {
                                    // Paste
                                    app.input_mode = InputMode::EditingPastedAscii;
                                    app.pasted_ascii.clear();
                                    app.error_message = None;
                                }
                                KeyCode::Char('d') => {
                                    // Disable ASCII
                                    app.ascii_source = AsciiSource::Disabled;
                                    app.current_ascii.clear();
                                    app.cfg.logo.key = String::new();
                                }
                                KeyCode::Enter | KeyCode::Right => {
                                    if let Some(next) = app.step.next() {
                                        app.step = next;
                                    }
                                }
                                KeyCode::Left => {
                                    if let Some(prev) = app.step.prev() {
                                        app.step = prev;
                                    }
                                }
                                KeyCode::Esc => { app.quit = true; }
                                _ => {}
                            }
                        }
                        Step::Layout => {
                            match key.code {
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let layouts = AppLayout::variants();
                                    let i = app.layout_list_state.selected().unwrap_or(0);
                                    if i + 1 < layouts.len() {
                                        app.layout_list_state.select(Some(i + 1));
                                        apply_layout(app, i + 1);
                                    }
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    let i = app.layout_list_state.selected().unwrap_or(0);
                                    if i > 0 {
                                        app.layout_list_state.select(Some(i - 1));
                                        apply_layout(app, i - 1);
                                    }
                                }
                                KeyCode::Enter | KeyCode::Tab | KeyCode::Right => {
                                    if let Some(next) = app.step.next() {
                                        app.step = next;
                                    }
                                }
                                KeyCode::Left => {
                                    if let Some(prev) = app.step.prev() {
                                        app.step = prev;
                                    }
                                }
                                KeyCode::Esc => { app.quit = true; }
                                _ => {}
                            }
                        }
                        Step::Panels => {
                            match key.code {
                                KeyCode::Tab => {
                                    // Toggle between left and right panel
                                    app.panel_focus = match app.panel_focus {
                                        PanelFocus::Left => PanelFocus::Right,
                                        PanelFocus::Right => PanelFocus::Left,
                                    };
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    match app.panel_focus {
                                        PanelFocus::Left => {
                                            let i = app.panel_left_state.selected().unwrap_or(0);
                                            if i + 1 < app.cfg.display.left.len() {
                                                app.panel_left_state.select(Some(i + 1));
                                            }
                                        }
                                        PanelFocus::Right => {
                                            let i = app.panel_right_state.selected().unwrap_or(0);
                                            if i + 1 < app.cfg.display.right.len() {
                                                app.panel_right_state.select(Some(i + 1));
                                            }
                                        }
                                    }
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    match app.panel_focus {
                                        PanelFocus::Left => {
                                            let i = app.panel_left_state.selected().unwrap_or(0);
                                            if i > 0 {
                                                app.panel_left_state.select(Some(i - 1));
                                            }
                                        }
                                        PanelFocus::Right => {
                                            let i = app.panel_right_state.selected().unwrap_or(0);
                                            if i > 0 {
                                                app.panel_right_state.select(Some(i - 1));
                                            }
                                        }
                                    }
                                }
                                KeyCode::Char(' ') => {
                                    // Toggle enabled/disabled
                                    match app.panel_focus {
                                        PanelFocus::Left => {
                                            if let Some(i) = app.panel_left_state.selected() {
                                                if i < app.cfg.display.left.len() {
                                                    app.cfg.display.left[i].enabled = !app.cfg.display.left[i].enabled;
                                                }
                                            }
                                        }
                                        PanelFocus::Right => {
                                            if let Some(i) = app.panel_right_state.selected() {
                                                if i < app.cfg.display.right.len() {
                                                    app.cfg.display.right[i].enabled = !app.cfg.display.right[i].enabled;
                                                }
                                            }
                                        }
                                    }
                                }
                                KeyCode::Char('r') => {
                                    // Reorder: move up (swap with above)
                                    match app.panel_focus {
                                        PanelFocus::Left => {
                                            if let Some(i) = app.panel_left_state.selected() {
                                                if i > 0 {
                                                    app.cfg.display.left.swap(i, i - 1);
                                                    app.panel_left_state.select(Some(i - 1));
                                                }
                                            }
                                        }
                                        PanelFocus::Right => {
                                            if let Some(i) = app.panel_right_state.selected() {
                                                if i > 0 {
                                                    app.cfg.display.right.swap(i, i - 1);
                                                    app.panel_right_state.select(Some(i - 1));
                                                }
                                            }
                                        }
                                    }
                                }
                                KeyCode::Enter | KeyCode::Right => {
                                    if let Some(next) = app.step.next() {
                                        app.step = next;
                                    }
                                }
                                KeyCode::Left => {
                                    if let Some(prev) = app.step.prev() {
                                        app.step = prev;
                                    }
                                }
                                KeyCode::Esc => { app.quit = true; }
                                _ => {}
                            }
                        }
                        Step::Summary => {
                            match key.code {
                                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('s') => {
                                    // Save configuration
                                    app.cfg.version = 2;
                                    // Write logo.txt if needed
                                    match &app.ascii_source {
                                        AsciiSource::Builtin(key) => {
                                            app.cfg.logo.key = key.clone();
                                            app.cfg.logo.path = "~/.config/atlasfetch/logo.txt".into();
                                            // Also save the logo to logo.txt
                                            if !app.current_ascii.is_empty() {
                                                let logo_path = shellexpand("~/.config/atlasfetch/logo.txt");
                                                if let Some(parent) = logo_path.parent() {
                                                    let _ = std::fs::create_dir_all(parent);
                                                }
                                                let _ = std::fs::write(&logo_path, &app.current_ascii);
                                            }
                                        }
                                        AsciiSource::CustomFile => {
                                            app.cfg.logo.key = String::new();
                                        }
                                        AsciiSource::Pasted(art) => {
                                            app.cfg.logo.key = String::new();
                                            app.cfg.logo.path = "~/.config/atlasfetch/logo.txt".into();
                                            let logo_path = shellexpand("~/.config/atlasfetch/logo.txt");
                                            if let Some(parent) = logo_path.parent() {
                                                let _ = std::fs::create_dir_all(parent);
                                            }
                                            let _ = std::fs::write(&logo_path, art);
                                        }
                                        AsciiSource::Disabled => {
                                            app.cfg.logo.key = String::new();
                                        }
                                    }
                                    match app.cfg.save() {
                                        Ok(_) => {
                                            app.saved = true;
                                            app.quit = true;
                                        }
                                        Err(e) => {
                                            app.error_message = Some(format!("Failed to save: {}", e));
                                        }
                                    }
                                }
                                KeyCode::Char('n') | KeyCode::Esc => {
                                    app.quit = true;
                                }
                                KeyCode::Left => {
                                    if let Some(prev) = app.step.prev() {
                                        app.step = prev;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        Event::Mouse(mouse) => {
            handle_mouse(app, mouse);
        }
        Event::Resize(w, h) => {
            app.term_width = w;
            app.term_height = h;
        }
        _ => {}
    }

    Ok(true)
}

fn handle_mouse(_app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let col = mouse.column;
            let row = mouse.row;

            // Check if click is in the preview area (right side)
            // Simple approach: if click is in right 45%, it's preview — ignore
            // if click is on left side, handle based on current step

            // For now, just translate clicks to keyboard equivalents
            // A full mouse implementation would check widget bounds
            let _ = (col, row);
        }
        MouseEventKind::ScrollDown => {
            // Scroll down in current list
            let key = KeyCode::Down;
            let event = Event::Key(crossterm::event::KeyEvent::new(key, crossterm::event::KeyModifiers::NONE));
            let _ = event;
        }
        MouseEventKind::ScrollUp => {
            let key = KeyCode::Up;
            let event = Event::Key(crossterm::event::KeyEvent::new(key, crossterm::event::KeyModifiers::NONE));
            let _ = event;
        }
        _ => {}
    }
}

// ── Helper functions ─────────────────────────────────────────────────────

fn apply_theme(app: &mut App, index: usize) {
    let themes = theme::all_themes();
    if index < themes.len() {
        app.cfg.logo.colors = themes[index].colors.clone();
    }
}

fn select_logo(app: &mut App, index: usize) {
    if index < app.logo_keys.len() {
        let key = app.logo_keys[index].clone();
        app.ascii_source = AsciiSource::Builtin(key.clone());
        app.cfg.logo.key = key.clone();
        // Load the ASCII art
        let dir = config::logo_dir().ok();
        if let Some(dir) = dir {
            let path = dir.join(&key);
            if let Ok(content) = std::fs::read_to_string(&path) {
                app.current_ascii = content.trim_end().to_string();
            }
        }
    }
}

fn apply_layout(app: &mut App, index: usize) {
    let layouts = AppLayout::variants();
    if index < layouts.len() {
        let layout = layouts[index];
        app.cfg.panel.gap = layout.gap();
        app.cfg.panel.left_pad = layout.padding();
        app.cfg.panel.right_pad = layout.padding();
    }
}

fn shellexpand(s: &str) -> std::path::PathBuf {
    if let Some(rest) = s.strip_prefix('~') {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        std::path::PathBuf::from(home).join(rest.trim_start_matches('/'))
    } else {
        std::path::PathBuf::from(s)
    }
}

// ── UI rendering ─────────────────────────────────────────────────────────

fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Layout: vertical split
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),  // Header + step indicator
            Constraint::Min(1),     // Main content
            Constraint::Length(3),  // Footer / navigation bar
        ])
        .split(area);

    // Header
    render_header(frame, chunks[0], app);

    // Main content
    if app.step == Step::Welcome {
        // Welcome screen: full-width, no preview
        render_welcome(frame, chunks[1]);
    } else {
        // Other screens: horizontal split (settings | preview)
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(chunks[1]);

        render_settings(frame, main_chunks[0], app);
        render_preview(frame, main_chunks[1], app);
    }

    // Footer
    render_footer(frame, chunks[2], app);

    // Error/success overlay
    if let Some(ref msg) = app.error_message {
        render_overlay(frame, area, msg, TuiColor::Red);
    }
}

// ── Header ───────────────────────────────────────────────────────────────

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let title = format!("atlasfetch setup — Step {}/{}: {}",
        app.step.index() + 1,
        Step::all().len(),
        app.step.label());

    let header = Paragraph::new(Text::styled(title, Style::default().bold().fg(TuiColor::Cyan)))
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded));
    frame.render_widget(header, area);

    // Progress bar
    let total = Step::all().len();
    let current = app.step.index() + 1;
    let progress = current as f64 / total as f64;
    let bar_width = area.width.saturating_sub(4) as usize;
    let filled = (bar_width as f64 * progress) as usize;
    let empty = bar_width.saturating_sub(filled);

    let bar_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    if bar_chunks.len() >= 2 {
        let bar_area = Rect::new(area.x + 2, area.y + area.height - 1, area.width.saturating_sub(4), 1);
        let bar = format!(
            "{}{}",
            "█".repeat(filled),
            "░".repeat(empty),
        );
        let bar_widget = Paragraph::new(Text::styled(bar, Style::default().fg(TuiColor::Cyan)));
        frame.render_widget(bar_widget, bar_area);
    }
}

// ── Settings panel ───────────────────────────────────────────────────────

fn render_settings(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.step {
        Step::Welcome => render_welcome(frame, area),
        Step::Theme => render_theme_selection(frame, area, app),
        Step::Ascii => render_ascii_selection(frame, area, app),
        Step::Layout => render_layout_selection(frame, area, app),
        Step::Panels => render_panel_editor(frame, area, app),
        Step::Summary => render_summary(frame, area, app),
    }
}

// ── Welcome ──────────────────────────────────────────────────────────────

fn render_welcome(frame: &mut Frame, area: Rect) {
    let text = Text::from(vec![
        Line::from(""),
        Line::from(Span::styled("atlasfetch setup", Style::default().bold().fg(TuiColor::Cyan))),
        Line::from(""),
        Line::from("Configure your system information display."),
        Line::from(""),
        Line::from("You'll choose:"),
        Line::from("  • Color theme"),
        Line::from("  • ASCII logo"),
        Line::from("  • Layout style"),
        Line::from("  • Panel fields"),
        Line::from(""),
        Line::from(Span::styled("Press Enter or Space to begin.", Style::default().fg(TuiColor::Green))),
        Line::from(Span::styled("Press q or Esc to quit.", Style::default().fg(TuiColor::Gray))),
    ]);
    let widget = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Welcome").border_type(BorderType::Rounded));
    frame.render_widget(widget, area);
}

// ── Theme selection ──────────────────────────────────────────────────────

fn render_theme_selection(frame: &mut Frame, area: Rect, app: &mut App) {
    let themes = theme::all_themes();
    let items: Vec<ListItem> = themes.iter().enumerate().map(|(_i, t)| {
        let color_preview: Vec<Span> = t.colors.iter().map(|c| {
            Span::styled("  ", Style::default().bg(TuiColor::Rgb(c.r, c.g, c.b)))
        }).collect();

        let mut spans = vec![
            Span::raw("  "),
            Span::styled(t.name, Style::default().bold()),
            Span::raw("  "),
        ];
        spans.extend(color_preview);
        spans.push(Span::raw(format!("  {}", t.description)));
        ListItem::new(Line::from(spans))
    }).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Theme").border_type(BorderType::Rounded))
        .highlight_style(Style::default().bg(TuiColor::Rgb(60, 60, 80)));

    frame.render_stateful_widget(list, area, &mut app.theme_list_state);
}

// ── ASCII selection ──────────────────────────────────────────────────────

fn render_ascii_selection(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(1)])
        .split(area);

    // Tabs / mode selection
    let mode_text = match app.input_mode {
        InputMode::Normal => format!(
            " [b] Built-in  |  [c] Custom file  |  [p] Paste  |  [d] Disable  (current: {})",
            match &app.ascii_source {
                AsciiSource::Builtin(k) => k.as_str(),
                AsciiSource::CustomFile => "custom file",
                AsciiSource::Pasted(_) => "pasted",
                AsciiSource::Disabled => "disabled",
            }
        ),
        InputMode::EditingCustomPath => {
            format!(" Enter file path: {}", app.custom_ascii_path)
        }
        InputMode::EditingPastedAscii => {
            format!(" Paste ASCII (Enter to finish): {}", app.pasted_ascii)
        }
    };
    let mode_widget = Paragraph::new(Span::raw(mode_text))
        .block(Block::default().borders(Borders::ALL).title("ASCII Art").border_type(BorderType::Rounded));
    frame.render_widget(mode_widget, chunks[0]);

    // Content: logo list or input
    if app.input_mode == InputMode::Normal {
        match &app.ascii_source {
            AsciiSource::Builtin(_) => {
                let items: Vec<ListItem> = app.logo_keys.iter().map(|k| {
                    ListItem::new(format!("  {}", k))
                }).collect();
                let list = List::new(items)
                    .block(Block::default().borders(Borders::NONE))
                    .highlight_style(Style::default().bg(TuiColor::Rgb(60, 60, 80)));
                frame.render_stateful_widget(list, chunks[1], &mut app.logo_list_state);
            }
            AsciiSource::CustomFile => {
                let text = Paragraph::new(Text::from(vec![
                    Line::from(Span::raw("Custom file selected.")),
                    Line::from(Span::raw("Press 'c' to change the path.")),
                ]));
                frame.render_widget(text, chunks[1]);
            }
            AsciiSource::Pasted(art) => {
                let preview_lines: Vec<Line> = art.lines().map(|l| Line::from(Span::raw(l))).collect();
                let text = Text::from(preview_lines);
                let paragraph = Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL).title("Pasted ASCII"));
                frame.render_widget(paragraph, chunks[1]);
            }
            AsciiSource::Disabled => {
                let text = Paragraph::new("ASCII art disabled. Panels only.");
                frame.render_widget(text, chunks[1]);
            }
        }
    }
}

// ── Layout selection ─────────────────────────────────────────────────────

fn render_layout_selection(frame: &mut Frame, area: Rect, app: &mut App) {
    let layouts = AppLayout::variants();
    let items: Vec<ListItem> = layouts.iter().map(|l| {
        let desc = l.description();
        ListItem::new(vec![
            Line::from(Span::styled(format!("  {} ", l.name()), Style::default().bold())),
            Line::from(Span::raw(format!("   {}  ", desc))),
        ])
    }).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Layout").border_type(BorderType::Rounded))
        .highlight_style(Style::default().bg(TuiColor::Rgb(60, 60, 80)));

    frame.render_stateful_widget(list, area, &mut app.layout_list_state);
}

// ── Panel editor ─────────────────────────────────────────────────────────

fn render_panel_editor(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left panel fields
    let left_items: Vec<ListItem> = app.cfg.display.left.iter().map(|f| {
        let check = if f.enabled { "✓" } else { " " };
        ListItem::new(format!(" [{}] {} ({})  {}", check, f.field, f.icon, f.label))
    }).collect();
    let left_list = List::new(left_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(match app.panel_focus {
                PanelFocus::Left => "Left Panel [focused]",
                PanelFocus::Right => "Left Panel",
            })
            .border_style(match app.panel_focus {
                PanelFocus::Left => Style::default().fg(TuiColor::Cyan),
                PanelFocus::Right => Style::default(),
            }))
        .highlight_style(Style::default().bg(TuiColor::Rgb(60, 60, 80)));
    frame.render_stateful_widget(left_list, chunks[0], &mut app.panel_left_state);

    // Right panel fields
    let right_items: Vec<ListItem> = app.cfg.display.right.iter().map(|f| {
        let check = if f.enabled { "✓" } else { " " };
        ListItem::new(format!(" [{}] {} ({})  {}", check, f.field, f.icon, f.label))
    }).collect();
    let right_list = List::new(right_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(match app.panel_focus {
                PanelFocus::Right => "Right Panel [focused]",
                PanelFocus::Left => "Right Panel",
            })
            .border_style(match app.panel_focus {
                PanelFocus::Right => Style::default().fg(TuiColor::Cyan),
                PanelFocus::Left => Style::default(),
            }))
        .highlight_style(Style::default().bg(TuiColor::Rgb(60, 60, 80)));
    frame.render_stateful_widget(right_list, chunks[1], &mut app.panel_right_state);
}

// ── Summary ──────────────────────────────────────────────────────────────

fn render_summary(frame: &mut Frame, area: Rect, app: &App) {
    let themes = theme::all_themes();

    let current_theme_name = themes.iter()
        .find(|t| t.colors == app.cfg.logo.colors)
        .map(|t| t.name)
        .unwrap_or("custom");

    let layout_info = format!("gap={}, padding={}", app.cfg.panel.gap, app.cfg.panel.left_pad);

    let logo_info = match &app.ascii_source {
        AsciiSource::Builtin(k) => format!("Built-in: {}", k),
        AsciiSource::CustomFile => format!("Custom file: {}", app.cfg.logo.path),
        AsciiSource::Pasted(_) => "Pasted ASCII".into(),
        AsciiSource::Disabled => "Disabled".into(),
    };

    let enabled_count = app.cfg.display.left.iter().filter(|f| f.enabled).count()
        + app.cfg.display.right.iter().filter(|f| f.enabled).count();

    let text = Text::from(vec![
        Line::from(""),
        Line::from(Span::styled("  Configuration Summary", Style::default().bold().fg(TuiColor::Cyan))),
        Line::from(""),
        Line::from(format!("  Theme:     {}", current_theme_name)),
        Line::from(format!("  ASCII:     {}", logo_info)),
        Line::from(format!("  Layout:    {}", layout_info)),
        Line::from(format!("  Panels:    {} enabled fields", enabled_count)),
        Line::from(format!("  Config:    ~/.config/atlasfetch/config.json")),
        Line::from(""),
        Line::from(Span::styled("  Press Enter or 's' to save and exit.", Style::default().fg(TuiColor::Green))),
        Line::from(Span::styled("  Press 'n' or Esc to exit without saving.", Style::default().fg(TuiColor::Gray))),
    ]);

    let widget = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Summary").border_type(BorderType::Rounded));
    frame.render_widget(widget, area);
}

// ── Live preview ─────────────────────────────────────────────────────────

fn render_preview(frame: &mut Frame, area: Rect, app: &App) {
    // For ASCII step, show only the art centered and clean
    if app.step == Step::Ascii {
        render_ascii_only_preview(frame, area, app);
        return;
    }

    // Build a minimal SysInfo for preview
    let info = info::SysInfo {
        os: "CachyOS".into(),
        host: "atlasbox".into(),
        user: "charlie".into(),
        kernel: "7.1.3".into(),
        uptime: "2h 14m".into(),
        packages: "1766".into(),
        shell: "fish".into(),
        terminal: "kitty".into(),
        cpu: "AMD Ryzen 3 2200G".into(),
        gpu: "Radeon Vega 8".into(),
        memory: "4.9/13.6G".into(),
        disk: "28/219G".into(),
        wm: "Hyprland".into(),
        load: "0.42".into(),
        processes: "342".into(),
        local_ip: String::new(),
        resolution: String::new(),
        de: String::new(),
        font: String::new(),
    };

    let preview_lines = render::render_preview(&app.cfg, &info, &app.current_ascii, area.width.saturating_sub(2));

    // Convert StyledLines to ratatui Text
    let lines: Vec<Line> = preview_lines.iter().map(|sl| {
        let spans: Vec<Span> = sl.segments.iter().map(|seg| {
            let mut style = Style::default();
            if let Some(fg) = &seg.fg {
                style = style.fg(TuiColor::Rgb(fg.r, fg.g, fg.b));
            }
            if let Some(bg) = &seg.bg {
                style = style.bg(TuiColor::Rgb(bg.r, bg.g, bg.b));
            }
            if seg.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            Span::styled(seg.text.clone(), style)
        }).collect();
        Line::from(spans)
    }).collect();

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("Preview").border_type(BorderType::Rounded));
    frame.render_widget(paragraph, area);
}

/// Render only the ASCII art centered in the preview area (for the ASCII step).
fn render_ascii_only_preview(frame: &mut Frame, area: Rect, app: &App) {
    if app.current_ascii.is_empty() {
        let text = Paragraph::new("No ASCII art selected.")
            .block(Block::default().borders(Borders::ALL).title("ASCII Preview").border_type(BorderType::Rounded));
        frame.render_widget(text, area);
        return;
    }

    let lines: Vec<&str> = app.current_ascii.lines().collect();
    let logo_width = lines.iter().map(|l| unicode_width::UnicodeWidthStr::width(*l)).max().unwrap_or(0);
    let colors = &app.cfg.logo.colors;

    let styled_lines: Vec<Line> = lines.iter().enumerate().map(|(i, raw)| {
        let trimmed = raw.trim_end();
        let vis = unicode_width::UnicodeWidthStr::width(trimmed);
        let pad = logo_width.saturating_sub(vis);
        let padded = format!("{}{}", trimmed, " ".repeat(pad));
        let color = if !colors.is_empty() {
            colors[i % colors.len()]
        } else {
            Color::new(255, 255, 255)
        };
        let spans: Vec<Span> = padded.chars().map(|ch| {
            if ch != ' ' {
                Span::styled(ch.to_string(), Style::default().fg(TuiColor::Rgb(color.r, color.g, color.b)))
            } else {
                Span::raw(" ")
            }
        }).collect();
        Line::from(spans)
    }).collect();

    let paragraph = Paragraph::new(Text::from(styled_lines))
        .block(Block::default().borders(Borders::ALL).title("ASCII Preview").border_type(BorderType::Rounded))
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(paragraph, area);
}

// ── Footer / navigation bar ──────────────────────────────────────────────

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let nav = match app.step {
        Step::Welcome => " [Enter/Space] Begin  |  [q/Esc] Quit".to_string(),
        Step::Theme | Step::Ascii | Step::Layout | Step::Panels => {
            let prev = if app.step.prev().is_some() { "[←] Back" } else { "" };
            let next = if app.step.next().is_some() { "[→/Tab] Next" } else { "[Enter] Save" };
            let extra = match app.step {
                Step::Ascii => "  |  [c]ustom  [p]aste  [d]isable".to_string(),
                Step::Panels => "  |  [Space] toggle  [r] reorder up  [Tab] switch side".to_string(),
                _ => String::new(),
            };
            format!(" {}  |  {}  {}", prev, next, extra)
        }
        Step::Summary => " [Enter/s] Save & exit  |  [n/Esc] Discard & exit".to_string(),
    };

    let footer = Paragraph::new(Span::raw(nav))
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded));
    frame.render_widget(footer, area);
}

// ── Overlay ──────────────────────────────────────────────────────────────

fn render_overlay(frame: &mut Frame, area: Rect, message: &str, color: TuiColor) {
    // Center the overlay
    let overlay_width = message.len().min(60) as u16 + 4;
    let overlay_height = 5;
    let x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.y + (area.height.saturating_sub(overlay_height)) / 2;

    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(TuiColor::Black).fg(color));

    let text = Paragraph::new(Text::styled(message, Style::default().fg(color)))
        .block(block);

    // Clear area and render overlay
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(text, overlay_area);
}
