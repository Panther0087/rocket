mod empty;
mod responder;
mod redirect;
mod with_status;
mod outcome;
mod flash;
mod data_type;

pub use hyper::server::Response as HyperResponse;
pub use hyper::net::Fresh as HyperFresh;
pub use hyper::status::StatusCode;
pub use hyper::header;
pub use hyper::mime;

pub use self::data_type::*;
pub use self::responder::Responder;
pub use self::empty::{Empty, Forward};
pub use self::redirect::Redirect;
pub use self::with_status::StatusResponse;
pub use self::outcome::Outcome;
pub use self::flash::Flash;

use std::ops::{Deref, DerefMut};

pub type FreshHyperResponse<'a> = HyperResponse<'a, HyperFresh>;

pub struct Response<'a>(Box<Responder + 'a>);

impl<'a> Response<'a> {
    pub fn new<T: Responder + 'a>(body: T) -> Response<'a> {
        Response(Box::new(body))
    }

    pub fn with_status<T: Responder + 'a>(status: StatusCode, body: T)
            -> Response<'a> {
        Response(Box::new(StatusResponse::new(status, body)))
    }

    pub fn forward() -> Response<'a> {
        Response(Box::new(Forward))
    }

    pub fn with_raw_status<T: Responder + 'a>(status: u16, body: T)
            -> Response<'a> {
        let status_code = StatusCode::from_u16(status);
        Response(Box::new(StatusResponse::new(status_code, body)))
    }

    pub fn empty() -> Response<'a> {
        Response(Box::new(Empty::new(StatusCode::Ok)))
    }

    pub fn not_found() -> Response<'a> {
        Response(Box::new(Empty::new(StatusCode::NotFound)))
    }

    pub fn server_error() -> Response<'a> {
        Response(Box::new(Empty::new(StatusCode::InternalServerError)))
    }
}

impl<'a> Deref for Response<'a> {
    type Target = Box<Responder + 'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> DerefMut for Response<'a> {
    fn deref_mut(&mut self) -> &mut Box<Responder + 'a> {
        &mut self.0
    }
}
