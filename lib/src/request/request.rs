use std::io::{Read};
use std::cell::RefCell;

use error::Error;
use param::FromParam;
use method::Method;

use content_type::ContentType;
use hyper::uri::RequestUri as HyperRequestUri;
use hyper::header;
use router::URIBuf;
use router::URI;
use router::Route;

// Hyper stuff.
use request::{HyperHeaders, HyperRequest};

pub struct Request<'a> {
    pub params: RefCell<Option<Vec<&'a str>>>, // This also sucks.
    pub method: Method,
    pub uri: URIBuf, // FIXME: Should be URI (without Hyper).
    pub data: Vec<u8>, // FIXME: Don't read this! (bad Hyper.)
    headers: HyperHeaders, // This sucks.
}

impl<'a> Request<'a> {
    pub fn get_param<T: FromParam<'a>>(&'a self, n: usize) -> Result<T, Error> {
        let params = self.params.borrow();
        if params.is_none() || n >= params.as_ref().unwrap().len() {
            Err(Error::NoKey)
        } else {
            T::from_param(params.as_ref().unwrap()[n])
        }
    }

    #[cfg(test)]
    pub fn mock(method: Method, uri: &str) -> Request {
        Request {
            params: RefCell::new(None),
            method: method,
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

    pub fn content_type(&self) -> ContentType {
        let hyp_ct = self.headers().get::<header::ContentType>();
        hyp_ct.map_or(ContentType::any(), |ct| ContentType::from(&ct.0))
    }

    pub fn uri(&'a self) -> URI<'a> {
        self.uri.as_uri()
    }

    pub fn set_params(&'a self, route: &Route) {
        *self.params.borrow_mut() = Some(route.get_params(self.uri.as_uri()))
    }

    #[cfg(test)]
    pub fn set_content_type(&mut self, ct: ContentType) {
        let hyper_ct = header::ContentType(ct.into());
        self.headers.set::<header::ContentType>(hyper_ct)
    }

}

impl<'a, 'h, 'k> From<HyperRequest<'h, 'k>> for Request<'a> {
    fn from(hyper_req: HyperRequest<'h, 'k>) -> Request<'a> {
        let (_, h_method, h_headers, h_uri, _, mut h_body) = hyper_req.deconstruct();

        let uri = match h_uri {
            HyperRequestUri::AbsolutePath(s) => URIBuf::from(s),
            _ => panic!("Can only accept absolute paths!")
        };

        // FIXME: GRRR.
        let mut data = vec![];
        h_body.read_to_end(&mut data).unwrap();

        Request {
            params: RefCell::new(None),
            method: Method::from_hyp(&h_method).unwrap(),
            uri: uri,
            data: data,
            headers: h_headers,
        }
    }
}
