#![feature(plugin, decl_macro, custom_derive)]
#![plugin(rocket_codegen)]
#![allow(dead_code, unused_variables)]

extern crate rocket;

use std::fmt;

use rocket::http::{RawStr, Cookies};
use rocket::http::uri::{Uri, UriDisplay};
use rocket::request::Form;

#[derive(FromForm)]
struct User<'a> {
    name: &'a RawStr,
    nickname: String,
}

// TODO: Make this deriveable.
impl<'a> UriDisplay for User<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "name={}&nickname={}",
               &self.name.replace(' ', "+") as &UriDisplay,
               &self.nickname.replace(' ', "+") as &UriDisplay)
    }
}

impl<'a, 'b> From<(&'a str, &'b str)> for User<'a> {
    fn from((name, nickname): (&'a str, &'b str)) -> User<'a> {
        User { name: name.into(), nickname: nickname.to_string() }
    }
}

// This one has no `UriDisplay`. It exists to ensure that this file still
// compiles even though it's used a URI parameter's type. As long as a user
// doesn't request a URI from that route, things should be okay.
#[derive(FromForm)]
struct Second {
    nickname: String,
}

#[post("/<id>")]
fn simple(id: i32) -> &'static str { "" }

#[post("/<id>/<name>")]
fn simple2(id: i32, name: String) -> &'static str { "" }

#[post("/<id>/<name>")]
fn simple2_flipped(name: String, id: i32) -> &'static str { "" }

#[post("/<id>")]
fn guard_1(cookies: Cookies, id: i32) -> &'static str { "" }

#[post("/<id>/<name>")]
fn guard_2(name: String, cookies: Cookies, id: i32) -> &'static str { "" }

#[post("/a/<id>/hi/<name>/hey")]
fn guard_3(id: i32, name: String, cookies: Cookies) -> &'static str { "" }

#[post("/<id>", data = "<form>")]
fn no_uri_display_okay(id: i32, form: Form<Second>) -> &'static str {
    "Typed URI testing."
}

#[post("/<name>?<query>", data = "<user>", rank = 2)]
fn complex<'r>(
    name: &RawStr,
    query: User<'r>,
    user: Form<'r, User<'r>>,
    cookies: Cookies
) -> &'static str { "" }

macro assert_uri_eq($($uri:expr => $expected:expr,)+) {
    $(assert_eq!($uri, Uri::from($expected));)+
}

#[test]
fn check_simple_unnamed() {
    assert_uri_eq! {
        uri!(simple: 100) => "/100",
        uri!(simple: -23) => "/-23",
    }

    // The "flipped" test ensures that the order of parameters depends on the
    // route's URI, not on the order in the function signature.
    assert_uri_eq! {
        uri!(simple2: 100, "hello".to_string()) => "/100/hello",
        uri!(simple2: 1349, "hey".to_string()) => "/1349/hey",
        uri!(simple2_flipped: 100, "hello".to_string()) => "/100/hello",
    }

    // Ensure that `.into()` is called.
    assert_uri_eq! {
        uri!(simple2: 100i8, "hello") => "/100/hello",
        uri!(simple2: 100i16, "hi") => "/100/hi",
        uri!(simple2: 100, "hello") => "/100/hello",
        uri!(simple2_flipped: 1349, "hey") => "/1349/hey",
    }

    // Ensure that the `UriDisplay` trait is being used.
    assert_uri_eq! {
        uri!(simple2: 100i8, "hello there") => "/100/hello%20there",
        uri!(simple2_flipped: 100i8, "hello there") => "/100/hello%20there",
    }
}

#[test]
fn check_simple_named() {
    assert_uri_eq! {
        uri!(simple: id = 100) => "/100",
        uri!(simple: id = -23) => "/-23",
    }

    assert_uri_eq! {
        uri!(simple2: id = 100, name = "hello".to_string()) => "/100/hello",
        uri!(simple2: name = "hi".to_string(), id = 123) => "/123/hi",
        uri!(simple2_flipped: id = 1349, name = "hey".to_string()) => "/1349/hey",
        uri!(simple2_flipped: name = "hello".to_string(), id = 100) => "/100/hello",
    }

    // Ensure that `.into()` is called.
    assert_uri_eq! {
        uri!(simple2: id = 100i8, name = "hello") => "/100/hello",
        uri!(simple2: id = 100i16, name = "hi") => "/100/hi",
        uri!(simple2: id = 1349, name = "hey") => "/1349/hey",
        uri!(simple2: name = "hello", id = 100i8) => "/100/hello",
        uri!(simple2: name = "hi", id = 100i16) => "/100/hi",
        uri!(simple2: name = "hey", id = 1349) => "/1349/hey",
        uri!(simple2_flipped: id = 1349, name = "hey") => "/1349/hey",
        uri!(simple2_flipped: name = "hello", id = 100i8) => "/100/hello",
    }

    // Ensure that the `UriDisplay` trait is being used.
    assert_uri_eq! {
        uri!(simple2: id = 100i8, name = "hello there") => "/100/hello%20there",
        uri!(simple2: name = "hello there", id = 100i8) => "/100/hello%20there",
        uri!(simple2_flipped: id = 100i8, name = "hello there") => "/100/hello%20there",
        uri!(simple2_flipped: name = "hello there", id = 100i8) => "/100/hello%20there",
    }
}

#[test]
fn check_mount_point() {
    assert_uri_eq! {
        uri!("/mount", simple: 100) => "/mount/100",
        uri!("/mount", simple: id = 23) => "/mount/23",
        uri!("/another", simple: 100) => "/another/100",
        uri!("/another", simple: id = 23) => "/another/23",
    }

    assert_uri_eq! {
        uri!("/a", simple2: 100, "hey") => "/a/100/hey",
        uri!("/b", simple2: id = 23, name = "hey") => "/b/23/hey",
    }
}

#[test]
fn check_guards_ignored() {
    assert_uri_eq! {
        uri!("/mount", guard_1: 100) => "/mount/100",
        uri!("/mount", guard_2: 2938, "boo") => "/mount/2938/boo",
        uri!("/mount", guard_3: 340, "Bob") => "/mount/a/340/hi/Bob/hey",
        uri!(guard_1: 100) => "/100",
        uri!(guard_2: 2938, "boo") => "/2938/boo",
        uri!(guard_3: 340, "Bob") => "/a/340/hi/Bob/hey",
        uri!("/mount", guard_1: id = 100) => "/mount/100",
        uri!("/mount", guard_2: id = 2938, name = "boo") => "/mount/2938/boo",
        uri!("/mount", guard_3: id = 340, name = "Bob") => "/mount/a/340/hi/Bob/hey",
        uri!(guard_1: id = 100) => "/100",
        uri!(guard_2: name = "boo", id = 2938) => "/2938/boo",
        uri!(guard_3: name = "Bob", id = 340) => "/a/340/hi/Bob/hey",
    }
}

#[test]
fn check_complex() {
    assert_uri_eq! {
        uri!(complex: "no idea", ("A B C", "a c")) => "/no%20idea?name=A+B+C&nickname=a+c",
        uri!(complex: "Bob", User { name: "Robert".into(), nickname: "Bob".into() })
            => "/Bob?name=Robert&nickname=Bob",
        uri!(complex: "no idea", User { name: "Robert Mike".into(), nickname: "Bob".into() })
            => "/no%20idea?name=Robert+Mike&nickname=Bob",
        uri!("/some/path", complex: "no idea", ("A B C", "a c"))
            => "/some/path/no%20idea?name=A+B+C&nickname=a+c",
        uri!(complex: name = "Bob", query = User { name: "Robert".into(), nickname: "Bob".into() })
            => "/Bob?name=Robert&nickname=Bob",
        uri!(complex: query = User { name: "Robert".into(), nickname: "Bob".into() }, name = "Bob")
            => "/Bob?name=Robert&nickname=Bob",
        uri!(complex: name = "no idea", query = ("A B C", "a c"))
            => "/no%20idea?name=A+B+C&nickname=a+c",
        uri!(complex: query = ("A B C", "a c"), name = "no idea")
            => "/no%20idea?name=A+B+C&nickname=a+c",
        uri!("/hey", complex: name = "no idea", query = ("A B C", "a c"))
            => "/hey/no%20idea?name=A+B+C&nickname=a+c",
    }
}

#[test]
fn check_scoped() {
    assert_uri_eq!{
        uri!(typed_uris::simple: 100) => "/typed_uris/100",
        uri!(typed_uris::simple: id = 100) => "/typed_uris/100",
        uri!(typed_uris::deeper::simple: 100) => "/typed_uris/deeper/100",
    }
}

mod typed_uris {
    use super::assert_uri_eq;

    #[post("/typed_uris/<id>")]
    fn simple(id: i32) -> &'static str { "" }

    #[test]
    fn check_simple_scoped() {
        assert_uri_eq! {
            uri!(simple: id = 100) => "/typed_uris/100",
            uri!(::simple: id = 100) => "/100",
            uri!("/mount", ::simple: id = 100) => "/mount/100",
            uri!(::simple2: id = 100i8, name = "hello") => "/100/hello",
        }
    }

    pub mod deeper {
        use super::assert_uri_eq;

        #[post("/typed_uris/deeper/<id>")]
        fn simple(id: i32) -> &'static str { "" }

        #[test]
        fn check_deep_scoped() {
            assert_uri_eq! {
                uri!(super::simple: id = 100) => "/typed_uris/100",
                uri!(::simple: id = 100) => "/100",
            }
        }
    }
}
