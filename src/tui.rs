//! # Tui
//!
//! Cooks up a **terminal user interface** using [`RataTui`](ratatui),
//! and houses its render loop and event handling.
//!
//! The `Tui` is drawn to [`Stdout`] and uses [`Crossterm`](CrosstermBackend) as its backend.
//!
//! The render loop is driven by [`Signal`]s from the [`Gui`] thread,
//! which receive [`Command`]s in response from the [`Tui`] thread,
//! communicating user input and state changes.

use clap::Parser as clap;
use ratatui::{
    Frame, Terminal,
    crossterm::{
        event::{self, Event, KeyCode, poll},
        execute, terminal,
    },
    layout::{Constraint, Direction, Layout, Rect},
    prelude::{Backend, CrosstermBackend},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph},
};
use std::{
    error::Error,
    io::Stdout,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};
use winit::event_loop::EventLoopProxy;

#[allow(unused_imports)]
use crate::{
    Command, Signal, View,
    config::Config,
    gui::Gui,
    interpreter::InterpreterError,
    interpreter::{BlockSummary, Interpreter},
    lexer::Lexer,
    machine::{CircularDirection, FeedMode, Motion, Positioning},
    machine::{Machine, Unit},
    parser::Plane,
    parser::{CodeBlock, MCode, Parser, Point},
    source::Source,
};

/// Maximum number of [`Block`]s from [`Source`] visible ahead of the current block.
const MAX_PREVIEW_AHEAD: usize = 10;

/// Represents the types of program cycle interruptions.
/// These interruptions need user input to be removed and resume cycle.
pub enum Interrupt {
    /// Confirm program start or restart.
    Start,
    /// M00 program stop detected.
    Stop,
    /// M01 optional program stop detected.
    OptionalStop,
    /// M30 Program end detected.
    End,
}

/// Represents the current state of the [`Tui`](crate::tui).
pub struct Tui {
    /// Receiver end of the channel for [`Signal`] from [`Gui`].
    signal: Receiver<Signal>,
    /// Event proxy for sending [`Command`]s to [`Gui`].
    proxy: EventLoopProxy<Command>,
    /// Current selected [`View`].
    view: View,
    /// Single step through code blocks.
    single: bool,
    /// Parsed source loaded [`Interpreter`], ready for iteration.
    interpreter: Interpreter,
    /// Index of current block being executed for preview.
    current: usize,
    /// Total number of summaries stored till [`MCode::Stop`] block.
    /// This is only set to [`Some`] once all the blocks have been interpreted once.
    total: Option<usize>,
    /// [`None`] if the program is running.
    interrupt: Option<Interrupt>,
    /// Summaries for all executed blocks.
    /// These stay in memory for the whole life of the program,
    /// making looping for the second times more efficient.
    summaries: Vec<BlockSummary>,
    /// Previously received [`Signal`] from [`Gui`].
    last_signal: Option<Signal>,
    /// Error that is a direct result of [`executing`](Interpreter::execute) a [`CodeBlock`].
    /// This is stored for rendering error to the Tui before exiting to the shell.
    error: Option<InterpreterError>,
}

impl Tui {
    /// Constructs a new [`Tui`] and loads the [`Source`] from file at input path.
    ///
    /// The [`Tui::view`] is set to [`View::default`],
    /// [`Tui::single`] block execution is set to `false`,
    /// and [`Tui::interrupt`] to [`Interrupt::Start`].
    ///
    /// # Errors
    /// Returns [`Error`](anyhow::Error) on failure to read [`Source`] file or build the [`Machine`].
    pub fn build(
        signal: Receiver<Signal>,
        max_travels: Point,
        proxy: EventLoopProxy<Command>,
    ) -> anyhow::Result<Self> {
        let config = Config::parse();
        let src = Source::from_file(&config.filepath)?;

        Ok(Self {
            signal,
            proxy,
            view: View::default(),
            single: false,
            interpreter: Interpreter::new(
                Parser::new(Lexer::new(src)),
                Machine::build(max_travels, Unit::default())?,
            ),
            current: 0,
            total: None,
            interrupt: Some(Interrupt::Start),
            summaries: Vec::new(),
            last_signal: None,
            error: None,
        })
    }

    /// Starts [`Tui`] execution by executing each G-Code line and managing the terminal state.
    ///
    /// The [`Tui`] thread cannot terminate the program now, just by returning an `Error`.
    /// A [`Command::Stop`], with an optional [`Error`](anyhow::Error),
    /// must be sent to the main thread running the [`Gui`],
    /// to tell it to exit the program.
    ///
    /// Therefore, to report any error from this function,
    /// it must be sent to the main [`Gui`] thread.
    pub fn run(mut self) {
        // on failure to prepare terminal, tell main thread to stop and stop current thread
        let mut terminal = match prepare_terminal() {
            Ok(t) => t,
            Err(e) => return self.proxy.send_event(Command::Stop(Some(e))).unwrap(),
        };

        let mut res = self.start_loop(&mut terminal);

        // prioritize terminal error
        if let Err(e) = restore_terminal(terminal) {
            res = Err(e)
        };

        match self.last_signal {
            Some(Signal::Stop) => {} // main thread already signalled to stop
            _ => self.proxy.send_event(Command::Stop(res.err())).unwrap(),
        }
    }

    /// Checks for any updates from the [`Gui`] thread by trying to receive any [`Signal`]
    /// **without blocking** current thread.
    ///
    /// On success, optionally returns a [`Signal`], if received, else returns [`None`].
    ///
    /// # Errors
    /// Returns [`TryRecvError::Disconnected`] if the main thread had already terminated.
    fn check_signal(&mut self) -> Result<Option<Signal>, TryRecvError> {
        match self.signal.try_recv() {
            Ok(signal) => Ok(Some(signal)),
            Err(err) => match err {
                TryRecvError::Empty => Ok(None),
                TryRecvError::Disconnected => Err(err),
            },
        }
    }

    /// Reloads internals of [`Tui`] to begin rendering again from the first block.
    /// Also sends [`Command::Clear`] to clear any toolpath from [`Gui`] screen.
    fn reload(&mut self) {
        self.current = 0;
        self.interrupt = Some(Interrupt::Start);
        self.interpreter.reload();
        self.proxy.send_event(Command::Clear).unwrap();
    }

    fn start_loop<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> anyhow::Result<()>
    where
        anyhow::Error: From<B::Error>,
    {
        // flag to check if last proceed request was fulfilled or not
        // one render command is sent regardless at start of the loop
        let mut proceed = true;

        // the main idea of this loop is that the event loop from main thread,
        // drives this loop with every proceed signal
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            let signal = self.check_signal()?;

            match signal {
                Some(Signal::Proceed) => proceed = true,
                Some(Signal::Stop) => return Ok(()),
                None => (),
            };

            if proceed && !self.single && self.interrupt.is_none() && self.error.is_none() {
                proceed = self.execute();
            } else if self.error.is_some() {
                // poll for enter event or continue and check if the main thread is still running
                if poll(Duration::from_millis(100))? {
                    if let Event::Key(key) = event::read()?
                        && key.kind != event::KeyEventKind::Release
                        && key.code == KeyCode::Enter
                    {
                        return Err(self.error.take().unwrap().into());
                    }
                }
            } else if poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()?
                    && key.kind != event::KeyEventKind::Release
                {
                    match key.code {
                        KeyCode::Char('Q') => return Ok(()),

                        KeyCode::Char('v') => {
                            match self.view {
                                View::Top => self.view = View::Isometric,
                                View::Isometric => self.view = View::Top,
                            };
                            self.proxy.send_event(Command::SetView(self.view)).unwrap();
                        }

                        KeyCode::Char('s') => self.single = !self.single,

                        KeyCode::Char('b') => self
                            .proxy
                            .send_event(Command::ToggleMachineBoundary)
                            .unwrap(),

                        KeyCode::Char('g') => self.proxy.send_event(Command::ToggleGrid).unwrap(),

                        KeyCode::Char('o') => self.proxy.send_event(Command::ToggleOrigin).unwrap(),

                        KeyCode::Char('t') => self.proxy.send_event(Command::ToggleTool).unwrap(),

                        KeyCode::Char('n') if proceed && self.interrupt.is_none() => {
                            proceed = self.execute();
                        }

                        // TODO verify interrupt order
                        KeyCode::Enter => match self.interrupt {
                            Some(Interrupt::End) => self.reload(),
                            Some(Interrupt::Start) => {
                                self.interrupt = None;
                                proceed = self.execute();
                            }
                            Some(_) => self.interrupt = None,
                            None => {}
                        },
                        _ => {}
                    }
                }
            }
        }
    }

    /// Executes the next block, which can be done in two ways:
    /// - For the first pass, each [`CodeBlock`] is executed with [`Interpreter::execute`] and the
    /// resulting [`BlockSummary`] is stored in [`Tui::summaries`].
    /// - For repeat passes, only stored [`BlockSummary`]s are queried and no actual interpretation
    /// or parsing takes place.
    ///
    /// Returns `true` when no [`MotionSummary`](crate::machine::MotionSummary) was found in the
    /// latest [`BlockSummary`], and another block needs to interpreted.
    ///
    /// Returns `false` when a valid [`MotionSummary`](crate::machine::MotionSummary) was found and
    /// sent to the [`Gui`] thread using [`Command::Render`].
    fn execute(&mut self) -> bool {
        if self.interrupt.is_some() {
            return false;
        }

        // branch off on if the results are already stored
        let block = match self.total {
            Some(total) => {
                if self.current > total {
                    unreachable!("Current count will never exceed total count.")
                } else if self.current == total {
                    None // end
                } else {
                    Some(self.summaries[self.current].clone()) // send stored summary
                }
            }
            None => match self.interpreter.execute() {
                Ok(res) => res, // res can be a new summary or None for exhaustion
                Err(e) => {
                    self.error = Some(e);
                    return false;
                }
            },
        };

        match block {
            Some(summary) => {
                let proceed = if let Some(motion) = &summary.motion {
                    self.proxy.send_event(Command::Render(*motion)).unwrap();
                    false
                } else {
                    match summary.mcode {
                        Some(MCode::Stop) => {
                            self.interrupt = Some(Interrupt::Stop);
                            false
                        }
                        Some(MCode::OptionalStop) => {
                            self.interrupt = Some(Interrupt::OptionalStop);
                            false
                        }
                        Some(MCode::End) => {
                            self.interrupt = Some(Interrupt::End);
                            false
                        }
                        Some(_) => true,
                        None => true,
                    }
                };

                if self.total.is_none() {
                    self.summaries.push(summary); // this was a new block
                }

                self.current += 1;

                proceed
            }

            None => {
                self.total = Some(self.current);
                self.interrupt = Some(Interrupt::End);
                false // end of blocks
            }
        }
    }

    /// Prepares individual sections of the terminal screen,
    /// and draws the current state of [`Tui`] in the said sections.
    fn draw(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
            .split(frame.area());

        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[0]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(top_chunks[1]);

        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(10), Constraint::Percentage(90)])
            .split(chunks[1]);

        frame.render_widget(self.main_widget(), top_chunks[0]);
        frame.render_widget(self.preview_widget(), right_chunks[0]);
        frame.render_widget(self.machine_widget(), right_chunks[1]);
        frame.render_widget(self.active_widget(), right_chunks[2]);
        frame.render_widget(self.title_widget(), bottom_chunks[0]);
        frame.render_widget(self.keys_widget(), bottom_chunks[1]);

        // present error, if any
        if let Some(e) = &self.error {
            let mut error_lines = vec![e.to_string()];
            let mut source = e.source();

            while let Some(cause) = source {
                error_lines.push(format!("caused by: {cause}"));
                source = cause.source();
            }

            let popup = Paragraph::new(error_lines.join("\n")).block(
                Block::default()
                    .title("Alarm")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(Color::DarkGray)),
            );

            let area = get_centered(60, 25, frame.area());
            frame.render_widget(popup, area);
        }
    }

    /// Generates a styled [`Paragraph`] using the [`BlockSummary`] for current block.
    fn main_widget(&self) -> Paragraph<'_> {
        if let Some(interrupt) = &self.interrupt {
            let style = Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD);

            let mut interrupt = vec![match interrupt {
                Interrupt::Start => Span::styled("START", style),
                Interrupt::Stop => Span::styled("STOP", style),
                Interrupt::OptionalStop => Span::styled("OPTIONAL STOP", style),
                Interrupt::End => Span::styled("END", style),
            }];

            interrupt.push(" interrupt detected.".into());

            let command = vec![
                "Press ".into(),
                Span::styled("Enter", style),
                " to remove the interrupt.".into(),
            ];

            Paragraph::new(Text::from(vec![interrupt.into(), command.into()]))
                .block(Block::default().style(Style::default()))
                .centered()
        } else {
            let summary = self
                .summaries
                .get(self.current.saturating_sub(1))
                .expect("App module has pushed the text descriptions for the current block.");

            let mut lines = vec![];

            #[allow(irrefutable_let_patterns)]
            if !summary.gcodes.is_empty()
                && let multiple = summary.gcodes.len() > 1
            {
                lines.push(Line::styled(
                    if multiple { "GCODES:" } else { "GCODE:" },
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
                for gcode in &summary.gcodes {
                    lines.push(Line::styled(gcode.to_string(), Style::default()));
                }
                lines.push(Line::from(""));
            };

            if let Some(mcode) = summary.mcode.clone() {
                lines.push(Line::styled(
                    "MCODE:",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
                lines.push(Line::styled(mcode.to_string(), Style::default()));
                lines.push(Line::from(""));
            }

            #[allow(irrefutable_let_patterns)]
            if !summary.codes.is_empty()
                && let multiple = summary.codes.len() > 1
            {
                lines.push(Line::styled(
                    if multiple {
                        "Other CODES:"
                    } else {
                        "Other CODE:"
                    },
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
                for code in &summary.codes {
                    lines.push(Line::styled(code.to_string(), Style::default()));
                }
            };

            Paragraph::new(lines)
        }
    }

    /// Generates a styled [`Paragraph`] with loaded [`Source`].
    /// One line of context is also provided in the preview.
    fn preview_widget(&self) -> Paragraph<'_> {
        let mut lines = vec![];

        let mut current = self.current.saturating_sub(2);

        while let Some(line) = self.interpreter.get_line(current)
            && current < self.current + MAX_PREVIEW_AHEAD
        {
            if current == self.current.saturating_sub(1)
                && !matches!(self.interrupt, Some(Interrupt::Start))
            {
                lines.push(Line::styled(
                    line,
                    Style::default().bg(Color::White).fg(Color::Black),
                ))
            } else {
                lines.push(Line::from(line))
            }

            current += 1;
        }

        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .padding(Padding::horizontal(2))
                    .borders(Borders::TOP | Borders::LEFT)
                    .title(Line::styled("Preview", Style::default().fg(Color::Yellow)).centered())
                    .style(Style::default()),
            )
    }

    /// Generates a styled [`Paragraph`] showing the state of [`Machine`].
    fn machine_widget(&self) -> Paragraph<'_> {
        let machine = self.interpreter.machine();
        let unit = Span::from(match machine.units() {
            Unit::Imperial => "in",
            Unit::Metric => "mm",
        });

        let mut line1 = vec![
            Span::styled(
                "X",
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            ),
            ": ".into(),
            machine.pos().x().to_string().into(),
            unit.clone(),
            " | ".into(),
            Span::styled(
                "Y",
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            ),
            ": ".into(),
            machine.pos().y().to_string().into(),
            unit.clone(),
            " | ".into(),
            Span::styled(
                "Z",
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            ),
            ": ".into(),
            machine.pos().z().to_string().into(),
            unit.clone(),
        ];
        // append feed if available
        if let Some(feed) = machine.feed().clone() {
            line1.extend(
                vec![
                    " | ".into(),
                    Span::styled(
                        "F",
                        Style::default()
                            .fg(Color::LightBlue)
                            .add_modifier(Modifier::BOLD),
                    ),
                    ": ".into(),
                    feed.to_string().into(),
                    unit,
                    Span::from(match machine.feed_mode() {
                        FeedMode::PerMinute => "/min",
                        FeedMode::PerRev => "/rev",
                    }),
                ]
                .into_iter(),
            );
        }

        let line2 = vec![
            Span::styled(
                match machine.motion() {
                    Motion::Rapid => "RAPID",
                    Motion::Feed => "FEED",
                    Motion::Arc(CircularDirection::Clockwise) => "CLOCKWISE",
                    Motion::Arc(CircularDirection::CounterClockwise) => "ANTICLOCKWISE",
                },
                Style::default().fg(Color::Blue),
            ),
            " | ".into(),
            Span::styled(
                match machine.plane() {
                    Plane::XY => "XY",
                    Plane::XZ => "XZ",
                    Plane::YZ => "YZ",
                },
                Style::default().fg(Color::Blue),
            ),
            " | ".into(),
            Span::styled(
                match machine.positioning() {
                    Positioning::Absolute => "ABSOLUTE",
                    Positioning::Incremental => "INCREMENTAL",
                },
                Style::default().fg(Color::Blue),
            ),
            " | ".into(),
            Span::styled(
                match machine.code_units() {
                    Unit::Imperial => "IMPERIAL",
                    Unit::Metric => "METRIC",
                },
                Style::default().fg(Color::Blue),
            ),
        ];

        Paragraph::new(Text::from(vec![line1.into(), line2.into()]))
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::LEFT)
                    .title(
                        Line::styled("Machine State", Style::default().fg(Color::Yellow))
                            .centered(),
                    )
                    .style(Style::default()),
            )
            .centered()
    }

    /// Generates a styled [`Paragraph`] showing the active state of [`Tui`] .
    fn active_widget(&self) -> Paragraph<'_> {
        let mut active = vec![];
        let style = Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD);

        if let Some(interrupt) = &self.interrupt {
            active.push(match interrupt {
                Interrupt::Start => Span::styled("START INTERRUPT", style),

                Interrupt::Stop => Span::styled("STOP INTERRUPT", style),

                Interrupt::OptionalStop => Span::styled("OPTIONAL STOP INTERRUPT", style),

                Interrupt::End => Span::styled("END INTERRUPT", style),
            });
            active.push(Span::from(" | "));
        }

        active.push(match self.view {
            View::Top => Span::styled("TOP", style),
            View::Isometric => Span::styled("ISOMETRIC", style),
        });

        if self.single {
            active.push(Span::from(" | "));
            active.push(Span::styled("SINGLE", style));
        }

        Paragraph::new(Line::from(active))
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::LEFT)
                    .title(Line::styled("Active", Style::default().fg(Color::Yellow)).centered())
                    .style(Style::default()),
            )
            .centered()
    }

    /// Generates a styled [`Paragraph`] with **program title**.
    fn title_widget(&self) -> Paragraph<'_> {
        Paragraph::new("GSim-RS")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::RIGHT)
                    .style(Style::default()),
            )
            .centered()
    }

    /// Generates a styled [`Paragraph`] with **possible keys inputs**.
    fn keys_widget(&self) -> Paragraph<'_> {
        let mut keys = vec![
            Span::styled("Q", Style::default().fg(Color::Yellow)),
            ": quit / ".into(),
            Span::styled("v", Style::default().fg(Color::Yellow)),
            ": toggle view / ".into(),
            Span::styled("s", Style::default().fg(Color::Yellow)),
            ": toggle single / ".into(),
            Span::styled("g", Style::default().fg(Color::Yellow)),
            ": toggle grid / ".into(),
            Span::styled("o", Style::default().fg(Color::Yellow)),
            ": toggle origin / ".into(),
            Span::styled("b", Style::default().fg(Color::Yellow)),
            ": toggle machine boundary / ".into(),
            Span::styled("t", Style::default().fg(Color::Yellow)),
            ": toggle tool".into(),
        ];

        if self.single && self.interrupt.is_none() {
            keys.push(" / ".into());
            keys.push(Span::styled("n", Style::default().fg(Color::Yellow)));
            keys.push(": next block".into());
        }

        Paragraph::new(Line::from(keys))
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::LEFT)
                    .title(Line::styled("Commands", Style::default().fg(Color::Yellow)).centered())
                    .style(Style::default()),
            )
            .centered()
    }
}

/// Prepares the terminal for use with [`Tui`],
/// by enabling raw mode and using alternate screen to preserve shell history.
fn prepare_terminal() -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    let mut stdout = std::io::stdout();

    terminal::enable_raw_mode()?;

    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture
    )?;

    let backend = CrosstermBackend::new(stdout);

    Terminal::new(backend).map_err(|e| e.into())
}

/// Restores the terminal to its previous state and restores the shell history.
fn restore_terminal(mut terminal: Terminal<CrosstermBackend<Stdout>>) -> anyhow::Result<()> {
    terminal::disable_raw_mode()?;

    execute!(
        terminal.backend_mut(),
        terminal::LeaveAlternateScreen,
        event::DisableMouseCapture
    )?;

    terminal.show_cursor()?;

    Ok(())
}

/// Creates a centered [`Rect`] using up a supplied percentages in X and Y.
fn get_centered(x: u16, y: u16, rect: Rect) -> Rect {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - y) / 2),
            Constraint::Percentage(y),
            Constraint::Percentage((100 - y) / 2),
        ])
        .split(rect);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - x) / 2),
            Constraint::Percentage(x),
            Constraint::Percentage((100 - x) / 2),
        ])
        .split(chunks[1])[1] // return the middle chunk
}
