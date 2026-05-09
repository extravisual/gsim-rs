use std::{
    fmt::Display,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

use ratatui::{
    Terminal,
    crossterm::event::{self, Event, KeyCode, poll},
    prelude::Backend,
};
use winit::event_loop::EventLoopProxy;

use crate::{
    Command, Signal,
    config::Config,
    describe::{Describe, Description},
    error::GSimError,
    interpreter::{BlockSummary, Interpreter},
    lexer::Lexer,
    machine::{Machine, Unit},
    parser::{MCode, Parser, Point},
    source::Source,
    ui::ui,
};

/// Represents the types of view possible on the left section.
/// Right section always previews the raw code.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Zeroable)]
pub enum View {
    /// Simlutate `X` & `Y` axes of the [`Machine`], from **top view**.
    Top,
    /// Simuate all three axes, from **isometric view**.
    #[default]
    Isometric,
}

unsafe impl bytemuck::Pod for View {}

/// Represents the types program cycle interruptions that need user input to resume cycle.
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

/// Possible errors that can happen during [`App`] event reading.
#[derive(Debug)]
pub enum AppError {
    IO(std::io::Error),
}

impl Describe for AppError {
    fn describe(&self) -> Description {
        match self {
            AppError::IO(error) => Description::new("Event Read Error Detected", error.to_string()),
        }
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::IO(error) => write!(f, "{}", error.to_string()),
        }
    }
}

/// Represents current state of the program.
pub struct App {
    pub error: Option<GSimError>,
    /// Current selected view.
    pub view: View,
    /// Single step through code blocks.
    pub single: bool,
    /// Parsed source loaded interpreter, ready for iteration.
    pub interpreter: Interpreter,
    /// Index of current block being executed for preview.
    pub current: usize,
    /// Total number of summaries stored till [`MCode::Stop`](crate::parser::MCode::Stop) block.
    /// This is stored on the first pass, so that next passes can know when to issue
    /// [`Interrupt::End`].
    pub total: Option<usize>,
    /// `None` if the program is running.
    pub interrupt: Option<Interrupt>,
    /// Summaries for all executed blocks.
    /// These stay in memory for the whole life of the program,
    /// making looping for the second times more efficient.
    pub summaries: Vec<BlockSummary>,
    /// Send rendering jobs to the [`Winit`](winit) thread.
    pub proxy: EventLoopProxy<Command>,
    /// Proceed and send another job to the [`Winit`](winit) thread.
    pub signal: Receiver<Signal>,
    pub last_signal: Option<Signal>,
}

impl App {
    /// Constructs an [`App`] and loads the [`Source`].
    ///
    /// The [`App::view`] is set to [`View::Text`]
    /// and [`App::single`] block execution is set to `false`.
    ///
    /// Returns [`GSimError`] on failure.
    pub fn build(
        config: Config,
        proxy: EventLoopProxy<Command>,
        signal: Receiver<Signal>,
        max_travels: Point,
    ) -> Result<Self, GSimError> {
        let src = Source::from_file(&config.filepath)?;

        Ok(Self {
            error: None,
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
            proxy,
            signal,
            last_signal: None,
        })
    }

    // check for any updates from the main thread
    fn signal(&mut self) -> anyhow::Result<Option<Signal>> {
        match self.signal.try_recv() {
            Ok(signal) => Ok(Some(signal)),
            Err(err) => match err {
                TryRecvError::Empty => Ok(None),
                TryRecvError::Disconnected => return Err(err.into()),
            },
        }
    }

    pub fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> anyhow::Result<Self>
    where
        anyhow::Error: From<B::Error>,
    {
        // to allow use of ? operator,
        // the parent sends `Command::Stop`

        // if last proceed request was sent or not
        // one render command is sent regardless of receiving proceed signal or not
        let mut pending = true;

        loop {
            terminal.draw(|f| ui(f, &self))?;
            // the main idea of this loop is that the event loop from main thread, drives this loop
            // with every proceed = true

            let signal = self.signal()?;

            // main thread signalled to terminate
            match signal {
                Some(Signal::Proceed) => pending = true,
                Some(Signal::Stop) => return Ok(self),
                None => (),
            };

            if pending && !self.single && self.interrupt.is_none() && self.error.is_none() {
                pending = self.execute();
            } else if self.error.is_some() {
                // if error, then poll for enter event or continue and check if the main thread is
                // still running
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
                    // Skip events that are not KeyEventKind::Press
                    match key.code {
                        KeyCode::Char('Q') => return Ok(self),

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

                        KeyCode::Char('n') if pending && self.interrupt.is_none() => {
                            pending = self.execute();
                        }

                        KeyCode::Enter => match self.interrupt {
                            Some(Interrupt::End) => self.reload(),
                            Some(Interrupt::Start) => {
                                self.interrupt = None;
                                pending = self.execute();
                            }
                            Some(_) => self.interrupt = None,
                            None => {}
                        },
                        _ => {}
                    }
                }
            } else {
                continue;
            }
        }
    }

    fn reload(&mut self) {
        self.current = 0;
        self.interrupt = Some(Interrupt::Start);
        self.interpreter.reload();
        self.proxy.send_event(Command::Clear).unwrap();
    }

    /// Execute a single block from the Parser.
    /// Returns true when no motion was detected, requesting parsing of another block
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
                    // end
                    None
                } else {
                    // send stored summary
                    Some(self.summaries[self.current].clone())
                }
            }
            None => match self.interpreter.execute() {
                // res can be a new summary or None for end
                Ok(res) => res,
                Err(err) => {
                    self.error = Some(err.into());
                    return false;
                }
            },
        };

        match block {
            Some(summary) => {
                let proceed = if let Some(motion) = &summary.motion {
                    self.proxy.send_event(Command::Render(*motion)).unwrap();
                    false
                } else if let Some(mcode) = &summary.mcode
                    && *mcode == MCode::End.to_string()
                {
                    // check the mcode for M30
                    self.interrupt = Some(Interrupt::End);
                    false
                } else {
                    true
                };

                // this was a new block
                if self.total.is_none() {
                    self.summaries.push(summary);
                }

                self.current += 1;

                proceed
            }

            None => {
                // end of blocks
                self.total = Some(self.current);
                self.interrupt = Some(Interrupt::End);
                false
            }
        }
    }
}
