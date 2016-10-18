use super::Environment;

use term_painter::Color::White;
use term_painter::ToStyle;

/// The type of a configuration parsing error.
#[derive(Debug, PartialEq, Clone)]
pub struct ParsingError {
    pub byte_range: (usize, usize),
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub desc: String,
}

/// The type of a configuration error.
#[derive(Debug, PartialEq, Clone)]
pub enum ConfigError {
    /// The current working directory could not be determined.
    BadCWD,
    /// The configuration file was not found.
    NotFound,
    /// There was an I/O error while reading the configuration file.
    IOError,
    /// The path at which the configuration file was found was invalid.
    ///
    /// Parameters: (path, reason)
    BadFilePath(String, &'static str),
    /// An environment specified in `ROCKET_ENV` is invalid.
    ///
    /// Parameters: (environment_name)
    BadEnv(String),
    /// An environment specified as a table `[environment]` is invalid.
    ///
    /// Parameters: (environment_name, filename)
    BadEntry(String, String),
    /// A key was specified with a value of the wrong type.
    ///
    /// Parameters: (entry_name, expected_type, actual_type, filename)
    BadType(String, &'static str, &'static str, String),
    /// There was a TOML parsing error.
    ///
    /// Parameters: (toml_source_string, filename, error_list)
    ParseError(String, String, Vec<ParsingError>),
}

impl ConfigError {
    /// Prints this configuration error with Rocket formatting.
    pub fn pretty_print(&self) {
        use self::ConfigError::*;

        let valid_envs = Environment::valid();
        match *self {
            BadCWD => error!("couldn't get current working directory"),
            NotFound => error!("config file was not found"),
            IOError => error!("failed reading the config file: IO error"),
            BadFilePath(ref path, ref reason) => {
                error!("configuration file path '{}' is invalid", path);
                info_!("{}", reason);
            }
            BadEntry(ref name, ref filename) => {
                error!("[{}] is not a known configuration environment", name);
                info_!("in {}", White.paint(filename));
                info_!("valid environments are: {}", White.paint(valid_envs));
            }
            BadEnv(ref name) => {
                error!("'{}' is not a valid ROCKET_ENV value", name);
                info_!("valid environments are: {}", White.paint(valid_envs));
            }
            BadType(ref name, ref expected, ref actual, ref filename) => {
                error!("'{}' key could not be parsed", name);
                info_!("in {}", White.paint(filename));
                info_!("expected value to be {}, but found {}",
                       White.paint(expected), White.paint(actual));
            }
            ParseError(ref source, ref filename, ref errors) => {
                for error in errors {
                    let (lo, hi) = error.byte_range;
                    let (line, col) = error.start;
                    let error_source = &source[lo..hi];

                    error!("config file could not be parsed as TOML");
                    info_!("at {}:{}:{}", White.paint(filename), line + 1, col + 1);
                    trace_!("'{}' - {}", error_source, White.paint(&error.desc));
                }
            }
        }
    }

    /// Whether this error is of `NotFound` variant.
    #[inline(always)]
    pub fn is_not_found(&self) -> bool {
        use self::ConfigError::*;
        match *self {
            NotFound => true,
            _ => false
        }
    }
}
