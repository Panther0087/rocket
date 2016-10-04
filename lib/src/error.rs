#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Error {
    BadMethod,
    BadParse,
    NoRoute, // TODO: Add a chain of routes attempted.
    Internal,
    NoKey,
}
