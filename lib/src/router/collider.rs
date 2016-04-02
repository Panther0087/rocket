pub trait Collider<T: ?Sized = Self> {
    fn collides_with(&self, other: &T) -> bool;
}

pub fn index_match_until(break_c: char, a: &str, b: &str, dir: bool)
        -> Option<(isize, isize)> {
    let (a_len, b_len) = (a.len() as isize, b.len() as isize);
    let (mut i, mut j, delta) = if dir {
        (0, 0, 1)
    } else {
        (a_len - 1, b_len - 1, -1)
    };

    while i >= 0 && j >= 0 && i < a_len && j < b_len {
        let (c1, c2) = (a.char_at(i as usize), b.char_at(j as usize));
        if c1 == break_c || c2 == break_c {
            break;
        } else if c1 != c2 {
            return None;
        } else {
            i += delta;
            j += delta;
        }
    }

    return Some((i, j));
}

fn do_match_until(break_c: char, a: &str, b: &str, dir: bool) -> bool {
    index_match_until(break_c, a, b, dir).is_some()
}

impl<'a> Collider<str> for &'a str {
    fn collides_with(&self, other: &str) -> bool {
        let (a, b) = (self, other);
        do_match_until('<', a, b, true) && do_match_until('>', a, b, false)
    }
}

#[cfg(test)]
mod tests {
    use router::Collider;
    use router::route::Route;
    use Method;
    use Method::*;
    use {Request, Response};

    type SimpleRoute = (Method, &'static str);

    fn dummy_handler(_req: Request) -> Response<'static> {
        Response::empty()
    }

    fn m_collide(a: SimpleRoute, b: SimpleRoute) -> bool {
        let route_a = Route::new(a.0, a.1.to_string(), dummy_handler);
        route_a.collides_with(&Route::new(b.0, b.1.to_string(), dummy_handler))
    }

    fn unranked_collide(a: &'static str, b: &'static str) -> bool {
        let route_a = Route::ranked(0, Get, a.to_string(), dummy_handler);
        route_a.collides_with(&Route::ranked(0, Get, b.to_string(), dummy_handler))
    }

    fn s_r_collide(a: &'static str, b: &'static str) -> bool {
        a.collides_with(&Route::new(Get, b.to_string(), dummy_handler))
    }

    fn r_s_collide(a: &'static str, b: &'static str) -> bool {
        let route_a = Route::new(Get, a.to_string(), dummy_handler);
        route_a.collides_with(b)
    }

    fn s_s_collide(a: &'static str, b: &'static str) -> bool {
        a.collides_with(b)
    }

    #[test]
    fn simple_collisions() {
        assert!(unranked_collide("a", "a"));
        assert!(unranked_collide("/a", "/a"));
        assert!(unranked_collide("/hello", "/hello"));
        assert!(unranked_collide("/hello", "/hello/"));
        assert!(unranked_collide("/hello/there/how/ar", "/hello/there/how/ar"));
        assert!(unranked_collide("/hello/there", "/hello/there/"));
    }

    #[test]
    fn simple_param_collisions() {
        assert!(unranked_collide("/hello/<name>", "/hello/<person>"));
        assert!(unranked_collide("/hello/<name>/hi", "/hello/<person>/hi"));
        assert!(unranked_collide("/hello/<name>/hi/there", "/hello/<person>/hi/there"));
        assert!(unranked_collide("/<name>/hi/there", "/<person>/hi/there"));
        assert!(unranked_collide("/<name>/hi/there", "/dude/<name>/there"));
        assert!(unranked_collide("/<name>/<a>/<b>", "/<a>/<b>/<c>"));
        assert!(unranked_collide("/<name>/<a>/<b>/", "/<a>/<b>/<c>/"));
    }

    #[test]
    fn medium_param_collisions() {
        assert!(unranked_collide("/hello/<name>", "/hello/bob"));
        assert!(unranked_collide("/<name>", "//bob"));
    }

    #[test]
    fn hard_param_collisions() {
        assert!(unranked_collide("/<name>bob", "/<name>b"));
        assert!(unranked_collide("/a<b>c", "/abc"));
        assert!(unranked_collide("/a<b>c", "/azooc"));
        assert!(unranked_collide("/a<b>", "/a"));
        assert!(unranked_collide("/<b>", "/a"));
        assert!(unranked_collide("/<a>/<b>", "/a/b<c>"));
        assert!(unranked_collide("/<a>/bc<b>", "/a/b<c>"));
        assert!(unranked_collide("/<a>/bc<b>d", "/a/b<c>"));
    }

    #[test]
    fn non_collisions() {
        assert!(!unranked_collide("/a", "/b"));
        assert!(!unranked_collide("/a/b", "/a"));
        assert!(!unranked_collide("/a/b", "/a/c"));
        assert!(!unranked_collide("/a/hello", "/a/c"));
        assert!(!unranked_collide("/hello", "/a/c"));
        assert!(!unranked_collide("/hello/there", "/hello/there/guy"));
        assert!(!unranked_collide("/b<a>/there", "/hi/there"));
        assert!(!unranked_collide("/<a>/<b>c", "/hi/person"));
        assert!(!unranked_collide("/<a>/<b>cd", "/hi/<a>e"));
        assert!(!unranked_collide("/a<a>/<b>", "/b<b>/<a>"));
        assert!(!unranked_collide("/a/<b>", "/b/<b>"));
        assert!(!unranked_collide("/a<a>/<b>", "/b/<b>"));
    }

    #[test]
    fn method_dependent_non_collisions() {
        assert!(!m_collide((Get, "/"), (Post, "/")));
        assert!(!m_collide((Post, "/"), (Put, "/")));
        assert!(!m_collide((Put, "/a"), (Put, "/")));
        assert!(!m_collide((Post, "/a"), (Put, "/")));
        assert!(!m_collide((Get, "/a"), (Put, "/")));
        assert!(!m_collide((Get, "/hello"), (Put, "/hello")));
    }

    #[test]
    fn test_str_non_collisions() {
        assert!(!s_r_collide("/a", "/b"));
        assert!(!s_r_collide("/a/b", "/a"));
        assert!(!s_r_collide("/a/b", "/a/c"));
        assert!(!s_r_collide("/a/hello", "/a/c"));
        assert!(!s_r_collide("/hello", "/a/c"));
        assert!(!s_r_collide("/hello/there", "/hello/there/guy"));
        assert!(!s_r_collide("/b<a>/there", "/hi/there"));
        assert!(!s_r_collide("/<a>/<b>c", "/hi/person"));
        assert!(!s_r_collide("/<a>/<b>cd", "/hi/<a>e"));
        assert!(!s_r_collide("/a<a>/<b>", "/b<b>/<a>"));
        assert!(!s_r_collide("/a/<b>", "/b/<b>"));
        assert!(!s_r_collide("/a<a>/<b>", "/b/<b>"));
        assert!(!r_s_collide("/a", "/b"));
        assert!(!r_s_collide("/a/b", "/a"));
        assert!(!r_s_collide("/a/b", "/a/c"));
        assert!(!r_s_collide("/a/hello", "/a/c"));
        assert!(!r_s_collide("/hello", "/a/c"));
        assert!(!r_s_collide("/hello/there", "/hello/there/guy"));
        assert!(!r_s_collide("/b<a>/there", "/hi/there"));
        assert!(!r_s_collide("/<a>/<b>c", "/hi/person"));
        assert!(!r_s_collide("/<a>/<b>cd", "/hi/<a>e"));
        assert!(!r_s_collide("/a<a>/<b>", "/b<b>/<a>"));
        assert!(!r_s_collide("/a/<b>", "/b/<b>"));
        assert!(!r_s_collide("/a<a>/<b>", "/b/<b>"));
    }

    #[test]
    fn test_str_collisions() {
        assert!(!s_s_collide("/a", "/b"));
        assert!(!s_s_collide("/a/b", "/a"));
        assert!(!s_s_collide("/a/b", "/a/c"));
        assert!(!s_s_collide("/a/hello", "/a/c"));
        assert!(!s_s_collide("/hello", "/a/c"));
        assert!(!s_s_collide("/hello/there", "/hello/there/guy"));
        assert!(!s_s_collide("/b<a>/there", "/hi/there"));
        assert!(!s_s_collide("/<a>/<b>c", "/hi/person"));
        assert!(!s_s_collide("/<a>/<b>cd", "/hi/<a>e"));
        assert!(!s_s_collide("/a<a>/<b>", "/b<b>/<a>"));
        assert!(!s_s_collide("/a/<b>", "/b/<b>"));
        assert!(!s_s_collide("/a<a>/<b>", "/b/<b>"));
    }
}
