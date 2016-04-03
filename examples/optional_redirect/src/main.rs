#![feature(plugin)]
#![plugin(rocket_macros)]
extern crate rocket;

use rocket::Rocket;
use rocket::response::Redirect;

#[route(GET, path = "/")]
fn root() -> Redirect {
    Redirect::to("/users/login")
}

#[route(GET, path = "/users/<name>")]
fn user(name: &str) -> Result<&'static str, Redirect> {
    match name {
        "Sergio" => Ok("Hello, Sergio!"),
        _ => Err(Redirect::to("/users/login"))
    }
}

#[route(GET, path = "/users/login")]
fn login() -> &'static str {
    "Hi! That user doesn't exist. Maybe you need to log in?"
}

fn main() {
    let rocket = Rocket::new("localhost", 8000);
    rocket.mount_and_launch("/", routes![root, user, login]);
}
