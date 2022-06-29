#![allow(unused)]
use colored::{Color, ColoredString, Colorize};
use std::collections::VecDeque;
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub use colored::control;

pub type BoxedCollector = Box<dyn OutputCollector + Send + Sync>;

lazy_static! {
    pub static ref GLOBAL_OUTPUT_COLLECTOR: RwLock<Box<dyn OutputCollector + 'static + Sync + Send>> =
        RwLock::new(Box::new(Logger::stdout(None)));
    static ref ERROR: ColoredString = Level::Error.as_ref().to_ascii_uppercase().red();
    static ref WARN: ColoredString = Level::Warn.as_ref().to_ascii_uppercase().yellow();
    static ref INFO: ColoredString = Level::Info.as_ref().to_ascii_uppercase().green();
    static ref DEBUG: ColoredString = Level::Debug.as_ref().to_ascii_uppercase().bright_white();
    static ref TRACE: ColoredString = Level::Trace.as_ref().to_ascii_uppercase().cyan();
    static ref L_BRACE: ColoredString = "[".color(Color::TrueColor {
        r: 74,
        g: 87,
        b: 107
    });
    static ref R_BRACE: ColoredString = "]".color(Color::TrueColor {
        r: 74,
        g: 87,
        b: 107
    });
}

#[derive(Debug, Clone)]
pub struct Config {
    location: OutputLocation,
    level: Level,
    no_color: bool,
}
impl Config {
    pub fn file<P: AsRef<Path>>(path: P) -> Self {
        Self {
            location: OutputLocation::File(path.as_ref().to_path_buf()),
            level: Level::default(),
            no_color: true,
        }
    }

    pub fn stdout() -> Self {
        Self {
            location: OutputLocation::Stdout,
            level: Level::default(),
            no_color: false,
        }
    }

    pub fn no_color(mut self, no_color: bool) -> Self {
        self.no_color = no_color;
        self
    }

    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    pub fn as_collector(self) -> std::io::Result<BoxedCollector> {
        match self.location {
            OutputLocation::File(path) => {
                let mut logger = Logger::file(path, Some(self.level))?;
                logger.set_no_color(self.no_color);
                Ok(Box::new(logger))
            }
            OutputLocation::Stdout => {
                let mut logger = Logger::stdout(Some(self.level));
                logger.set_no_color(self.no_color);
                Ok(Box::new(logger))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum OutputLocation {
    File(PathBuf),
    Stdout,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    fn colored_string(&self) -> &'static ColoredString {
        match &self {
            Level::Error => &ERROR,
            Level::Debug => &DEBUG,
            Level::Info => &INFO,
            Level::Warn => &WARN,
            Level::Trace => &TRACE,
        }
    }
}

impl Default for Level {
    fn default() -> Self {
        Level::Info
    }
}

impl AsRef<str> for Level {
    fn as_ref(&self) -> &str {
        match &self {
            Level::Error => "error",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Trace => "trace",
        }
    }
}

pub struct Arguments<'args> {
    pub level: Option<Level>,
    pub args: fmt::Arguments<'args>,
}

impl<'args> Arguments<'args> {
    pub fn new(args: fmt::Arguments<'args>) -> Self {
        Self { level: None, args }
    }

    pub fn level(mut self, level: Level) -> Self {
        self.level = Some(level);
        self
    }
}

pub trait OutputCollector: Writer + Leveled + Scoped + Colored {}

pub trait Writer {
    fn write_out(&mut self, args: Arguments<'_>) -> io::Result<()>;
}

pub trait Leveled {
    fn set_level(&mut self, level: Level);
}

pub trait Scoped {
    fn append_scope(&mut self, scope: String);
    fn pop_scope(&mut self);
}

pub trait Colored {
    fn set_override(&mut self, should_color: bool);
}

pub struct Logger<'l> {
    level: Level,
    handle: Box<dyn std::io::Write + Send + Sync + 'l>,
    scopes: VecDeque<String>,
    timestamp: bool,
    no_color: bool,
}

impl<'l> Logger<'l> {
    pub fn new(
        handle: impl std::io::Write + Send + Sync + 'l,
        level: Option<Level>,
        no_color: bool,
    ) -> Self {
        Self {
            level: level.unwrap_or_default(),
            handle: Box::new(handle),
            scopes: VecDeque::new(),
            timestamp: true,
            no_color,
        }
    }

    pub fn stdout(level: Option<Level>) -> Self {
        Self::new(std::io::stdout(), level, false)
    }

    pub fn file(path: impl AsRef<Path>, level: Option<Level>) -> io::Result<Self> {
        Ok(Self::new(
            File::open(path.as_ref()).or_else(|_| File::create(path.as_ref()))?,
            level,
            true,
        ))
    }

    pub fn set_no_color(&mut self, no_color: bool) {
        self.no_color = no_color;
    }

    fn verify_should_colorize(&self) {
        let control = &colored::control::SHOULD_COLORIZE;
        if control.should_colorize() && self.no_color {
            control.set_override(false);
        } else if !control.should_colorize() && !self.no_color {
            control.set_override(true);
        }
    }
}

impl<'l> Writer for Logger<'l> {
    fn write_out(&mut self, args: Arguments<'_>) -> std::io::Result<()> {
        use chrono::prelude::*;

        self.verify_should_colorize();

        let level = if let Some(level) = args.level {
            if level > self.level {
                return Ok(());
            } else {
                level
            }
        } else {
            self.level
        };

        let mut s = format!("{}{: ^5}{}", *L_BRACE, level.colored_string(), *R_BRACE);

        if self.timestamp {
            s.push_str(&format!(
                "{}{}{}",
                *L_BRACE,
                Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                *R_BRACE
            ));
        }

        for scope in self.scopes.iter() {
            s.push_str(&format!("{}{}{}", *L_BRACE, scope, *R_BRACE));
        }
        s.push(' ');
        let args_str = format!("{}", args.args);
        s.push_str(&args_str);
        s.push('\n');

        write!(&mut self.handle, "{}", s)
    }
}
impl<'l> Leveled for Logger<'l> {
    fn set_level(&mut self, level: Level) {
        self.level = level;
    }
}

impl<'l> Scoped for Logger<'l> {
    fn append_scope(&mut self, scope: String) {
        self.scopes.push_back(scope);
    }

    fn pop_scope(&mut self) {
        self.scopes.pop_back();
    }
}

impl<'l> Colored for Logger<'l> {
    fn set_override(&mut self, should_color: bool) {
        self.no_color = !should_color;
    }
}

impl<'l> OutputCollector for Logger<'l> {}

#[macro_export]
macro_rules! write_out {
    (-> $dst:expr, $($arg:tt)*) =>
    {{
         use crate::log::{Arguments};
         $dst.write_out(Arguments::new(format_args!($($arg)*)))
     }};
    (error -> $dst:expr, $($arg:tt)*) =>
    {{
         use crate::log::{Arguments, Level};
         $dst.write_out(Arguments::new(format_args!($($arg)*)).level(Level::Error))
     }};
    (info -> $dst:expr, $($arg:tt)*) =>
    {{
         use crate::log::{Arguments, Level};
         $dst.write_out(Arguments::new(format_args!($($arg)*)).level(Level::Info))
     }};
    (debug -> $dst:expr, $($arg:tt)*) =>
    {{
         use crate::log::{Arguments, Level};
         $dst.write_out(Arguments::new(format_args!($($arg)*)).level(Level::Debug))
     }};
    (warn -> $dst:expr, $($arg:tt)*) =>
    {{
         use crate::log::{Arguments, Level};
         $dst.write_out(Arguments::new(format_args!($($arg)*)).level(Level::Warn))
     }};
    (trace -> $dst:expr, $($arg:tt)*) =>
    {{
         use crate::log::{Arguments, Level};
         $dst.write_out(Arguments::new(format_args!($($arg)*)).level(Level::Trace))
     }};
    ($($arg:tt)*) =>
    {{
         use crate::log::GLOBAL_OUTPUT_COLLECTOR;
         write_out!(-> GLOBAL_OUTPUT_COLLECTOR, $($arg)*)
     }};
    (error $($arg:tt)*) =>
    {{
         use crate::log::GLOBAL_OUTPUT_COLLECTOR;
         write_out!(error -> GLOBAL_OUTPUT_COLLECTOR, $($arg)*)
     }};
    (info $($arg:tt)*) =>
    {{
         use crate::log::GLOBAL_OUTPUT_COLLECTOR;
         write_out!(info -> GLOBAL_OUTPUT_COLLECTOR, $($arg)*)
     }};
    (debug $($arg:tt)*) =>
    {{
         use crate::log::GLOBAL_OUTPUT_COLLECTOR;
         write_out!(debug -> GLOBAL_OUTPUT_COLLECTOR, $($arg)*)
     }};
    (warn $($arg:tt)*) =>
    {{
         use crate::log::GLOBAL_OUTPUT_COLLECTOR;
         write_out!(warn -> GLOBAL_OUTPUT_COLLECTOR, $($arg)*)
     }};
    (trace $($arg:tt)*) =>
    {{
         use crate::log::GLOBAL_OUTPUT_COLLECTOR;
         if let Ok(mut collector) = GLOBAL_OUTPUT_COLLECTOR.try_write() {
             write_out!(trace -> collector, $($arg)*)
         }
     }};
}

#[macro_export]
macro_rules! error {
    ($dst:expr => $($arg:tt)*) => {{
        use crate::log::write_out;
        if let Err(e) = write_out!(error -> $dst, $($arg)*) {
            eprintln!("logging failed - {}", e);
        }
    }};
    ($($arg:tt)*) => {{
        use crate::log::{error, GLOBAL_OUTPUT_COLLECTOR};
        if let Ok(mut collector) = GLOBAL_OUTPUT_COLLECTOR.try_write() {
            error!(collector => $($arg)*);
        }
    }};
}
#[macro_export]
macro_rules! info {
    ($dst:expr => $($arg:tt)*) => {{
        use crate::log::write_out;
        if let Err(e) = write_out!(info -> $dst, $($arg)*) {
            eprintln!("logging failed - {}", e);
        }
    }};
    ($($arg:tt)*) => {{
        use crate::log::{info, GLOBAL_OUTPUT_COLLECTOR};
        if let Ok(mut collector) = GLOBAL_OUTPUT_COLLECTOR.try_write() {
            info!(collector => $($arg)*);
        }
    }};
}
#[macro_export]
macro_rules! debug {
    ($dst:expr => $($arg:tt)*) => {{
        use crate::log::write_out;
        if let Err(e) = write_out!(debug -> $dst, $($arg)*) {
            eprintln!("logging failed - {}", e);
        }
    }};
    ($($arg:tt)*) => {{
        use crate::log::{debug, GLOBAL_OUTPUT_COLLECTOR};
        if let Ok(mut collector) = GLOBAL_OUTPUT_COLLECTOR.try_write() {
            debug!(collector => $($arg)*);
        }
    }};
}
#[macro_export]
macro_rules! warning {
    ($dst:expr => $($arg:tt)*) => {{
        use crate::log::write_out;
        if let Err(e) = write_out!(warn -> $dst, $($arg)*) {
            eprintln!("logging failed - {}", e);
        }
    }};
    ($($arg:tt)*) => {{
        use crate::log::{warning, GLOBAL_OUTPUT_COLLECTOR};
        if let Ok(mut collector) = GLOBAL_OUTPUT_COLLECTOR.try_write() {
            warning!(collector => $($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! trace {
    ($dst:expr => $($arg:tt)*) => {{
        use crate::log::write_out;
        if let Err(e) = write_out!(trace -> $dst, $($arg)*) {
            eprintln!("logging failed - {}", e);
        }
    }};
    ($($arg:tt)*) => {{
        use crate::log::{trace, GLOBAL_OUTPUT_COLLECTOR};
        if let Ok(mut collector) = GLOBAL_OUTPUT_COLLECTOR.try_write() {
            trace!(collector => $($arg)*);
        }
    }};
}

pub use {debug, error, info, trace, warning, write_out};
