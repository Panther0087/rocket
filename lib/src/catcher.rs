use handler::ErrorHandler;
use response::Response;
use codegen::StaticCatchInfo;
use error::Error;
use request::Request;

use std::fmt;
use term_painter::ToStyle;
use term_painter::Color::*;

pub struct Catcher {
    pub code: u16,
    handler: ErrorHandler,
    is_default: bool
}

// TODO: Should `Catcher` be an interface? Should there be an `ErrorHandler`
// that takes in a `RoutingError` and returns a `Response`? What's the right
// interface here?

impl Catcher {
    pub fn new(code: u16, handler: ErrorHandler) -> Catcher {
        Catcher::new_with_default(code, handler, false)
    }

    pub fn handle<'r>(&self, error: Error, request: &'r Request<'r>) -> Response<'r> {
        (self.handler)(error, request)
    }

    fn new_with_default(code: u16, handler: ErrorHandler, default: bool) -> Catcher {
        Catcher {
            code: code,
            handler: handler,
            is_default: default
        }
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }
}

impl<'a> From<&'a StaticCatchInfo> for Catcher {
    fn from(info: &'a StaticCatchInfo) -> Catcher {
        Catcher::new(info.code, info.handler)
    }
}

impl fmt::Display for Catcher {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", Blue.paint(&self.code))
    }
}

pub mod defaults {
    use request::Request;
    use response::{StatusCode, Response};
    use super::Catcher;
    use error::Error;
	use std::collections::HashMap;

    pub fn not_found<'r>(_error: Error, _request: &'r Request<'r>) -> Response<'r> {
        Response::with_status(StatusCode::NotFound, "\
			<head>\
  				<meta charset=\"utf-8\">\
				<title>404: Not Found</title>\
	  		</head>\
			<body align=\"center\">\
            <h1>404: Not Found</h1>\
            <p>The page you were looking for could not be found.<p>\
            <hr />\
            <small>Rocket</small>\
			</body>\
        ")
    }

    pub fn internal_error<'r>(_error: Error, _request: &'r Request<'r>)
        -> Response<'r> {
        Response::with_status(StatusCode::InternalServerError, "\
			<head>\
  				<meta charset=\"utf-8\">\
				<title>404: Not Found</title>\
	  		</head>\
			<body align=\"center\">\
            <h1>500: Internal Server Error</h1>\
            <p>The server encountered a problem processing your request.<p>\
            <hr />\
            <small>Rocket</small>\
        ")
    }

    pub fn get() -> HashMap<u16, Catcher> {
		let mut map = HashMap::new();
		map.insert(404, Catcher::new_with_default(404, not_found, true));
		map.insert(500, Catcher::new_with_default(500, internal_error, true));
		map
    }
}
