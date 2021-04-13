#![allow(dead_code)]
use crate::Result;
use anyhow::anyhow;

use std::iter::IntoIterator;

#[derive(Clone, Debug, PartialEq)]
/// Represents a dependency in distribution agnostic way, used for a recipe.
pub struct Dependency {
    /// fallback global name
    name: Option<String>,
    /// each value corresponds to (image name, dependency name)
    names: Vec<(String, String)>,
}

impl Dependency {
    pub fn new(name: &str, names: Vec<(String, String)>) -> Self {
        Self {
            name: Some(name.to_string()),
            names,
        }
    }

    /// Returns this dependency name for the corresponding `image` if it is available
    pub fn get_name<'d>(&'d self, image: &str) -> Option<&'d str> {
        self.names
            .iter()
            .find(|it| it.0 == image)
            .map(|it| it.1.as_str())
    }

    /// Parses a `Dependency` from a string like `image1{libexmpl-dev},exmpl-devel' where
    /// `exmpl-devel` will become the fallback global `name` field of the dependency and
    /// `names` field will contain an entry ("image1", "libexmpl-dev").
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
            } else if name.is_none() {
                name = Some(elem.to_string());
            } else {
                return Err(anyhow!("double global name in dependency `{}`", elem));
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

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Dependencies {
    inner: Vec<Dependency>,
}

impl Dependencies {
    pub fn new<I>(deps: I) -> Result<Self>
    where
        I: IntoIterator<Item = String>,
    {
        let mut inner = vec![];
        for dependency in deps.into_iter().map(|s| Dependency::parse(&s.as_ref())) {
            inner.push(dependency?);
        }

        Ok(Self { inner })
    }

    pub fn as_ref(&self) -> &[Dependency] {
        &self.inner
    }

    /// Renders a list of names appropriate for specified image
    pub fn resolve_names(&self, image: &str) -> Vec<String> {
        let mut deps = Vec::with_capacity(self.inner.len());

        for dep in self.inner.iter() {
            if let Some(special_name) = dep.get_name(image) {
                deps.push(special_name.to_string());
            } else if let Some(name) = &dep.name {
                deps.push(name.to_string());
            }
        }

        deps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dependency() {
        let expect = Dependency::new(
            "openssl-devel",
            vec![
                ("cent8".to_string(), "libssl-dev".to_string()),
                ("deb10".to_string(), "openssl-devel".to_string()),
            ],
        );
        let out =
            Dependency::parse("cent8{libssl-dev},openssl-devel, deb10{openssl-devel}").unwrap();
        assert_eq!(expect, out);
    }

    #[test]
    fn parses_depencies() {
        let expect = Dependencies {
            inner: vec![
                Dependency::new(
                    "openssl-devel",
                    vec![("cent8".to_string(), "libssl-dev".to_string())],
                ),
                Dependency::new("gcc", vec![]),
                Dependency::new(
                    "libcurl4-openssl-dev",
                    vec![("cent8".to_string(), "libcurl-devel".to_string())],
                ),
            ],
        };
        let input = vec![
            "openssl-devel, cent8{libssl-dev}".to_string(),
            "gcc".to_string(),
            "libcurl4-openssl-dev, cent8{libcurl-devel}".to_string(),
        ];
        let got = Dependencies::new(input).unwrap();
        assert_eq!(expect, got);
    }

    #[test]
    fn resolves_names() {
        let expect = vec![
            "libssl-dev".to_string(),
            "gcc".to_string(),
            "libcurl-devel".to_string(),
        ];
        let input = vec![
            "openssl-devel, cent8{libssl-dev}".to_string(),
            "gcc".to_string(),
            "libcurl4-openssl-dev, cent8{libcurl-devel}".to_string(),
        ];

        let deps = Dependencies::new(input).unwrap();
        let got = deps.resolve_names("cent8");

        assert_eq!(expect, got);
    }
}
