#[macro_use] extern crate rocket;

use rocket::request::Form;

#[derive(FromForm)]
struct Simple {
    value: String
}

#[post("/", data = "<form>")]
fn index(form: Form<Simple>) -> String {
    form.into_inner().value
}

mod limits_tests {
    use rocket;
    use rocket::config::{Environment, Config};
    use rocket::local::blocking::Client;
    use rocket::http::{Status, ContentType};
    use rocket::data::Limits;

    fn rocket_with_forms_limit(limit: u64) -> rocket::Rocket {
        let config = Config::build(Environment::Development)
            .limits(Limits::default().limit("forms", limit.into()))
            .unwrap();

        rocket::custom(config).mount("/", routes![super::index])
    }

    #[test]
    fn large_enough() {
        let client = Client::tracked(rocket_with_forms_limit(128)).unwrap();
        let response = client.post("/")
            .body("value=Hello+world")
            .header(ContentType::Form)
            .dispatch();

        assert_eq!(response.into_string(), Some("Hello world".into()));
    }

    #[test]
    fn just_large_enough() {
        let client = Client::tracked(rocket_with_forms_limit(17)).unwrap();
        let response = client.post("/")
            .body("value=Hello+world")
            .header(ContentType::Form)
            .dispatch();

        assert_eq!(response.into_string(), Some("Hello world".into()));
    }

    #[test]
    fn much_too_small() {
        let client = Client::tracked(rocket_with_forms_limit(4)).unwrap();
        let response = client.post("/")
            .body("value=Hello+world")
            .header(ContentType::Form)
            .dispatch();

        assert_eq!(response.status(), Status::UnprocessableEntity);
    }

    #[test]
    fn contracted() {
        let client = Client::tracked(rocket_with_forms_limit(10)).unwrap();
        let response = client.post("/")
            .body("value=Hello+world")
            .header(ContentType::Form)
            .dispatch();

        assert_eq!(response.into_string(), Some("Hell".into()));
    }
}
