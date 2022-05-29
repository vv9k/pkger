#[macro_use]
extern crate anyhow;

#[macro_use]
extern crate lazy_static;

pub mod archive;
pub mod build;
pub mod container;
pub mod docker;
pub mod gpg;
pub mod image;
pub mod oneshot;
#[macro_export]
pub mod output;
pub mod recipe;
pub mod ssh;
pub mod template;

pub use anyhow::{anyhow, Context as ErrContext, Error, Result};

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
