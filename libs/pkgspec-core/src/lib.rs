use std::path::Path;
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    WriteError(#[from] std::io::Error),
    #[error(transparent)]
    FormatError(#[from] std::fmt::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Manifest {
    fn save_to(&self, path: impl AsRef<Path>) -> Result<()>;
    fn render(&self) -> Result<String>;
}
