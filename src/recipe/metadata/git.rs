use crate::{Error, Result};

use std::convert::TryFrom;

#[derive(Clone, Debug)]
pub struct GitSource {
    url: String,
    // defaults to master
    branch: String,
}

impl From<&str> for GitSource {
    fn from(s: &str) -> Self {
        Self {
            url: s.to_string(),
            branch: "master".to_string(),
        }
    }
}

impl TryFrom<toml::value::Table> for GitSource {
    type Error = Error;
    fn try_from(table: toml::value::Table) -> Result<Self> {
        if let Some(url) = table.get("url") {
            if !url.is_str() {
                return Err(anyhow!("expected a string as url, found `{:?}`", url));
            }

            let url = url.as_str().unwrap().to_string();

            if let Some(branch) = table.get("branch") {
                if !branch.is_str() {
                    return Err(anyhow!("expected a string as branch, found `{:?}`", branch));
                }

                return Ok(GitSource::new(
                    url,
                    Some(branch.as_str().unwrap().to_string()),
                ));
            }

            Ok(GitSource::new(url, None::<&str>))
        } else {
            Err(anyhow!(
                "expected a url entry in a table, found `{:?}`",
                table
            ))
        }
    }
}

impl TryFrom<toml::Value> for GitSource {
    type Error = Error;
    fn try_from(value: toml::Value) -> Result<Self> {
        match value {
            toml::Value::Table(table) => Self::try_from(table),
            toml::Value::String(s) => Ok(Self::from(s.as_str())),
            value => Err(anyhow!(
                "expected a table or a string as git source, found `{:?}`",
                value
            )),
        }
    }
}
impl GitSource {
    pub fn new<U, B>(url: U, branch: Option<B>) -> Self
    where
        U: Into<String>,
        B: Into<String>,
    {
        Self {
            url: url.into(),
            branch: branch.map(B::into).unwrap_or_else(|| "master".to_string()),
        }
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn branch(&self) -> &str {
        &self.branch
    }
}
