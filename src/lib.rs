pub mod raptor;
pub mod spatial;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("No route was found")]
    NoRoute,
}
