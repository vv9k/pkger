pub mod binary;
pub mod source;

use binary::{BinaryDebControl, BinaryDebControlBuilder};
use source::{SourceDebControl, SourceDebControlBuilder};

pub struct DebControlBuilder {}
impl DebControlBuilder {
    pub fn source_package_builder<S>(name: S) -> SourceDebControlBuilder
    where
        S: Into<String>,
    {
        SourceDebControl::builder().package(name)
    }

    pub fn binary_package_builder<S>(name: S) -> BinaryDebControlBuilder
    where
        S: Into<String>,
    {
        BinaryDebControl::builder().package(name)
    }
}
