// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;

use async_trait::async_trait;
use crossterm::event::DisableMouseCapture;
use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use futures::StreamExt;
use tokio::select;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tui::backend::Backend;
use tui::backend::CrosstermBackend;
use tui::layout::Alignment;
use tui::layout::Constraint;
use tui::layout::Direction;
use tui::layout::Layout;
use tui::style::Color;
use tui::style::Modifier;
use tui::style::Style;
use tui::text::Span;
use tui::text::Spans;
use tui::widgets::Block;
use tui::widgets::Borders;
use tui::widgets::Paragraph;
use tui::widgets::Row;
use tui::widgets::Table;
use tui::widgets::TableState;
use tui::widgets::Wrap;
use tui::Frame;
use tui::Terminal;

use crate::duration::duration_string;
use crate::helm::HelmResult;
use crate::helm::Installation;
use crate::helm::InstallationId;
use crate::layer::LogEntry;
use crate::Task;

use super::Message;
use super::Output;
use super::Sender;
use crate::layer::log;

pub struct TuiOutput {
    thread: Option<JoinHandle<Result<()>>>,
}

struct StatefulList<T> {
    state: TableState,
    items: Vec<T>,
}

impl<T> StatefulList<T> {
    fn with_items(items: Vec<T>, selected: Option<usize>) -> StatefulList<T> {
        let mut state = TableState::default();
        state.select(selected);
        StatefulList { state, items }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            _ if self.items.is_empty() => None,
            Some(i) => {
                if i >= self.items.len().saturating_sub(1) {
                    Some(0)
                } else {
                    Some(i.saturating_add(1))
                }
            }
            None => Some(0),
        };
        self.state.select(i);
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            _ if self.items.is_empty() => None,
            Some(i) => {
                if i == 0 {
                    Some(self.items.len().saturating_sub(1))
                } else {
                    Some(i.saturating_sub(1))
                }
            }
            None => Some(0),
        };
        self.state.select(i);
    }

    fn deselect(&mut self) {
        self.state.select(None);
    }

    fn get_selected(&self) -> Option<&T> {
        self.state.selected().and_then(|i| self.items.get(i))
    }
}

fn ui<B: Backend>(f: &mut Frame<B>, state: &mut State) {
    let size = f.size();

    let border_style = match (state.stop_requested, state.finished, state.has_errors) {
        (true, _, _) => Style::default().fg(Color::DarkGray),
        (false, _, true) => Style::default().fg(Color::Red),
        (false, true, false) => Style::default().fg(Color::Green),
        (false, false, false) => Style::default(),
    };

    let task = match state.task {
        Some(Task::Diff) => "diff",
        Some(Task::Upgrade) => "upgrade",
        Some(Task::Test) => "test",
        Some(Task::Template) => "template",
        Some(Task::Outdated) => "outdated",
        None => "unknown",
    };

    let title_status = match (state.stop_requested, state.finished, state.has_errors) {
        (true, _, _) => "EXIT REQUESTED",
        (false, _, true) => "ERRORS",
        (false, true, false) => "SUCCESS",
        (false, false, false) => "",
    };

    let now = Instant::now();
    let duration = match (state.start_instant, state.finished_duration) {
        (None, None) => String::new(),
        (_, Some(duration)) => duration_string(&duration),
        (Some(start_instant), None) => duration_string(&(now - start_instant)),
    };

    let title = ["helmci", task, title_status, &duration];
    let title: Vec<&str> = title.iter().filter(|v| !v.is_empty()).copied().collect();
    let title = title.join(" - ");

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);
    f.render_widget(block, size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)].as_ref())
        .split(f.size());

    let job_or_none = if let Some(id) = state.jobs.state.selected() {
        state.jobs.items.get(id)
    } else {
        None
    };

    let default_text: &dyn HasMultilineText = if let Some(job) = job_or_none {
        job.as_ref()
    } else {
        &state.logs
    };

    let text: &dyn HasMultilineText = match state.mode {
        DisplayMode::Installations => {
            let items: Vec<Row> = state.jobs.items.iter().map(|i| i.get_row(state)).collect();
            let list = installations_to_table(items, border_style);
            f.render_stateful_widget(list, chunks[0], &mut state.jobs.state);
            default_text
        }

        DisplayMode::Commands => {
            let items: Vec<Row> = state
                .selected_commands
                .items
                .iter()
                .map(|i| i.get_row(state))
                .collect();

            let title = job_or_none.map_or_else(
                || Spans::from("Commands"),
                |job| {
                    let mut spans = Spans::from(Span::styled("Commands: ", Style::default()));
                    spans.0.extend(job.get_title(state).0);
                    spans
                },
            );

            let list = commands_to_table(items, title, border_style);

            f.render_stateful_widget(list, chunks[0], &mut state.selected_commands.state);

            if let Some(id) = state.selected_commands.state.selected() {
                state
                    .selected_commands
                    .items
                    .get(id)
                    .map_or(default_text, |command| command.as_ref())
            } else {
                default_text
            }
        }
    };

    let paragraph = Paragraph::new(text.get_text(state))
        .block(
            Block::default()
                .title(text.get_title(state))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .scroll(state.scroll);
    f.render_widget(paragraph, chunks[1]);
}

fn commands_to_table<'a>(items: Vec<Row<'a>>, title: Spans<'a>, border_style: Style) -> Table<'a> {
    let list = Table::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .header(Row::new(vec!["Name", "Duration", "Cmd"]).style(Style::default().fg(Color::Yellow)))
        .widths(&[
            Constraint::Length(20),
            Constraint::Length(8),
            Constraint::Percentage(100),
        ])
        .column_spacing(1)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::ITALIC)
                .fg(Color::Black)
                .bg(Color::White),
        )
        .highlight_symbol(">>");
    list
}

fn installations_to_table(items: Vec<Row>, border_style: Style) -> Table {
    let list = Table::new(items)
        .block(
            Block::default()
                .title("Jobs")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .header(
            Row::new(vec!["Context", "Namespace", "Release", "Duration"])
                .style(Style::default().fg(Color::Yellow)),
        )
        .widths(&[
            Constraint::Length(60),
            Constraint::Length(20),
            Constraint::Length(30),
            Constraint::Length(8),
        ])
        .column_spacing(1)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::ITALIC)
                .fg(Color::Black)
                .bg(Color::White),
        )
        .highlight_symbol(">>");
    list
}

trait HasRows {
    fn get_row(&self, state: &State) -> Row;
}

trait HasMultilineText {
    fn get_title(&self, state: &State) -> Spans;
    // fn get_block_title(&self) -> String;
    fn get_text<'a>(&'a self, state: &'a State) -> Vec<Spans<'a>>;
}

fn key_value_space(key: &str, value: String) -> Spans {
    let spans = vec![
        Span::styled(format!("{key}: "), Style::default().fg(Color::Blue)),
        Span::styled(value, Style::default().fg(Color::White)),
    ];

    Spans::from(spans)
}

impl HasMultilineText for Logs {
    fn get_title(&self, _state: &State) -> Spans {
        Spans::from(Span::styled("Global Logs", Style::default()))
    }

    fn get_text(&self, _state: &State) -> Vec<Spans> {
        let result: Vec<Spans> = self
            .0
            .iter()
            .map(|entry| {
                let style = get_log_style(entry.level);
                Spans::from(vec![
                    Span::styled(&entry.name, style),
                    Span::styled(" - ", style),
                    Span::styled(&entry.message, style),
                ])
            })
            .collect();
        result
    }

    // fn get_block_title(&self) -> String {
    //     "Logs".to_string()
    // }
}

impl HasRows for Installation {
    fn get_row(&self, state: &State) -> Row {
        let status = state.job_status.get(&self.id);
        let style = match status {
            // Some(JobStatus::Skipped) => Style::default().fg(Color::DarkGray),
            Some(JobStatus::New) => Style::default().fg(Color::White),
            Some(JobStatus::Started(_)) => Style::default().fg(Color::Yellow),
            Some(JobStatus::Finished(true, _)) => Style::default().fg(Color::Green),
            Some(JobStatus::Finished(false, _)) => Style::default().fg(Color::Red),
            None => Style::default().fg(Color::Blue),
        };
        let duration = match status {
            Some(JobStatus::Started(start_instant)) => Some(Instant::now() - *start_instant),
            Some(JobStatus::Finished(_, duration)) => Some(*duration),
            _ => None,
        };

        let duration = duration.map_or_else(
            || Span::styled("", style),
            |duration| Span::styled(duration_string(&duration), style),
        );
        let spans = vec![
            Span::styled(self.context.clone(), style),
            Span::styled(self.namespace.clone(), style),
            Span::styled(self.name.clone(), style),
            duration,
        ];
        Row::new(spans)
    }
}

impl HasMultilineText for Installation {
    fn get_title(&self, state: &State) -> Spans {
        let status = state.job_status.get(&self.id);
        let style = match status {
            // Some(JobStatus::Skipped) => Style::default().fg(Color::DarkGray),
            Some(JobStatus::New) => Style::default().fg(Color::White),
            Some(JobStatus::Started(_)) => Style::default().fg(Color::Yellow),
            Some(JobStatus::Finished(true, _)) => Style::default().fg(Color::Green),
            Some(JobStatus::Finished(false, _)) => Style::default().fg(Color::Red),
            None => Style::default().fg(Color::Blue),
        };
        Spans::from(Span::styled(self.name.clone(), style))
    }

    fn get_text<'a>(&'a self, state: &'a State) -> Vec<Spans<'a>> {
        let job_status = state.job_status.get(&self.id);

        let status = job_status.map_or_else(
            || "No status found".to_string(),
            |status| format!("{status:?}"),
        );

        let duration = if let Some(JobStatus::Finished(_, duration)) = job_status {
            duration_string(duration)
        } else {
            String::new()
        };

        let mut lines = vec![
            key_value_space("Name", self.name.clone()),
            key_value_space("Namespace", self.namespace.clone()),
            key_value_space("Context", self.context.clone()),
            key_value_space("Status", status),
            key_value_space("Duration", duration),
            key_value_space("Values", format!("{:?}", self.values_files)),
            key_value_space("Chart", format!("{:?}", self.chart)),
            key_value_space("Depends", format!("{:?}", self.depends)),
            key_value_space("Timeout", self.timeout.to_string()),
        ];

        for result in &state.commands {
            let list_of_spans = result.get_text(state);

            if result.installation.id == self.id {
                lines.push(Spans::from(""));
                lines.push(Spans::from(""));
                lines.push(Spans::from(Span::styled(
                    format!("----- START COMMAND: {} ----", result.command),
                    Style::default().fg(Color::Yellow),
                )));
                lines.extend(list_of_spans);
                lines.push(Spans::from(Span::styled(
                    format!("----- END COMMAND: {} ----", result.command),
                    Style::default().fg(Color::Yellow),
                )));
            }
        }

        lines
    }

    // fn get_block_title(&self) -> String {
    //     format!("Job Details: {}", self.name)
    // }
}

impl HasRows for HelmResult {
    fn get_row(&self, _state: &State) -> Row {
        let duration = self.duration();
        let cmd = self.command_line();

        let style = if self.is_err() {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        let spans = vec![
            Span::styled(self.command.to_string(), style),
            Span::styled(duration_string(&duration), style),
            Span::styled(cmd.to_string(), style),
        ];
        Row::new(spans)
    }
}

impl HasMultilineText for HelmResult {
    fn get_title(&self, _state: &State) -> Spans {
        let str = self.command.to_string();
        let span = if self.is_err() {
            Span::styled(str, Style::default().fg(Color::Red))
        } else {
            Span::styled(str, Style::default().fg(Color::Green))
        };
        Spans::from(span)
    }

    fn get_text(&self, _state: &State) -> Vec<Spans> {
        let mut lines = vec![key_value_space("Result", self.result_line())];
        lines.push(key_value_space("Cmd", self.command_line().to_string()));

        let stdout = self.stdout();
        let stderr = self.stderr();

        lines.push(Spans::from(Span::styled(
            "stdout:",
            Style::default().fg(Color::Blue),
        )));
        lines.extend(format_lines(stdout, Style::default().fg(Color::White)));
        lines.push(Spans::from(Span::styled(
            "stderr:",
            Style::default().fg(Color::Blue),
        )));
        lines.extend(format_lines(stderr, Style::default().fg(Color::White)));

        lines.push(key_value_space(
            "Duration",
            duration_string(&self.duration()),
        ));
        lines
    }

    // fn get_block_title(&self) -> String {
    //     "Command Output".to_string()
    // }
}

fn get_log_style(level: tracing::Level) -> Style {
    let style = Style::default();
    match level {
        tracing::Level::ERROR => style.fg(Color::Red),
        tracing::Level::WARN => style.fg(Color::Yellow),
        tracing::Level::INFO => style.fg(Color::Green),
        tracing::Level::DEBUG => style.fg(Color::White),
        tracing::Level::TRACE => style.fg(Color::DarkGray),
    }
}
fn format_lines(str: &str, style: Style) -> Vec<Spans> {
    let mut lines = vec![];
    for line in str.lines() {
        lines.push(Spans::from(vec![
            Span::styled("---> ", Style::default().fg(Color::DarkGray)),
            Span::styled(line, style),
        ]));
    }
    lines
}

#[derive(Debug)]
enum JobStatus {
    // Skipped,
    New,
    Started(Instant),
    Finished(bool, Duration),
}

enum DisplayMode {
    Installations,
    Commands,
}

struct Logs(Vec<LogEntry>);

impl Logs {
    const fn new() -> Logs {
        Logs(vec![])
    }

    fn add_log(&mut self, entry: LogEntry) {
        self.0.push(entry);
    }
}

struct State {
    task: Option<Task>,
    start_instant: Option<Instant>,
    mode: DisplayMode,
    jobs: StatefulList<Arc<Installation>>,
    job_status: HashMap<InstallationId, JobStatus>,
    commands: Vec<Arc<HelmResult>>,
    selected_commands: StatefulList<Arc<HelmResult>>,
    logs: Logs,
    scroll: (u16, u16),
    finished: bool,
    finished_duration: Option<Duration>,
    stop_requested: bool,
    has_errors: bool,
    reader: EventStream,
}

impl State {
    fn new() -> State {
        State {
            task: None,
            start_instant: None,
            mode: DisplayMode::Installations,
            jobs: StatefulList::with_items(vec![], None),
            job_status: HashMap::new(),
            commands: vec![],
            selected_commands: StatefulList::with_items(vec![], Some(0)),
            logs: Logs::new(),
            scroll: (0, 0),
            finished: false,
            finished_duration: None,
            stop_requested: false,
            has_errors: false,
            reader: EventStream::new(),
        }
    }

    fn add_command(&mut self, ir: Arc<HelmResult>) {
        if let Some(job) = self.jobs.get_selected() {
            if job.id == ir.installation.id {
                self.selected_commands.items.push(ir.clone());
            }
        }
        self.commands.push(ir);
    }

    fn update_selected_items(&mut self, installation: Option<Arc<Installation>>) {
        let list = if let Some(installation) = installation {
            let list = self
                .commands
                .iter()
                .filter(|item| item.installation.id == installation.id)
                .cloned()
                .collect();
            list
        } else {
            vec![]
        };
        self.selected_commands.items = list;
    }
}

pub fn start() -> Result<(TuiOutput, Sender)> {
    let stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    execute!(terminal.backend_mut(), EnterAlternateScreen).unwrap();
    terminal.clear()?;
    terminal.hide_cursor()?;

    let (tx, rx) = mpsc::channel(50);
    let thread: JoinHandle<Result<()>> = tokio::spawn(async move {
        let state: State = State::new();

        process_events(rx, &mut terminal, state).await?;

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )
        .unwrap();
        terminal.show_cursor()?;
        Ok(())
    });

    let rc = TuiOutput {
        thread: Some(thread),
    };

    Ok((rc, tx))
}

async fn process_events(
    mut rx: mpsc::Receiver<Arc<Message>>,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut state: State,
) -> Result<()> {
    loop {
        select! {
            Some(msg) = rx.recv() => {
                process_message(&msg, &mut state);
            }
            Some(Ok(event)) = state.reader.next() => {
                process_event(&event, &mut state);
            }
            else => {
                // This shouldn't happen, but if it does, just exit.
                state.stop_requested = true;
                state.finished = true;
            }
        }

        terminal.draw(|f| {
            ui(f, &mut state);
        })?;

        if state.stop_requested && state.finished {
            break;
        }
    }

    Ok(())
}

#[async_trait]
impl Output for TuiOutput {
    async fn wait(&mut self) -> Result<()> {
        if let Some(thread) = self.thread.take() {
            thread.await??;
        }
        Ok(())
    }
}

fn process_message(msg: &Arc<Message>, state: &mut State) {
    match msg.as_ref() {
        Message::SkippedJob(i) => {
            let str = format!("Skipped Job {}", i.name);
            state.logs.add_log(log(tracing::Level::INFO, &str));
            // Need to think about if we want skipped jobs to appear in list or not
            // state.job_status.insert(i.uuid, JobStatus::Skipped);
            // state.jobs.items.push(i);
        }
        Message::InstallationVersion(i, our_version, upstream_version) => {
            if our_version != upstream_version {
                let str = format!(
                    "Installation {} our version {our_version} upstream version {upstream_version}",
                    i.name
                );
                state.logs.add_log(log(tracing::Level::INFO, &str));
            }
        }
        Message::NewJob(i) => {
            let str = format!("Job {}", i.name);
            state.logs.add_log(log(tracing::Level::DEBUG, &str));
            state.job_status.insert(i.id, JobStatus::New);
            state.jobs.items.push(i.clone());
        }
        Message::InstallationResult(ir) => {
            state.add_command(ir.clone());
        }
        Message::StartedJob(i, start_instant) => {
            let str = format!("Started {}", i.name);
            state.logs.add_log(log(tracing::Level::INFO, &str));
            state
                .job_status
                .insert(i.id, JobStatus::Started(*start_instant));
        }
        Message::FinishedJob(i, result, duration) => {
            let str = format!("Finished {}", i.name);
            state.logs.add_log(log(tracing::Level::INFO, &str));
            state
                .job_status
                .insert(i.id, JobStatus::Finished(result.is_ok(), *duration));
            if result.is_err() {
                state.has_errors = true;
            }
        }
        Message::Log(entry) => {
            state.logs.add_log(entry.clone());
        }
        Message::Start(task, start_instant) => {
            state.task = Some(*task);
            state.start_instant = Some(*start_instant);
        }
        Message::FinishedAll(result, duration) => {
            if result.is_err() {
                state.has_errors = true;
            };
            let str = format!("Finished Everything {}", duration_string(duration));
            state.logs.add_log(log(tracing::Level::INFO, &str));
            state.finished = true;
            state.finished_duration = Some(*duration);
        }
    }
}

fn process_event(event: &Event, state: &mut State) {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Char('q') => {
                state.stop_requested = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.stop_requested = true;
            }
            KeyCode::Char('l') => {
                state.stop_requested = false;
                state.mode = DisplayMode::Installations;
                state.jobs.deselect();
                state.scroll = (0, 0);
            }
            KeyCode::Esc => match state.mode {
                DisplayMode::Installations => {
                    state.stop_requested = !state.stop_requested;
                }
                DisplayMode::Commands => {
                    state.stop_requested = false;
                    state.mode = DisplayMode::Installations;
                    state.scroll = (0, 0);
                }
            },
            KeyCode::Down => {
                state.stop_requested = false;
                match state.mode {
                    DisplayMode::Installations => {
                        state.jobs.next();
                        let installation_or_none = state.jobs.get_selected().cloned();
                        state.update_selected_items(installation_or_none);
                    }
                    DisplayMode::Commands => state.selected_commands.next(),
                }

                state.scroll = (0, 0);
            }
            KeyCode::Up => {
                state.stop_requested = false;
                match state.mode {
                    DisplayMode::Installations => {
                        state.jobs.previous();
                        let installation_or_none = state.jobs.get_selected().cloned();
                        state.update_selected_items(installation_or_none);
                    }
                    DisplayMode::Commands => state.selected_commands.previous(),
                }
                state.scroll = (0, 0);
            }
            KeyCode::Enter => match state.mode {
                DisplayMode::Installations => {
                    state.stop_requested = false;
                    let installation_or_none = state.jobs.get_selected();
                    if installation_or_none.is_some() {
                        state.mode = DisplayMode::Commands;
                        state.scroll = (0, 0);
                        state.selected_commands.state.select(Some(0));
                    }
                }
                DisplayMode::Commands => {
                    state.stop_requested = false;
                }
            },
            KeyCode::PageUp => {
                state.stop_requested = false;
                let y = state.scroll.0.saturating_sub(10);
                state.scroll = (y, state.scroll.1);
            }
            KeyCode::PageDown => {
                state.stop_requested = false;
                let y = state.scroll.0.saturating_add(10);
                state.scroll = (y, state.scroll.1);
            }
            KeyCode::Left => {
                state.stop_requested = false;
                let x = state.scroll.1.saturating_sub(10);
                state.scroll = (state.scroll.0, x);
            }
            KeyCode::Right => {
                state.stop_requested = false;
                let x = state.scroll.1.saturating_add(10);
                state.scroll = (state.scroll.0, x);
            }
            _ => {
                state.stop_requested = false;
            }
        },
        Event::Mouse(_mouse) => {}
        Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
    }
}
