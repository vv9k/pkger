#[macro_use]
extern crate anyhow;

#[macro_use]
extern crate lazy_static;

pub mod archive;
pub mod build;
pub mod gpg;
pub mod image;
#[macro_export]
pub mod log;
pub mod oneshot;
pub mod proxy;
pub mod recipe;
pub mod runtime;
pub mod ssh;
pub mod template;

pub use anyhow::{anyhow, Context as ErrContext, Error, Result};

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn unix_timestamp() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

#[macro_export]
macro_rules! err {
    ($it:ident) => {
       Err(Error::msg($it))
    };
    ($lit:literal) => {
        Err(Error::msg($lit))
    };
    ($($tt:tt)*) => {
        Err(Error::msg(format!($($tt)*)))
    };
}
