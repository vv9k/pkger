#![allow(dead_code)]
use crate::Result;
use anyhow::anyhow;

use std::iter::IntoIterator;

#[derive(Debug, PartialEq)]
pub struct Dependency {
    // fallback global name
    name: Option<String>,
    // each value corresponds to (image name, dependency name)
    names: Vec<(String, String)>,
}

impl Dependency {
    pub fn new(name: &str, names: Vec<(String, String)>) -> Self {
        Self {
            name: Some(name.to_string()),
            names,
        }
    }

    pub fn parse(dep: &str) -> Result<Self> {
        let mut names = vec![];
        let mut name = None;

        let elems = dep.split(',').map(str::trim).collect::<Vec<_>>();
        if elems.is_empty() {
            return Ok(Self::new(dep, names));
        }

        for elem in elems {
            if let Some(idx) = elem.find('{') {
                names.push(Self::parse_with_image(elem, idx)?);
            } else {
                if name.is_none() {
                    name = Some(elem.to_string());
                } else {
                    return Err(anyhow!("double global name in dependency `{}`", elem));
                }
            }
        }

        Ok(Self { name, names })
    }

    fn parse_with_image(elem: &str, idx: usize) -> Result<(String, String)> {
        let name = elem[..idx].to_string();
        let _elem = &elem[idx + 1..];
        if let Some(end_idx) = _elem.find('}') {
            return Ok((name, _elem[..end_idx].to_string()));
        }

        Err(anyhow!(
            "missing closing bracket `}}` from dependency `{}`",
            _elem
        ))
    }
}

pub struct Dependencies {
    inner: Vec<Dependency>,
}

impl Dependencies {
    pub fn new<I, S>(deps: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut inner = vec![];
        for dependency in deps.into_iter().map(|s| Dependency::parse(&s.into())) {
            inner.push(dependency?);
        }

        Ok(Self { inner })
    }

    pub fn as_ref(&self) -> &[Dependency] {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dependencies() {
        let expect = Dependency::new(
            "openssl-devel",
            vec![
                ("centos8".to_string(), "libssl-dev".to_string()),
                ("debian10".to_string(), "openssl-devel".to_string()),
            ],
        );
        let out = Dependency::parse("centos8{libssl-dev},openssl-devel, debian10{openssl-devel}")
            .unwrap();
        assert_eq!(expect, out);
    }
}
