#![allow(dead_code)]
use crate::Result;
use anyhow::anyhow;

use std::collections::HashSet;
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
    pub fn new(name: Option<&str>, names: Vec<(String, String)>) -> Self {
        Self {
            name: name.map(str::to_string),
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
            return Ok(Self::new(Some(dep), names));
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

    /// Renders a HashSet of names appropriate for specified image
    pub fn resolve_names(&self, image: &str) -> HashSet<String> {
        let mut deps = HashSet::with_capacity(self.inner.len());

        for dep in self.inner.iter() {
            if let Some(special_name) = dep.get_name(image) {
                deps.insert(special_name.to_string());
            } else if let Some(name) = &dep.name {
                deps.insert(name.to_string());
            }
        }

        deps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! dep {
        ( $name:expr, $($image:expr => $dep:expr),* ) => {
            Dependency::new(
                $name,
                vec![
                $(
                    ($image.to_string(), $dep.to_string()),
                )*]
            )
        };
    }

    #[test]
    fn parses_dependency() {
        let expect =
            dep!(Some("openssl-devel"), "cent8" => "libssl-dev", "deb10" => "openssl-devel");
        let out =
            Dependency::parse("cent8{libssl-dev},openssl-devel, deb10{openssl-devel}").unwrap();
        assert_eq!(expect, out);
    }

    macro_rules! test_parse_deps {
        (input = $($inp:literal),*
         want = $($deps:expr),* ) => {
            let expect = Dependencies {
                inner: vec![
                    $(
                        $deps
                    ),*
                ]
            };

            let input = vec![
                $(
                    $inp.to_string()
                ),*
            ];

            let got = Dependencies::new(input).unwrap();
            assert_eq!(expect, got);
        };
    }

    #[test]
    fn parses_depencies() {
        test_parse_deps!(
            input =
            want =
        );
        test_parse_deps!(
            input = "debian10{curl}", "wget"
            want = dep!(None, "debian10" => "curl"),
                   dep!(Some("wget"),)
        );
        test_parse_deps!(
            input = "debian10{gcc}, other9{gcc-dev}"
            want = dep!(None, "debian10" => "gcc", "other9" => "gcc-dev")
        );
        test_parse_deps!(
            input = "openssl-devel, cent8{libssl-dev}", "gcc",
                    "libcurl4-openssl-dev, cent8{libcurl-devel}"
            want = dep!(Some("openssl-devel"), "cent8" => "libssl-dev"),
                   dep!(Some("gcc"),),
                   dep!(Some("libcurl4-openssl-dev"), "cent8" => "libcurl-devel")
        );
    }
    macro_rules! test_resolve_names {
        ( image = $image:literal
          input = $($inp:literal),*
          want = $($deps:literal),*
         ) => {
            let mut expect = HashSet::new();
            $(
                expect.insert($deps.to_string());
            )*

            let input = vec![
                $(
                    $inp.to_string()
                ),*
            ];

            let deps = Dependencies::new(input).unwrap();
            let got = deps.resolve_names($image);

            assert_eq!(expect, got);
        };
    }

    #[test]
    fn resolves_names() {
        test_resolve_names!(
            image = "cent8"
            input = "openssl-devel, cent8{libssl-dev}",
                    "gcc",
                    "libcurl4-openssl-dev, cent8{libcurl-devel}"
            want = "libssl-dev", "gcc", "libcurl-devel"
        );
        test_resolve_names!(
            image = "debian10"
            input = "debian10{curl}",
                    "gcc",
                    "libcurl4-openssl-dev, cent8{libcurl-devel}"
            want = "curl", "gcc", "libcurl4-openssl-dev"
        );
    }
}
