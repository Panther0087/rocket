#![feature(question_mark)]
#![feature(specialization)]
#![feature(conservative_impl_trait)]

//! # Rocket - Core API Documentation
//!
//! Hello, and welcome to the core Rocket API documentation!
//!
//! This API documentation is highly technical and is purely a reference.
//! There's an [overview](https://rocket.rs/overview) of Rocket on the main site
//! as well as a [full, detailed guide](https://rocket.rs/guide). If you'd like
//! pointers on getting started, see the
//! [quickstart](https://rocket.rs/guide/quickstart) or [getting
//! started](https://rocket.rs/guide/getting_started) chapters of the guide.
//!
//! You may also be interested in looking at the [contrib API
//! documentation](../rocket_contrib), which contains JSON and templating
//! support.
//!
//! ## Libraries
//!
//! Rocket's functionality is split into three crates:
//!
//!   1. [Core](/rocket) - The core library. Needed by every Rocket application.
//!   2. [Codegen](/rocket_codegen) - Core code generation plugin. Should always
//!      be used alongsize `rocket`, though it's not necessary.
//!   3. [Contrib](/rocket_contrib) - Provides useful functionality for many
//!      Rocket application. Completely optional.
//!
//! ## Usage
//!
//! The sanctioned way to use Rocket is via the code generation plugin. This
//! makes Rocket easier to use and allows a somewhat stable API as Rust matures.
//! To use Rocket in your Cargo-based project, add the following to
//! `Cargo.toml`:
//!
//! ```rust,ignore
//! [dependencies]
//! rocket = "*"
//! rocket_codegen = "*"
//! ```
//!
//! If you'll be deploying your project to Crates.io, you'll need to change the
//! "*" to the current version of Rocket.
//!
//! Then, add the following to top of your `main.rs` file:
//!
//! ```rust,ignore
//! #![feature(plugin)]
//! #![plugin(rocket_codegen)]
//!
//! extern crate rocket;
//! ```
//!
//! See the [guide](https://guide.rocket.rs) for more information on how to
//! write Rocket application.
//!
//! ## Configuration
//!
//! Rocket is configured via the `Rocket.toml` file.
//!

#[macro_use] extern crate log;
extern crate term_painter;
extern crate hyper;
extern crate url;
// extern crate mime;
extern crate toml;

#[cfg(test)] #[macro_use] extern crate lazy_static;

#[doc(hidden)] #[macro_use] pub mod logger;
#[doc(hidden)] pub mod http;
pub mod request;
pub mod response;
pub mod outcome;

mod error;
mod router;
mod rocket;
mod codegen;
mod catcher;
mod config;

/// Defines the types for request and error handlers.
pub mod handler {
    use request::{Request, Data};
    use response::Response;
    use error::Error;

    /// The type of a request handler.
    pub type Handler = for<'r> fn(&'r Request, Data) -> Response<'r>;

    /// The type of an error handler.
    pub type ErrorHandler = for<'r> fn(Error, &'r Request) -> Response<'r>;
}

#[doc(inline)] pub use response::{Response, Responder};
#[doc(inline)] pub use handler::{Handler, ErrorHandler};
#[doc(inline)] pub use logger::LoggingLevel;
pub use codegen::{StaticRouteInfo, StaticCatchInfo};
pub use router::Route;
pub use request::{Request, Data};
pub use error::Error;
pub use catcher::Catcher;
pub use rocket::Rocket;

/// Alias to Rocket::ignite().
pub fn ignite() -> Rocket {
    Rocket::ignite()
}
