use std::{io, mem};
use std::sync::Arc;
use std::collections::HashMap;

#[allow(unused_imports)]
use futures::future::FutureExt;
use futures::stream::StreamExt;
use futures::future::{Future, BoxFuture};
use tokio::sync::{mpsc, oneshot};

use yansi::Paint;
use state::Container;
use figment::Figment;

use crate::{logger, handler};
use crate::config::Config;
use crate::request::{Request, FormItems};
use crate::data::Data;
use crate::catcher::Catcher;
use crate::response::{Body, Response};
use crate::router::{Router, Route};
use crate::outcome::Outcome;
use crate::error::{Error, ErrorKind};
use crate::fairing::{Fairing, Fairings};
use crate::logger::PaintExt;
use crate::ext::AsyncReadExt;
use crate::shutdown::Shutdown;

use crate::http::{Method, Status, Header};
use crate::http::private::{Listener, Connection, Incoming};
use crate::http::hyper::{self, header};
use crate::http::uri::Origin;

/// The main `Rocket` type: used to mount routes and catchers and launch the
/// application.
pub struct Rocket {
    pub(crate) config: Config,
    pub(crate) figment: Figment,
    pub(crate) managed_state: Container,
    router: Router,
    default_catcher: Option<Catcher>,
    catchers: HashMap<u16, Catcher>,
    fairings: Fairings,
    shutdown_receiver: Option<mpsc::Receiver<()>>,
    pub(crate) shutdown_handle: Shutdown,
}

// A token returned to force the execution of one method before another.
pub(crate) struct Token;

// This function tries to hide all of the Hyper-ness from Rocket. It essentially
// converts Hyper types into Rocket types, then calls the `dispatch` function,
// which knows nothing about Hyper. Because responding depends on the
// `HyperResponse` type, this function does the actual response processing.
async fn hyper_service_fn(
    rocket: Arc<Rocket>,
    h_addr: std::net::SocketAddr,
    hyp_req: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, io::Error> {
    // This future must return a hyper::Response, but the response body might
    // borrow from the request. Instead, write the body in another future that
    // sends the response metadata (and a body channel) prior.
    let (tx, rx) = oneshot::channel();

    tokio::spawn(async move {
        // Get all of the information from Hyper.
        let (h_parts, h_body) = hyp_req.into_parts();

        // Convert the Hyper request into a Rocket request.
        let req_res = Request::from_hyp(
            &rocket, h_parts.method, h_parts.headers, &h_parts.uri, h_addr
        );

        let mut req = match req_res {
            Ok(req) => req,
            Err(e) => {
                error!("Bad incoming request: {}", e);
                // TODO: We don't have a request to pass in, so we just
                // fabricate one. This is weird. We should let the user know
                // that we failed to parse a request (by invoking some special
                // handler) instead of doing this.
                let dummy = Request::new(&rocket, Method::Get, Origin::dummy());
                let r = rocket.handle_error(Status::BadRequest, &dummy).await;
                return rocket.issue_response(r, tx).await;
            }
        };

        // Retrieve the data from the hyper body.
        let mut data = Data::from_hyp(h_body).await;

        // Dispatch the request to get a response, then write that response out.
        let token = rocket.preprocess_request(&mut req, &mut data).await;
        let r = rocket.dispatch(token, &mut req, data).await;
        rocket.issue_response(r, tx).await;
    });

    // Receive the response written to `tx` by the task above.
    rx.await.map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

impl Rocket {
    #[inline]
    async fn issue_response(
        &self,
        response: Response<'_>,
        tx: oneshot::Sender<hyper::Response<hyper::Body>>,
    ) {
        match self.write_response(response, tx).await {
            Ok(()) => info_!("{}", Paint::green("Response succeeded.")),
            Err(e) => error_!("Failed to write response: {:?}.", e),
        }
    }

    #[inline]
    async fn write_response(
        &self,
        mut response: Response<'_>,
        tx: oneshot::Sender<hyper::Response<hyper::Body>>,
    ) -> io::Result<()> {
        let mut hyp_res = hyper::Response::builder()
            .status(response.status().code);

        for header in response.headers().iter() {
            let name = header.name.as_str();
            let value = header.value.as_bytes();
            hyp_res = hyp_res.header(name, value);
        }

        let send_response = move |res: hyper::ResponseBuilder, body| -> io::Result<()> {
            let response = res.body(body)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            tx.send(response).map_err(|_| {
                let msg = "client disconnected before the response was started";
                io::Error::new(io::ErrorKind::BrokenPipe, msg)
            })
        };

        match response.body_mut() {
            None => {
                hyp_res = hyp_res.header(header::CONTENT_LENGTH, "0");
                send_response(hyp_res, hyper::Body::empty())?;
            }
            Some(body) => {
                if let Some(s) = body.size().await {
                    hyp_res = hyp_res.header(header::CONTENT_LENGTH, s.to_string());
                }

                let chunk_size = match *body {
                    Body::Chunked(_, chunk_size) => chunk_size as usize,
                    Body::Sized(_, _) => crate::response::DEFAULT_CHUNK_SIZE,
                };

                let (mut sender, hyp_body) = hyper::Body::channel();
                send_response(hyp_res, hyp_body)?;

                let mut stream = body.as_reader().into_bytes_stream(chunk_size);
                while let Some(next) = stream.next().await {
                    sender.send_data(next?).await
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                }
            }
        };

        Ok(())
    }

    /// Preprocess the request for Rocket things. Currently, this means:
    ///
    ///   * Rewriting the method in the request if _method form field exists.
    ///   * Run the request fairings.
    ///
    /// Keep this in-sync with derive_form when preprocessing form fields.
    pub(crate) async fn preprocess_request(
        &self,
        req: &mut Request<'_>,
        data: &mut Data
    ) -> Token {
        // Check if this is a form and if the form contains the special _method
        // field which we use to reinterpret the request's method.
        let (min_len, max_len) = ("_method=get".len(), "_method=delete".len());
        let peek_buffer = data.peek(max_len).await;
        let is_form = req.content_type().map_or(false, |ct| ct.is_form());

        if is_form && req.method() == Method::Post && peek_buffer.len() >= min_len {
            if let Ok(form) = std::str::from_utf8(peek_buffer) {
                let method: Option<Result<Method, _>> = FormItems::from(form)
                    .filter(|item| item.key.as_str() == "_method")
                    .map(|item| item.value.parse())
                    .next();

                if let Some(Ok(method)) = method {
                    req._set_method(method);
                }
            }
        }

        // Run request fairings.
        self.fairings.handle_request(req, data).await;

        Token
    }

    /// Route the request and process the outcome to eventually get a response.
    fn route_and_process<'s, 'r: 's>(
        &'s self,
        request: &'r Request<'s>,
        data: Data
    ) -> impl Future<Output = Response<'r>> + Send + 's {
        async move {
            let mut response = match self.route(request, data).await {
                Outcome::Success(response) => response,
                Outcome::Forward(data) => {
                    // There was no matching route. Autohandle `HEAD` requests.
                    if request.method() == Method::Head {
                        info_!("Autohandling {} request.", Paint::default("HEAD").bold());

                        // Dispatch the request again with Method `GET`.
                        request._set_method(Method::Get);

                        // Return early so we don't set cookies twice.
                        let try_next: BoxFuture<'_, _> =
                            Box::pin(self.route_and_process(request, data));
                        return try_next.await;
                    } else {
                        // No match was found and it can't be autohandled. 404.
                        self.handle_error(Status::NotFound, request).await
                    }
                }
                Outcome::Failure(status) => self.handle_error(status, request).await,
            };

            // Set the cookies. Note that error responses will only include
            // cookies set by the error handler. See `handle_error` for more.
            let delta_jar = request.cookies().take_delta_jar();
            for cookie in delta_jar.delta() {
                response.adjoin_header(cookie);
            }

            response
        }
    }

    /// Tries to find a `Responder` for a given `request`. It does this by
    /// routing the request and calling the handler for each matching route
    /// until one of the handlers returns success or failure, or there are no
    /// additional routes to try (forward). The corresponding outcome for each
    /// condition is returned.
    //
    // TODO: We _should_ be able to take an `&mut` here and mutate the request
    // at any pointer _before_ we pass it to a handler as long as we drop the
    // outcome. That should be safe. Since no mutable borrow can be held
    // (ensuring `handler` takes an immutable borrow), any caller to `route`
    // should be able to supply an `&mut` and retain an `&` after the call.
    #[inline]
    pub(crate) fn route<'s, 'r: 's>(
        &'s self,
        request: &'r Request<'s>,
        mut data: Data,
    ) -> impl Future<Output = handler::Outcome<'r>> + 's {
        async move {
            // Go through the list of matching routes until we fail or succeed.
            let matches = self.router.route(request);
            for route in matches {
                // Retrieve and set the requests parameters.
                info_!("Matched: {}", route);
                request.set_route(route);

                // Dispatch the request to the handler.
                let outcome = route.handler.handle(request, data).await;

                // Check if the request processing completed (Some) or if the
                // request needs to be forwarded. If it does, continue the loop
                // (None) to try again.
                info_!("{} {}", Paint::default("Outcome:").bold(), outcome);
                match outcome {
                    o@Outcome::Success(_) | o@Outcome::Failure(_) => return o,
                    Outcome::Forward(unused_data) => data = unused_data,
                }
            }

            error_!("No matching routes for {}.", request);
            Outcome::Forward(data)
        }
    }

    #[inline]
    pub(crate) async fn dispatch<'s, 'r: 's>(
        &'s self,
        _token: Token,
        request: &'r Request<'s>,
        data: Data
    ) -> Response<'r> {
        info!("{}:", request);

        // Remember if the request is `HEAD` for later body stripping.
        let was_head_request = request.method() == Method::Head;

        // Route the request and run the user's handlers.
        let mut response = self.route_and_process(request, data).await;

        // Add a default 'Server' header if it isn't already there.
        // TODO: If removing Hyper, write out `Date` header too.
        if !response.headers().contains("Server") {
            response.set_header(Header::new("Server", "Rocket"));
        }

        // Run the response fairings.
        self.fairings.handle_response(request, &mut response).await;

        // Strip the body if this is a `HEAD` request.
        if was_head_request {
            response.strip_body();
        }

        response
    }

    // Finds the error catcher for the status `status` and executes it for the
    // given request `req`. If a user has registered a catcher for `status`, the
    // catcher is called. If the catcher fails to return a good response, the
    // 500 catcher is executed. If there is no registered catcher for `status`,
    // the default catcher is used.
    pub(crate) fn handle_error<'s, 'r: 's>(
        &'s self,
        status: Status,
        req: &'r Request<'s>
    ) -> impl Future<Output = Response<'r>> + 's {
        async move {
            warn_!("Responding with {} catcher.", Paint::red(&status));

            // For now, we reset the delta state to prevent any modifications
            // from earlier, unsuccessful paths from being reflected in error
            // response. We may wish to relax this in the future.
            req.cookies().reset_delta();

            // Try to get the active catcher but fallback to user's 500 catcher.
            let code = Paint::red(status.code);
            let response = if let Some(catcher) = self.catchers.get(&status.code) {
                catcher.handler.handle(status, req).await
            } else if let Some(ref default) =  self.default_catcher {
                warn_!("No {} catcher found. Using default catcher.", code);
                default.handler.handle(status, req).await
            } else {
                warn_!("No {} or default catcher found. Using Rocket default catcher.", code);
                crate::catcher::default(status, req)
            };

            // Dispatch to the catcher. If it fails, use the Rocket default 500.
            match response {
                Ok(r) => r,
                Err(err_status) => {
                    error_!("Catcher unexpectedly failed with {}.", err_status);
                    warn_!("Using Rocket's default 500 error catcher.");
                    let default = crate::catcher::default(Status::InternalServerError, req);
                    default.expect("Rocket has default 500 response")
                }
            }
        }
    }

    // TODO.async: Solidify the Listener APIs and make this function public
    async fn listen_on<L>(mut self, listener: L) -> Result<(), Error>
        where L: Listener + Send + Unpin + 'static,
              <L as Listener>::Connection: Send + Unpin + 'static,
    {
        // We do this twice if `listen_on` was called through `launch()` but
        // only once if `listen_on()` gets called directly.
        self.prelaunch_check().await?;

        // Freeze managed state for synchronization-free accesses later.
        self.managed_state.freeze();

        // Run the launch fairings.
        self.fairings.pretty_print_counts();
        self.fairings.handle_launch(&self);

        // Determine the address and port we actually bound to.
        self.config.port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let proto = self.config.tls.as_ref().map_or("http://", |_| "https://");
        let full_addr = format!("{}:{}", self.config.address, self.config.port);

        launch_info!("{}{} {}{}",
                     Paint::emoji("🚀 "),
                     Paint::default("Rocket has launched from").bold(),
                     Paint::default(proto).bold().underline(),
                     Paint::default(&full_addr).bold().underline());

        // Determine keep-alives.
        let http1_keepalive = self.config.keep_alive != 0;
        let http2_keep_alive = match self.config.keep_alive {
            0 => None,
            n => Some(std::time::Duration::from_secs(n as u64))
        };

        // We need to get this before moving `self` into an `Arc`.
        let mut shutdown_receiver = self.shutdown_receiver.take()
            .expect("shutdown receiver has already been used");

        let rocket = Arc::new(self);
        let service = hyper::make_service_fn(move |conn: &<L as Listener>::Connection| {
            let rocket = rocket.clone();
            let remote = conn.remote_addr().unwrap_or_else(|| ([0, 0, 0, 0], 0).into());
            async move {
                Ok::<_, std::convert::Infallible>(hyper::service_fn(move |req| {
                    hyper_service_fn(rocket.clone(), remote, req)
                }))
            }
        });

        #[derive(Clone)]
        struct TokioExecutor;

        impl<Fut> hyper::Executor<Fut> for TokioExecutor
            where Fut: Future + Send + 'static, Fut::Output: Send
        {
            fn execute(&self, fut: Fut) {
                tokio::spawn(fut);
            }
        }

        hyper::Server::builder(Incoming::from_listener(listener))
            .http1_keepalive(http1_keepalive)
            .http2_keep_alive_interval(http2_keep_alive)
            .executor(TokioExecutor)
            .serve(service)
            .with_graceful_shutdown(async move { shutdown_receiver.recv().await; })
            .await
            .map_err(|e| Error::new(ErrorKind::Runtime(Box::new(e))))
    }
}

impl Rocket {
    /// Create a new `Rocket` application using the configuration information in
    /// `Rocket.toml`. If the file does not exist or if there is an I/O error
    /// reading the file, the defaults, overridden by any environment-based
    /// paramparameters, are used. See the [`config`](crate::config)
    /// documentation for more information on defaults.
    ///
    /// This method is typically called through the
    /// [`rocket::ignite()`](crate::ignite) alias.
    ///
    /// # Panics
    ///
    /// If there is an error reading configuration sources, this function prints
    /// a nice error message and then exits the process.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # {
    /// rocket::ignite()
    /// # };
    /// ```
    pub fn ignite() -> Rocket {
        Rocket::custom(Config::figment())
    }

    /// Creates a new `Rocket` application using the supplied configuration
    /// provider. This method is typically called through the
    /// [`rocket::custom()`](crate::custom()) alias.
    ///
    /// # Panics
    ///
    /// If there is an error reading configuration sources, this function prints
    /// a nice error message and then exits the process.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use figment::{Figment, providers::{Toml, Env, Format}};
    ///
    /// #[rocket::launch]
    /// fn rocket() -> _ {
    ///     let figment = Figment::from(rocket::Config::default())
    ///         .merge(Toml::file("MyApp.toml").nested())
    ///         .merge(Env::prefixed("MY_APP_"));
    ///
    ///     rocket::custom(figment)
    /// }
    /// ```
    #[inline]
    pub fn custom<T: figment::Provider>(provider: T) -> Rocket {
        let (config, figment) = (Config::from(&provider), Figment::from(provider));
        logger::try_init(config.log_level, config.cli_colors, false);
        config.pretty_print(figment.profile());

        let managed_state = Container::new();
        let (shutdown_sender, shutdown_receiver) = mpsc::channel(1);
        Rocket {
            config, figment,
            managed_state,
            shutdown_handle: Shutdown(shutdown_sender),
            router: Router::new(),
            default_catcher: None,
            catchers: HashMap::new(),
            fairings: Fairings::new(),
            shutdown_receiver: Some(shutdown_receiver),
        }
    }

    /// Mounts all of the routes in the supplied vector at the given `base`
    /// path. Mounting a route with path `path` at path `base` makes the route
    /// available at `base/path`.
    ///
    /// # Panics
    ///
    /// Panics if the `base` mount point is not a valid static path: a valid
    /// origin URI without dynamic parameters.
    ///
    /// Panics if any route's URI is not a valid origin URI. This kind of panic
    /// is guaranteed not to occur if the routes were generated using Rocket's
    /// code generation.
    ///
    /// # Examples
    ///
    /// Use the `routes!` macro to mount routes created using the code
    /// generation facilities. Requests to the `/hello/world` URI will be
    /// dispatched to the `hi` route.
    ///
    /// ```rust,no_run
    /// # #[macro_use] extern crate rocket;
    /// #
    /// #[get("/world")]
    /// fn hi() -> &'static str {
    ///     "Hello!"
    /// }
    ///
    /// #[launch]
    /// fn rocket() -> rocket::Rocket {
    ///     rocket::ignite().mount("/hello", routes![hi])
    /// }
    /// ```
    ///
    /// Manually create a route named `hi` at path `"/world"` mounted at base
    /// `"/hello"`. Requests to the `/hello/world` URI will be dispatched to the
    /// `hi` route.
    ///
    /// ```rust
    /// use rocket::{Request, Route, Data};
    /// use rocket::handler::{HandlerFuture, Outcome};
    /// use rocket::http::Method::*;
    ///
    /// fn hi<'r>(req: &'r Request, _: Data) -> HandlerFuture<'r> {
    ///     Outcome::from(req, "Hello!").pin()
    /// }
    ///
    /// # let _ = async { // We don't actually want to launch the server in an example.
    /// rocket::ignite().mount("/hello", vec![Route::new(Get, "/world", hi)])
    /// #     .launch().await;
    /// # };
    /// ```
    #[inline]
    pub fn mount<R: Into<Vec<Route>>>(mut self, base: &str, routes: R) -> Self {
        let base_uri = Origin::parse_owned(base.to_string())
            .unwrap_or_else(|e| {
                error!("Invalid mount point URI: {}.", Paint::white(base));
                panic!("Error: {}", e);
            });

        if base_uri.query().is_some() {
            error!("Mount point '{}' contains query string.", base);
            panic!("Invalid mount point.");
        }

        info!("{}{} {}{}",
              Paint::emoji("🛰  "),
              Paint::magenta("Mounting"),
              Paint::blue(&base_uri),
              Paint::magenta(":"));

        for route in routes.into() {
            let old_route = route.clone();
            let route = route.map_base(|old| format!("{}{}", base, old))
                .unwrap_or_else(|e| {
                    error_!("Route `{}` has a malformed URI.", old_route);
                    error_!("{}", e);
                    panic!("Invalid route URI.");
                });

            info_!("{}", route);
            self.router.add(route);
        }

        self
    }

    /// Registers all of the catchers in the supplied vector.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # #[macro_use] extern crate rocket;
    /// use rocket::Request;
    ///
    /// #[catch(500)]
    /// fn internal_error() -> &'static str {
    ///     "Whoops! Looks like we messed up."
    /// }
    ///
    /// #[catch(400)]
    /// fn not_found(req: &Request) -> String {
    ///     format!("I couldn't find '{}'. Try something else?", req.uri())
    /// }
    ///
    /// #[launch]
    /// fn rocket() -> rocket::Rocket {
    ///     rocket::ignite().register(catchers![internal_error, not_found])
    /// }
    /// ```
    #[inline]
    pub fn register(mut self, catchers: Vec<Catcher>) -> Self {
        info!("{}{}", Paint::emoji("👾 "), Paint::magenta("Catchers:"));

        for catcher in catchers {
            info_!("{}", catcher);

            let existing = match catcher.code {
                Some(code) => self.catchers.insert(code, catcher),
                None => self.default_catcher.replace(catcher)
            };

            if let Some(existing) = existing {
                warn_!("Replacing existing '{}' catcher.", existing);
            }
        }

        self
    }

    /// Add `state` to the state managed by this instance of Rocket.
    ///
    /// This method can be called any number of times as long as each call
    /// refers to a different `T`.
    ///
    /// Managed state can be retrieved by any request handler via the
    /// [`State`](crate::State) request guard. In particular, if a value of type `T`
    /// is managed by Rocket, adding `State<T>` to the list of arguments in a
    /// request handler instructs Rocket to retrieve the managed value.
    ///
    /// # Panics
    ///
    /// Panics if state of type `T` is already being managed.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[macro_use] extern crate rocket;
    /// use rocket::State;
    ///
    /// struct MyValue(usize);
    ///
    /// #[get("/")]
    /// fn index(state: State<MyValue>) -> String {
    ///     format!("The stateful value is: {}", state.0)
    /// }
    ///
    /// #[launch]
    /// fn rocket() -> rocket::Rocket {
    ///     rocket::ignite()
    ///         .mount("/", routes![index])
    ///         .manage(MyValue(10))
    /// }
    /// ```
    #[inline]
    pub fn manage<T: Send + Sync + 'static>(self, state: T) -> Self {
        let type_name = std::any::type_name::<T>();
        if !self.managed_state.set(state) {
            error!("State for type '{}' is already being managed!", type_name);
            panic!("Aborting due to duplicately managed state.");
        }

        self
    }

    /// Attaches a fairing to this instance of Rocket. If the fairing is an
    /// _attach_ fairing, it is run immediately. All other kinds of fairings
    /// will be executed at their appropriate time.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[macro_use] extern crate rocket;
    /// use rocket::Rocket;
    /// use rocket::fairing::AdHoc;
    ///
    /// #[launch]
    /// fn rocket() -> rocket::Rocket {
    ///     rocket::ignite()
    ///         .attach(AdHoc::on_launch("Launch Message", |_| {
    ///             println!("Rocket is launching!");
    ///         }))
    /// }
    /// ```
    #[inline]
    pub fn attach<F: Fairing>(mut self, fairing: F) -> Self {
        let future = async move {
            let fairing = Box::new(fairing);
            let mut fairings = mem::replace(&mut self.fairings, Fairings::new());
            let rocket = fairings.attach(fairing, self).await;
            (rocket, fairings)
        };

        // TODO: Reuse a single thread to run all attach fairings.
        let (rocket, mut fairings) = match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                std::thread::spawn(move || {
                    handle.block_on(future)
                }).join().unwrap()
            }
            Err(_) => {
                std::thread::spawn(|| {
                    futures::executor::block_on(future)
                }).join().unwrap()
            }
        };

        self = rocket;

        // Note that `self.fairings` may now be non-empty! Move them to the end.
        fairings.append(self.fairings);
        self.fairings = fairings;
        self
    }

    /// Returns the active configuration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[macro_use] extern crate rocket;
    /// use rocket::Rocket;
    /// use rocket::fairing::AdHoc;
    ///
    /// #[launch]
    /// fn rocket() -> rocket::Rocket {
    ///     rocket::ignite()
    ///         .attach(AdHoc::on_launch("Config Printer", |rocket| {
    ///             println!("Rocket launch config: {:?}", rocket.config());
    ///         }))
    /// }
    /// ```
    #[inline(always)]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns the figment for configured provider.
    ///
    /// # Example
    ///
    /// ```rust
    /// let rocket = rocket::ignite();
    /// let figment = rocket.figment();
    ///
    /// let port: u16 = figment.extract_inner("port").unwrap();
    /// assert_eq!(port, rocket.config().port);
    /// ```
    #[inline(always)]
    pub fn figment(&self) -> &Figment {
        &self.figment
    }

    /// Returns an iterator over all of the routes mounted on this instance of
    /// Rocket. The order is unspecified.
    ///
    /// # Example
    ///
    /// ```rust
    /// # #[macro_use] extern crate rocket;
    /// use rocket::Rocket;
    /// use rocket::fairing::AdHoc;
    ///
    /// #[get("/hello")]
    /// fn hello() -> &'static str {
    ///     "Hello, world!"
    /// }
    ///
    /// fn main() {
    ///     let mut rocket = rocket::ignite()
    ///         .mount("/", routes![hello])
    ///         .mount("/hi", routes![hello]);
    ///
    ///     for route in rocket.routes() {
    ///         match route.base() {
    ///             "/" => assert_eq!(route.uri.path(), "/hello"),
    ///             "/hi" => assert_eq!(route.uri.path(), "/hi/hello"),
    ///             _ => unreachable!("only /hello, /hi/hello are expected")
    ///         }
    ///     }
    ///
    ///     assert_eq!(rocket.routes().count(), 2);
    /// }
    /// ```
    #[inline(always)]
    pub fn routes(&self) -> impl Iterator<Item = &Route> + '_ {
        self.router.routes()
    }

    /// Returns an iterator over all of the catchers registered on this instance
    /// of Rocket. The order is unspecified.
    ///
    /// # Example
    ///
    /// ```rust
    /// # #[macro_use] extern crate rocket;
    /// use rocket::Rocket;
    /// use rocket::fairing::AdHoc;
    ///
    /// #[catch(404)] fn not_found() -> &'static str { "Nothing here, sorry!" }
    /// #[catch(500)] fn just_500() -> &'static str { "Whoops!?" }
    /// #[catch(default)] fn some_default() -> &'static str { "Everything else." }
    ///
    /// fn main() {
    ///     let mut rocket = rocket::ignite()
    ///         .register(catchers![not_found, just_500, some_default]);
    ///
    ///     let mut codes: Vec<_> = rocket.catchers().map(|c| c.code).collect();
    ///     codes.sort();
    ///
    ///     assert_eq!(codes, vec![None, Some(404), Some(500)]);
    /// }
    /// ```
    #[inline(always)]
    pub fn catchers(&self) -> impl Iterator<Item = &Catcher> + '_ {
        self.catchers.values().chain(self.default_catcher.as_ref())
    }

    /// Returns `Some` of the managed state value for the type `T` if it is
    /// being managed by `self`. Otherwise, returns `None`.
    ///
    /// # Example
    ///
    /// ```rust
    /// #[derive(PartialEq, Debug)]
    /// struct MyState(&'static str);
    ///
    /// let rocket = rocket::ignite().manage(MyState("hello!"));
    /// assert_eq!(rocket.state::<MyState>(), Some(&MyState("hello!")));
    /// ```
    #[inline(always)]
    pub fn state<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.managed_state.try_get()
    }

    /// Returns a handle which can be used to gracefully terminate this instance
    /// of Rocket. In routes, use the [`Shutdown`] request guard.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use std::{thread, time::Duration};
    /// # rocket::async_test(async {
    /// let mut rocket = rocket::ignite();
    /// let handle = rocket.shutdown();
    ///
    /// thread::spawn(move || {
    ///     thread::sleep(Duration::from_secs(10));
    ///     handle.shutdown();
    /// });
    ///
    /// // Shuts down after 10 seconds
    /// let shutdown_result = rocket.launch().await;
    /// assert!(shutdown_result.is_ok());
    /// # });
    /// ```
    #[inline(always)]
    pub fn shutdown(&self) -> Shutdown {
        self.shutdown_handle.clone()
    }

    /// Perform "pre-launch" checks: verify that there are no routing colisions
    /// and that there were no fairing failures.
    pub(crate) async fn prelaunch_check(&mut self) -> Result<(), Error> {
        if let Err(e) = self.router.collisions() {
            return Err(Error::new(ErrorKind::Collision(e)));
        }

        if let Some(failures) = self.fairings.failures() {
            return Err(Error::new(ErrorKind::FailedFairings(failures.to_vec())))
        }

        Ok(())
    }

    /// Returns a `Future` that drives the server, listening for and dispatching
    /// requests to mounted routes and catchers. The `Future` completes when the
    /// server is shut down via [`Shutdown`], encounters a fatal error, or if
    /// the the `ctrlc` configuration option is set, when `Ctrl+C` is pressed.
    ///
    /// # Error
    ///
    /// If there is a problem starting the application, an [`Error`] is
    /// returned. Note that a value of type `Error` panics if dropped without
    /// first being inspected. See the [`Error`] documentation for more
    /// information.
    ///
    /// # Example
    ///
    /// ```rust
    /// #[rocket::main]
    /// async fn main() {
    /// # if false {
    ///     let result = rocket::ignite().launch().await;
    ///     assert!(result.is_ok());
    /// # }
    /// }
    /// ```
    pub async fn launch(mut self) -> Result<(), Error> {
        use std::net::ToSocketAddrs;
        use futures::future::Either;
        use crate::http::private::bind_tcp;

        self.prelaunch_check().await?;

        let full_addr = format!("{}:{}", self.config.address, self.config.port);
        let addr = full_addr.to_socket_addrs()
            .map(|mut addrs| addrs.next().expect(">= 1 socket addr"))
            .map_err(|e| Error::new(ErrorKind::Io(e)))?;

        // If `ctrl-c` shutdown is enabled, we `select` on `the ctrl-c` signal
        // and server. Otherwise, we only wait on the `server`, hence `pending`.
        let shutdown_handle = self.shutdown_handle.clone();
        let shutdown_signal = match self.config.ctrlc {
            true => tokio::signal::ctrl_c().boxed(),
            false => futures::future::pending().boxed(),
        };

        #[cfg(feature = "tls")]
        let server = {
            use crate::http::tls::bind_tls;

            if let Some(tls_config) = &self.config.tls {
                let (certs, key) = tls_config.to_readers().map_err(ErrorKind::Io)?;
                let l = bind_tls(addr, certs, key).await.map_err(ErrorKind::Bind)?;
                self.listen_on(l).boxed()
            } else {
                let l = bind_tcp(addr).await.map_err(ErrorKind::Bind)?;
                self.listen_on(l).boxed()
            }
        };

        #[cfg(not(feature = "tls"))]
        let server = {
            let l = bind_tcp(addr).await.map_err(ErrorKind::Bind)?;
            self.listen_on(l).boxed()
        };

        match futures::future::select(shutdown_signal, server).await {
            Either::Left((Ok(()), server)) => {
                // Ctrl-was pressed. Signal shutdown, wait for the server.
                shutdown_handle.shutdown();
                server.await
            }
            Either::Left((Err(err), server)) => {
                // Error setting up ctrl-c signal. Let the user know.
                warn!("Failed to enable `ctrl-c` graceful signal shutdown.");
                info_!("Error: {}", err);
                server.await
            }
            // Server shut down before Ctrl-C; return the result.
            Either::Right((result, _)) => result,
        }
    }
}
