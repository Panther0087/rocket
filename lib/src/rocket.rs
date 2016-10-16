use std::collections::HashMap;
use std::str::from_utf8_unchecked;
use std::cmp::min;
use std::process;

use term_painter::Color::*;
use term_painter::ToStyle;

use config;
use logger;
use request::{Request, Data, FormItems};
use router::{Router, Route};
use catcher::{self, Catcher};
use outcome::Outcome;
use error::Error;

use http::{Method, StatusCode};
use http::hyper::{HyperRequest, FreshHyperResponse};
use http::hyper::{HyperServer, HyperHandler, HyperSetCookie, header};

pub struct Rocket {
    address: String,
    port: usize,
    router: Router,
    default_catchers: HashMap<u16, Catcher>,
    catchers: HashMap<u16, Catcher>,
}

#[doc(hidden)]
impl HyperHandler for Rocket {
    fn handle<'h, 'k>(&self,
                      req: HyperRequest<'h, 'k>,
                      mut res: FreshHyperResponse<'h>) {
        res.headers_mut().set(header::Server("rocket".to_string()));
        self.dispatch(req, res)
    }
}

impl Rocket {
    fn dispatch<'h, 'k>(&self,
                        hyp_req: HyperRequest<'h, 'k>,
                        mut res: FreshHyperResponse<'h>) {
        // Get a copy of the URI for later use.
        let uri = hyp_req.uri.to_string();

        // Get all of the information from Hyper.
        let (_, h_method, h_headers, h_uri, _, h_body) = hyp_req.deconstruct();

        // Try to create a Rocket request from the hyper request info.
        let mut request = match Request::new(h_method, h_headers, h_uri) {
            Ok(req) => req,
            Err(ref reason) => {
                let mock_request = Request::mock(Method::Get, uri.as_str());
                debug_!("Bad request: {}", reason);
                return self.handle_error(StatusCode::InternalServerError,
                                         &mock_request, res);
            }
        };

        // Retrieve the data from the request.
        let mut data = match Data::from_hyp(h_body) {
            Ok(data) => data,
            Err(reason) => {
                debug_!("Bad data in request: {}", reason);
                return self.handle_error(StatusCode::InternalServerError,
                                         &request, res);
            }
        };

        // Preprocess the request.
        self.preprocess_request(&mut request, &data);

        info!("{}:", request);
        trace_!("Peek size: {} bytes", data.peek().len());
        let matches = self.router.route(&request);
        for route in matches {
            // Retrieve and set the requests parameters.
            info_!("Matched: {}", route);
            request.set_params(route);

            // Dispatch the request to the handler.
            let response = (route.handler)(&request, data);

            // Check if the request processing completed or if the request needs
            // to be forwarded. If it does, continue the loop to try again.
            info_!("{} {}", White.paint("Response:"), response);
            let mut responder = match response {
                Outcome::Success(responder) => responder,
                Outcome::Failure(status_code) => {
                    return self.handle_error(status_code, &request, res);
                }
                Outcome::Forward(unused_data) => {
                    data = unused_data;
                    continue;
                }
            };

            // We have a responder. Update the cookies in the header.
            let cookie_delta = request.cookies().delta();
            if cookie_delta.len() > 0 {
                res.headers_mut().set(HyperSetCookie(cookie_delta));
            }

            // Actually process the response.
            let outcome = responder.respond(res);
            info_!("{} {}", White.paint("Outcome:"), outcome);

            // Check if the responder wants to forward to a catcher.
            match outcome.forwarded() {
                Some((c, r)) => return self.handle_error(c, &request, r),
                None => return
            };
        }

        error_!("No matching routes.");
        self.handle_error(StatusCode::NotFound, &request, res);
    }

    /// Preprocess the request for Rocket-specific things. At this time, we're
    /// only checking for _method in forms.
    fn preprocess_request(&self, req: &mut Request, data: &Data) {
        // Check if this is a form and if the form contains the special _method
        // field which we use to reinterpret the request's method.
        let data_len = data.peek().len();
        let (min_len, max_len) = ("_method=get".len(), "_method=delete".len());
        if req.content_type().is_form() && data_len >= min_len {
            let form = unsafe {
                from_utf8_unchecked(&data.peek()[..min(data_len, max_len)])
            };

            let mut form_items = FormItems(form);
            if let Some(("_method", value)) = form_items.next() {
                if let Ok(method) = value.parse() {
                    req.method = method;
                }
            }
        }
    }

    // Call when no route was found.
    fn handle_error<'r>(&self,
                        code: StatusCode,
                        req: &'r Request,
                        response: FreshHyperResponse) {

        // Find the catcher or use the one for internal server errors.
        let catcher = self.catchers.get(&code.to_u16()).unwrap_or_else(|| {
            error_!("No catcher found for {}.", code);
            warn_!("Using internal server error catcher.");
            self.catchers.get(&500).expect("500 Catcher")
        });

        if let Some(mut responder) = catcher.handle(Error::NoRoute, req).responder() {
            if !responder.respond(response).is_success() {
                error_!("Catcher outcome was unsuccessul; aborting response.");
            } else {
                info_!("Responded with {} catcher.", White.paint(code));
            }
        } else {
            error_!("Catcher returned an incomplete response.");
            warn_!("Using default error response.");
            let catcher = self.default_catchers.get(&code.to_u16())
                .unwrap_or(self.default_catchers.get(&500).expect("500 default"));
            let responder = catcher.handle(Error::Internal, req).responder();
            responder.unwrap().respond(response).unwrap()
        }
    }

    pub fn mount(mut self, base: &str, routes: Vec<Route>) -> Self {
        info!("🛰  {} '{}':", Magenta.paint("Mounting"), base);
        for mut route in routes {
            let path = format!("{}/{}", base, route.path.as_str());
            route.set_path(path);

            info_!("{}", route);
            self.router.add(route);
        }

        self
    }

    pub fn catch(mut self, catchers: Vec<Catcher>) -> Self {
        info!("👾  {}:", Magenta.paint("Catchers"));
        for c in catchers {
            if self.catchers.get(&c.code).map_or(false, |e| !e.is_default()) {
                let msg = "(warning: duplicate catcher!)";
                info_!("{} {}", c, Yellow.paint(msg));
            } else {
                info_!("{}", c);
            }

            self.catchers.insert(c.code, c);
        }

        self
    }

    pub fn launch(self) {
        if self.router.has_collisions() {
            warn!("Route collisions detected!");
        }

        let full_addr = format!("{}:{}", self.address, self.port);
        let server = match HyperServer::http(full_addr.as_str()) {
            Ok(hyper_server) => hyper_server,
            Err(e) => {
                error!("failed to start server.");
                error_!("{}", e);
                process::exit(1);
            }
        };

        info!("🚀  {} {}...",
              White.paint("Rocket has launched from"),
              White.bold().paint(&full_addr));

        server.handle(self).unwrap();
    }

    /// Retrieves the configuration parameter named `name` for the current
    /// environment. Returns Some(value) if the paremeter exists. Otherwise,
    /// returns None.
    pub fn config<S: AsRef<str>>(_name: S) -> Option<&'static str> {
        // TODO: Implement me.
        None
    }

    pub fn ignite() -> Rocket {
        // Note: init() will exit the process under config errors.
        let config = config::init();

        logger::init(config.log_level);
        info!("🔧  Configured for {}.", config.env);
        info_!("listening: {}:{}",
               White.paint(&config.address),
               White.paint(&config.port));
        info_!("logging: {:?}", White.paint(config.log_level));
        info_!("session key: {}", White.paint(config.take_session_key().is_some()));
        for (name, value) in config.extras() {
            info_!("{} {}: {}", Yellow.paint("[extra]"), name, White.paint(value));
        }

        Rocket {
            address: config.address.clone(),
            port: config.port,
            router: Router::new(),
            default_catchers: catcher::defaults::get(),
            catchers: catcher::defaults::get(),
        }
    }
}
