use rocket::config::{Config, LogLevel};

async fn test_config(profile: &str) {
    let mut rocket = rocket::custom(Config::figment().select(profile));
    let config = rocket.config().await;
    match &*profile {
        "debug" => {
            assert_eq!(config.address, std::net::Ipv4Addr::LOCALHOST);
            assert_eq!(config.port, 8000);
            assert_eq!(config.workers, 1);
            assert_eq!(config.keep_alive, 0);
            assert_eq!(config.log_level, LogLevel::Normal);
        }
        "release" => {
            assert_eq!(config.address, std::net::Ipv4Addr::LOCALHOST);
            assert_eq!(config.port, 8000);
            assert_eq!(config.workers, 12);
            assert_eq!(config.keep_alive, 5);
            assert_eq!(config.log_level, LogLevel::Critical);
            assert!(!config.secret_key.is_zero());
        }
        _ => {
            panic!("Unknown profile: {}", profile);
        }
    }
}

#[rocket::async_test]
async fn test_debug_config() {
    test_config("debug").await
}

#[rocket::async_test]
async fn test_release_config() {
    test_config("release").await
}
