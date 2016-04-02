mod collider;
mod route;
mod uri;

pub use self::collider::Collider;
pub use self::uri::{URI, URIBuf};
pub use self::route::Route;

use term_painter::ToStyle;
use term_painter::Color::*;
use std::collections::hash_map::HashMap;
use method::Method;

type Selector = (Method, usize);

pub struct Router {
    routes: HashMap<Selector, Vec<Route>> // for now
}

impl Router {
    pub fn new() -> Router {
        Router {
            routes: HashMap::new()
        }
    }

    pub fn add(&mut self, route: Route) {
        let selector = (route.method, route.path.segment_count());
        self.routes.entry(selector).or_insert(vec![]).push(route);
    }

    // TODO: Make a `Router` trait with this function. Rename this `Router`
    // struct to something like `RocketRouter`. If that happens, returning a
    // `Route` structure is inflexible. Have it be an associated type.
    pub fn route<'b>(&'b self, method: Method, uri: &str) -> Option<&'b Route> {
        let mut matched_route = None;

        let path = URI::new(uri);
        let num_segments = path.segment_count();
        if let Some(routes) = self.routes.get(&(method, num_segments)) {
            for route in routes.iter().filter(|r| r.collides_with(uri)) {
                println!("\t=> Matched {} to: {}", uri, route);
                if let None = matched_route {
                    matched_route = Some(route);
                }
            }
        }

        matched_route
    }

    pub fn has_collisions(&self) -> bool {
        let mut result = false;
        for routes in self.routes.values() {
            for i in 0..routes.len() {
                for j in (i + 1)..routes.len() {
                    let (a_route, b_route) = (&routes[i], &routes[j]);
                    if a_route.collides_with(b_route) {
                        result = true;
                        println!("{} {} and {} collide!",
                            Yellow.bold().paint("Warning:"),
                            a_route, b_route);
                    }
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod test {
    use Method::*;
    use super::{Router, Route};
    use {Response, Request};

    fn dummy_handler(_req: Request) -> Response<'static> {
        Response::empty()
    }

    fn router_with_routes(routes: &[&'static str]) -> Router {
        let mut router = Router::new();
        for route in routes {
            let route = Route::new(Get, route.to_string(), dummy_handler);
            router.add(route);
        }

        router
    }

    #[test]
    fn test_collisions() {
        let router = router_with_routes(&["/hello", "/hello"]);
        assert!(router.has_collisions());

        let router = router_with_routes(&["/<a>", "/hello"]);
        assert!(router.has_collisions());

        let router = router_with_routes(&["/<a>", "/<b>"]);
        assert!(router.has_collisions());

        let router = router_with_routes(&["/hello/bob", "/hello/<b>"]);
        assert!(router.has_collisions());
    }

    #[test]
    fn test_ok_routing() {
        let router = router_with_routes(&["/hello"]);
        assert!(router.route(Get, "/hello").is_some());

        let router = router_with_routes(&["/<a>"]);
        assert!(router.route(Get, "/hello").is_some());
        assert!(router.route(Get, "/hi").is_some());
        assert!(router.route(Get, "/bobbbbbbbbbby").is_some());
        assert!(router.route(Get, "/dsfhjasdf").is_some());

        let router = router_with_routes(&["/<a>/<b>"]);
        assert!(router.route(Get, "/hello/hi").is_some());
        assert!(router.route(Get, "/a/b/").is_some());
        assert!(router.route(Get, "/i/a").is_some());
        assert!(router.route(Get, "/jdlk/asdij").is_some());

        let mut router = Router::new();
        router.add(Route::new(Put, "/hello".to_string(), dummy_handler));
        router.add(Route::new(Post, "/hello".to_string(), dummy_handler));
        router.add(Route::new(Delete, "/hello".to_string(), dummy_handler));
        assert!(router.route(Put, "/hello").is_some());
        assert!(router.route(Post, "/hello").is_some());
        assert!(router.route(Delete, "/hello").is_some());
    }

    #[test]
    fn test_err_routing() {
        let router = router_with_routes(&["/hello"]);
        assert!(router.route(Put, "/hello").is_none());
        assert!(router.route(Post, "/hello").is_none());
        assert!(router.route(Options, "/hello").is_none());
        assert!(router.route(Get, "/hell").is_none());
        assert!(router.route(Get, "/hi").is_none());
        assert!(router.route(Get, "/hello/there").is_none());
        assert!(router.route(Get, "/hello/i").is_none());
        assert!(router.route(Get, "/hillo").is_none());

        let router = router_with_routes(&["/<a>"]);
        assert!(router.route(Put, "/hello").is_none());
        assert!(router.route(Post, "/hello").is_none());
        assert!(router.route(Options, "/hello").is_none());
        assert!(router.route(Get, "/hello/there").is_none());
        assert!(router.route(Get, "/hello/i").is_none());

        let router = router_with_routes(&["/<a>/<b>"]);
        assert!(router.route(Get, "/a/b/c").is_none());
        assert!(router.route(Get, "/a").is_none());
        assert!(router.route(Get, "/a/").is_none());
        assert!(router.route(Get, "/a/b/c/d").is_none());
        assert!(router.route(Put, "/hello/hi").is_none());
        assert!(router.route(Put, "/a/b").is_none());
        assert!(router.route(Put, "/a/b").is_none());
    }

    fn match_params(router: &Router, path: &str, expected: &[&str]) -> bool {
        router.route(Get, path).map_or(false, |route| {
            let params = route.get_params(path);
            if params.len() != expected.len() {
                return false;
            }

            for i in 0..params.len() {
                if params[i] != expected[i] {
                    return false;
                }
            }

            true
        })
    }

    #[test]
    fn test_params() {
        let router = router_with_routes(&["/<a>"]);
        assert!(match_params(&router, "/hello", &["hello"]));
        assert!(match_params(&router, "/hi", &["hi"]));
        assert!(match_params(&router, "/bob", &["bob"]));
        assert!(match_params(&router, "/i", &["i"]));

        let router = router_with_routes(&["/hello"]);
        assert!(match_params(&router, "/hello", &[]));

        let router = router_with_routes(&["/<a>/<b>"]);
        assert!(match_params(&router, "/a/b", &["a", "b"]));
        assert!(match_params(&router, "/912/sas", &["912", "sas"]));

        let router = router_with_routes(&["/hello/<b>"]);
        assert!(match_params(&router, "/hello/b", &["b"]));
        assert!(match_params(&router, "/hello/sergio", &["sergio"]));

        let router = router_with_routes(&["/hello/<b>/age"]);
        assert!(match_params(&router, "/hello/sergio/age", &["sergio"]));
        assert!(match_params(&router, "/hello/you/age", &["you"]));
    }
}
