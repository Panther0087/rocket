//! Rocket's logging infrastructure.

use std::str::FromStr;

use log::{self, Log, LogLevel, LogRecord, LogMetadata};
use term_painter::Color::*;
use term_painter::ToStyle;

struct RocketLogger(LoggingLevel);

/// Defines the different levels for log messages.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum LoggingLevel {
    /// Only shows errors and warning.
    Critical,
    /// Shows everything except debug and trace information.
    Normal,
    /// Shows everything.
    Debug,
}

impl LoggingLevel {
    #[inline(always)]
    fn max_log_level(&self) -> LogLevel {
        match *self {
            LoggingLevel::Critical => LogLevel::Warn,
            LoggingLevel::Normal => LogLevel::Info,
            LoggingLevel::Debug => LogLevel::Trace,
        }
    }
}

impl FromStr for LoggingLevel {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let level = match s {
            "critical" => LoggingLevel::Critical,
            "normal" => LoggingLevel::Normal,
            "debug" => LoggingLevel::Debug,
            _ => return Err(())
        };

        Ok(level)
    }
}

#[doc(hidden)] #[macro_export]
macro_rules! log_ {
    ($name:ident: $format:expr) => { log_!($name: $format,) };
    ($name:ident: $format:expr, $($args:expr),*) => {
        $name!(target: "_", $format, $($args),*);
    };
}

#[doc(hidden)] #[macro_export]
macro_rules! error_ { ($($args:expr),+) => { log_!(error: $($args),+); }; }
#[doc(hidden)] #[macro_export]
macro_rules! info_ { ($($args:expr),+) => { log_!(info: $($args),+); }; }
#[doc(hidden)] #[macro_export]
macro_rules! trace_ { ($($args:expr),+) => { log_!(trace: $($args),+); }; }
#[doc(hidden)] #[macro_export]
macro_rules! debug_ { ($($args:expr),+) => { log_!(debug: $($args),+); }; }
#[doc(hidden)] #[macro_export]
macro_rules! warn_ { ($($args:expr),+) => { log_!(warn: $($args),+); }; }

impl Log for RocketLogger {
    fn enabled(&self, md: &LogMetadata) -> bool {
        md.level() <= self.0.max_log_level()
    }

    fn log(&self, record: &LogRecord) {
        // Print nothing if this level isn't enabled.
        if !self.enabled(record.metadata()) {
            return;
        }

        // Don't print Hyper's messages unless Debug is enabled.
        let from_hyper = record.location().module_path().starts_with("hyper::");
        if from_hyper && self.0 != LoggingLevel::Debug {
            return;
        }

        // In Rocket, we abuse target with value "_" to indicate indentation.
        if record.target() == "_" && self.0 != LoggingLevel::Critical {
            print!("    {} ", White.paint("=>"));
        }

        use log::LogLevel::*;
        match record.level() {
            Info => println!("{}", Blue.paint(record.args())),
            Trace => println!("{}", Magenta.paint(record.args())),
            Error => {
                println!("{} {}",
                         Red.bold().paint("Error:"),
                         Red.paint(record.args()))
            }
            Warn => {
                println!("{} {}",
                         Yellow.bold().paint("Warning:"),
                         Yellow.paint(record.args()))
            }
            Debug => {
                let loc = record.location();
                print!("\n{} ", Blue.bold().paint("-->"));
                println!("{}:{}", Blue.paint(loc.file()), Blue.paint(loc.line()));
                println!("{}", record.args());
            }
        }
    }
}

#[doc(hidden)]
pub fn init(level: LoggingLevel) {
    let result = log::set_logger(|max_log_level| {
        max_log_level.set(level.max_log_level().to_log_level_filter());
        Box::new(RocketLogger(level))
    });

    if let Err(err) = result {
        println!("Logger failed to initialize: {}", err);
    }
}
