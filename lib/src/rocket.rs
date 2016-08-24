use super::*;
use response::FreshHyperResponse;
use request::HyperRequest;
use catcher;

use std::io::Read;
use std::collections::HashMap;

use term_painter::Color::*;
use term_painter::ToStyle;

use hyper::uri::RequestUri as HyperRequestUri;
use hyper::method::Method as HyperMethod;
use hyper::server::Server as HyperServer;
use hyper::server::Handler as HyperHandler;

pub struct Rocket {
    address: &'static str,
    port: isize,
    router: Router,
    catchers: HashMap<u16, Catcher>,
}

fn uri_is_absolute(uri: &HyperRequestUri) -> bool {
    match *uri {
        HyperRequestUri::AbsolutePath(_) => true,
        _ => false
    }
}

fn unwrap_absolute_path(uri: &HyperRequestUri) -> &str {
    match *uri {
        HyperRequestUri::AbsolutePath(ref s) => s.as_str(),
        _ => panic!("Can only accept absolute paths!")
    }
}

fn method_is_valid(method: &HyperMethod) -> bool {
    Method::from_hyp(method).is_some()
}

impl HyperHandler for Rocket {
    fn handle<'a, 'k>(&'a self, req: HyperRequest<'a, 'k>,
            mut res: FreshHyperResponse<'a>) {
        info!("{:?} '{}':", Green.paint(&req.method), Blue.paint(&req.uri));

        let finalize = |mut req: HyperRequest, _res: FreshHyperResponse| {
            let mut buf = vec![];
            // FIXME: Simple DOS attack here. Working around Hyper bug.
            let _ = req.read_to_end(&mut buf);
        };

        if !uri_is_absolute(&req.uri) {
            error_!("Internal failure. Bad URI.");
            debug_!("Debug: {}", req.uri);
            return finalize(req, res);
        }

        if !method_is_valid(&req.method) {
            error_!("Internal failure. Bad method.");
            debug_!("Method: {}", req.method);
            return finalize(req, res);
        }

        res.headers_mut().set(response::header::Server("rocket".to_string()));
        self.dispatch(req, res)
    }
}

impl Rocket {
    fn dispatch<'h, 'k>(&self, mut req: HyperRequest<'h, 'k>,
                        res: FreshHyperResponse<'h>) {
        // We read all of the contents now because we have to do it at some
        // point thanks to Hyper. FIXME: Simple DOS attack here.
        let mut buf = vec![];
        let _ = req.read_to_end(&mut buf);

        // Extract the method, uri, and try to find a route.
        let method = Method::from_hyp(&req.method).unwrap();
        let uri = unwrap_absolute_path(&req.uri);
        let route = self.router.route(method, uri);

        // A closure which we call when we know there is no route.
        let handle_not_found = |response: FreshHyperResponse| {
            error_!("Dispatch failed. Returning 404.");

            let request = Request::new(&req.headers, method, uri, None, &buf);
            let catcher = self.catchers.get(&404).unwrap();
            catcher.handle(RoutingError::unchained(request)).respond(response);
        };

        // No route found. Handle the not_found error and return.
        if route.is_none() {
            error_!("No matching routes.");
            return handle_not_found(res);
        }

        // Okay, we've got a route. Unwrap it, generate a request, and dispatch.
        let route = route.unwrap();
        let params = route.get_params(uri);
        let request = Request::new(&req.headers, method, uri, Some(params), &buf);

        // TODO: Paint these magenta.
        trace_!("Dispatching request.");
        let outcome = (route.handler)(request).respond(res);

        // TODO: keep trying lower ranked routes before dispatching a not found
        // error.
        info_!("{} {}", White.paint("Outcome:"), outcome);
        outcome.map_forward(|res| {
            error_!("No further matching routes.");
            // TODO: Have some way to know why this was failed forward. Use that
            // instead of always using an unchained error.
            handle_not_found(res);
        });
    }

    pub fn new(address: &'static str, port: isize) -> Rocket {
        // FIXME: Allow user to override level/disable logging.
        logger::init(logger::Level::Normal);

        Rocket {
            address: address,
            port: port,
            router: Router::new(),
            catchers: catcher::defaults::get(),
        }
    }

    pub fn mount(&mut self, base: &'static str, routes: Vec<Route>)
            -> &mut Self {
        info!("🛰  {} '{}':", Magenta.paint("Mounting"), base);
        for mut route in routes {
            let path = format!("{}/{}", base, route.path.as_str());
            route.set_path(path);

            info_!("{}", route);
            self.router.add(route);
        }

        self
    }

    pub fn catch(&mut self, catchers: Vec<Catcher>) -> &mut Self {
        info!("👾  {}:", Magenta.paint("Catchers"));
        for c in catchers {
            if self.catchers.contains_key(&c.code) &&
                    !self.catchers.get(&c.code).unwrap().is_default() {
                let msg = format!("warning: overrides {} catcher!", c.code);
                info_!("{} ({})", c, Yellow.paint(msg.as_str()));
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
        info!("🚀  {} {}...", White.paint("Rocket has launched from"),
            White.bold().paint(&full_addr));
        let _ = HyperServer::http(full_addr.as_str()).unwrap().handle(self);
    }

    pub fn mount_and_launch(mut self, base: &'static str, routes: Vec<Route>) {
        self.mount(base, routes);
        self.launch();
    }
}
