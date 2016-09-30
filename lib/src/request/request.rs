use std::io::{Read};
use std::cell::RefCell;
use std::fmt;

use term_painter::Color::*;
use term_painter::ToStyle;

use error::Error;
use super::{FromParam, FromSegments};
use method::Method;

use content_type::ContentType;
use hyper::uri::RequestUri as HyperRequestUri;
use hyper::header;
use router::URIBuf;
use router::URI;
use router::Route;

// Hyper stuff.
use request::{Cookies, HyperCookie, HyperHeaders, HyperRequest};

pub struct Request<'a> {
    pub method: Method,
    pub uri: URIBuf, // FIXME: Should be URI (without Hyper).
    pub data: Vec<u8>, // FIXME: Don't read this! (bad Hyper.)
    cookies: Cookies,
    headers: HyperHeaders, // This sucks.
    params: RefCell<Option<Vec<&'a str>>>, // This also sucks.
}

impl<'a> Request<'a> {
    // FIXME: Don't do the from_param parsing here. I think. Not sure. Decide.
    pub fn get_param<T: FromParam<'a>>(&self, n: usize) -> Result<T, Error> {
        let params = self.params.borrow();
        if params.is_none() || n >= params.as_ref().unwrap().len() {
            debug!("{} is >= param count {}", n, params.as_ref().unwrap().len());
            Err(Error::NoKey)
        } else {
            T::from_param(params.as_ref().unwrap()[n]).map_err(|_| Error::BadParse)
        }
    }

    pub fn cookies<'r>(&'r self) -> &'r Cookies {
        &self.cookies
    }

    /// i is the index of the first segment to consider
    pub fn get_segments<'r: 'a, T: FromSegments<'a>>(&'r self, i: usize)
            -> Result<T, Error> {
        if i >= self.uri().segment_count() {
            debug!("{} is >= segment count {}", i, self.uri().segment_count());
            Err(Error::NoKey)
        } else {
            // TODO: Really want to do self.uri.segments().skip(i).into_inner(),
            // but the std lib doesn't implement it for Skip.
            let mut segments = self.uri.segments();
            for _ in segments.by_ref().take(i) { /* do nothing */ }
            T::from_segments(segments).map_err(|_| Error::BadParse)
        }
    }

    pub fn mock(method: Method, uri: &str) -> Request {
        Request {
            params: RefCell::new(None),
            method: method,
            cookies: Cookies::new(&[]),
            uri: URIBuf::from(uri),
            data: vec![],
            headers: HyperHeaders::new()
        }
    }

    // FIXME: Get rid of Hyper.
    #[inline(always)]
    pub fn headers(&self) -> &HyperHeaders {
        &self.headers
    }

    // FIXME: This should be an Option. Not all requests have content types.
    pub fn content_type(&self) -> ContentType {
        let hyp_ct = self.headers().get::<header::ContentType>();
        hyp_ct.map_or(ContentType::any(), |ct| ContentType::from(&ct.0))
    }

    /// Returns the first content-type accepted by this request.
    pub fn accepts(&self) -> ContentType {
        let accept = self.headers().get::<header::Accept>();
        accept.map_or(ContentType::any(), |accept| {
            let items = &accept.0;
            if items.len() < 1 {
                return ContentType::any();
            } else {
                return ContentType::from(items[0].item.clone())
            }
        })
    }

    pub fn uri(&'a self) -> URI<'a> {
        self.uri.as_uri()
    }

    // FIXME: Don't need a refcell for this.
    pub fn set_params(&'a self, route: &Route) {
        *self.params.borrow_mut() = Some(route.get_params(self.uri.as_uri()))
    }

    #[cfg(test)]
    pub fn set_content_type(&mut self, ct: ContentType) {
        let hyper_ct = header::ContentType(ct.into());
        self.headers.set::<header::ContentType>(hyper_ct)
    }

    pub fn from_hyp<'h, 'k>(hyper_req: HyperRequest<'h, 'k>)
            -> Result<Request<'a>, String> {
        let (_, h_method, h_headers, h_uri, _, mut h_body) = hyper_req.deconstruct();

        let uri = match h_uri {
            HyperRequestUri::AbsolutePath(s) => URIBuf::from(s),
            _ => return Err(format!("Bad URI: {}", h_uri))
        };

        let method = match Method::from_hyp(&h_method) {
            Some(m) => m,
            _ => return Err(format!("Bad method: {}", h_method))
        };

        let cookies = match h_headers.get::<HyperCookie>() {
           // TODO: What to do about key?
           Some(cookie) => cookie.to_cookie_jar(&[]),
           None => Cookies::new(&[])
        };

        // FIXME: GRRR.
        let mut data = vec![];
        h_body.read_to_end(&mut data).unwrap();

        let request = Request {
            params: RefCell::new(None),
            method: method,
            cookies: cookies,
            uri: uri,
            data: data,
            headers: h_headers,
        };

        Ok(request)
    }
}

impl<'r> fmt::Display for Request<'r> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", Green.paint(&self.method), Blue.paint(&self.uri))
    }
}
