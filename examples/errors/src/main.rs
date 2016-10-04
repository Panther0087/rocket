#![feature(plugin)]
#![plugin(rocket_codegen)]

extern crate rocket;
use rocket::{Error, Request};

#[get("/hello/<name>/<age>")]
fn hello(name: &str, age: i8) -> String {
    format!("Hello, {} year old named {}!", age, name)
}

#[error(404)]
fn not_found<'r>(_error: Error, request: &'r Request<'r>) -> String {
    format!("<p>Sorry, but '{}' is not a valid path!</p>
            <p>Try visiting /hello/&lt;name&gt;/&lt;age&gt; instead.</p>",
            request.uri)
}

fn main() {
    let mut rocket = rocket::ignite();
    rocket.mount("/", routes![hello]);
    rocket.catch(errors![not_found]);
    rocket.launch();
}
