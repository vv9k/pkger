use http::Uri;
use ipnet::{Ipv4Net, Ipv6Net};
use std::net::{self, IpAddr, ToSocketAddrs};
use std::{env, str::FromStr};

pub const HTTPS_PROXY_ENV: &str = "https_proxy";
pub const HTTP_PROXY_ENV: &str = "http_proxy";
pub const NO_PROXY_ENV: &str = "no_proxy";

#[derive(Clone, Debug, PartialEq)]
pub enum NoProxyOption {
    IpAddr(IpAddr),
    Ipv4Net(Ipv4Net),
    Ipv6Net(Ipv6Net),
    Domain(String),
    WildcardDomain(String),
}

impl FromStr for NoProxyOption {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(anyhow!("Invalid address"));
        }
        if let Ok(addr) = s.parse::<Ipv4Net>() {
            return Ok(Self::Ipv4Net(addr));
        }
        if let Ok(addr) = s.parse::<Ipv6Net>() {
            return Ok(Self::Ipv6Net(addr));
        }
        if let Ok(addr) = s.parse::<IpAddr>() {
            return Ok(Self::IpAddr(addr));
        }
        if s.starts_with('.') || s.starts_with('*') {
            return Ok(Self::WildcardDomain(s.into()));
        }

        if !s.contains('.') {
            return Err(anyhow!("Invalid address"));
        }

        Ok(Self::Domain(s.into()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShouldProxyResult {
    Http,
    Https,
    No,
}

#[derive(Clone, Default, Debug)]
pub struct ProxyConfig {
    https_proxy: Option<Uri>,
    http_proxy: Option<Uri>,
    no_proxy: Vec<NoProxyOption>,
}

impl ProxyConfig {
    pub fn from_env() -> Self {
        let https_proxy = env::var(HTTPS_PROXY_ENV)
            .ok()
            .or_else(|| env::var(HTTPS_PROXY_ENV.to_ascii_uppercase()).ok());
        let http_proxy = env::var(HTTP_PROXY_ENV)
            .ok()
            .or_else(|| env::var(HTTP_PROXY_ENV.to_ascii_uppercase()).ok());
        let mut no_proxy = vec![];
        for addr in env::var(NO_PROXY_ENV)
            .ok()
            .or_else(|| env::var(NO_PROXY_ENV.to_ascii_uppercase()).ok())
            .unwrap_or_default()
            .split(',')
        {
            if let Ok(addr) = addr.parse::<NoProxyOption>() {
                no_proxy.push(addr);
            }
        }

        ProxyConfig {
            https_proxy: https_proxy.and_then(|addr| addr.parse().ok()),
            http_proxy: http_proxy.and_then(|addr| addr.parse().ok()),
            no_proxy,
        }
    }

    pub fn set_https_proxy(&mut self, uri: Uri) {
        self.https_proxy = Some(uri);
    }

    pub fn set_http_proxy(&mut self, uri: Uri) {
        self.http_proxy = Some(uri);
    }

    pub fn set_no_proxy(&mut self, no_proxy: impl IntoIterator<Item = NoProxyOption>) {
        self.no_proxy = no_proxy.into_iter().collect();
    }

    pub fn http_proxy(&self) -> Option<&Uri> {
        self.http_proxy.as_ref()
    }

    pub fn https_proxy(&self) -> Option<&Uri> {
        self.https_proxy.as_ref()
    }

    pub fn no_proxy(&self) -> &[NoProxyOption] {
        &self.no_proxy[..]
    }

    pub fn is_proxy_set(&self) -> bool {
        self.http_proxy.is_some() || self.https_proxy.is_some()
    }

    pub fn should_proxy(&self, uri: impl TryInto<Uri>) -> ShouldProxyResult {
        let uri = if let Ok(uri) = uri.try_into() {
            uri
        } else {
            return ShouldProxyResult::No;
        };
        let mut should_proxy = ShouldProxyResult::No;

        match uri.scheme_str() {
            Some("https") if self.https_proxy.is_some() => should_proxy = ShouldProxyResult::Https,
            Some("http") if self.http_proxy.is_some() => should_proxy = ShouldProxyResult::Http,
            _ => {}
        }

        match uri.port_u16() {
            Some(443) if self.https_proxy.is_some() => should_proxy = ShouldProxyResult::Https,
            Some(80) if self.http_proxy.is_some() => should_proxy = ShouldProxyResult::Http,
            _ => {}
        }

        let host = match uri.host() {
            Some(host) => host,
            None => return ShouldProxyResult::No,
        };

        let res = host.parse::<net::IpAddr>();
        let is_ip = res.is_ok();
        let addr = if is_ip {
            Some(res.unwrap())
        } else if let Some(addr) = host
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
        {
            match addr.port() {
                443 if self.https_proxy.is_some() => should_proxy = ShouldProxyResult::Https,
                80 if self.http_proxy.is_some() => should_proxy = ShouldProxyResult::Http,
                _ => {}
            }

            Some(addr.ip())
        } else {
            None
        };

        for opt in &self.no_proxy {
            match addr {
                Some(IpAddr::V4(addr)) => match opt {
                    NoProxyOption::Ipv4Net(net) => {
                        if net.contains(&addr) {
                            should_proxy = ShouldProxyResult::No;
                            break;
                        }
                    }
                    NoProxyOption::IpAddr(IpAddr::V4(noproxy_addr)) => {
                        if noproxy_addr == &addr {
                            should_proxy = ShouldProxyResult::No;
                            break;
                        }
                    }
                    _ => {}
                },
                Some(IpAddr::V6(addr)) => match opt {
                    NoProxyOption::Ipv6Net(net) => {
                        if net.contains(&addr) {
                            should_proxy = ShouldProxyResult::No;
                            break;
                        }
                    }
                    NoProxyOption::IpAddr(IpAddr::V6(noproxy_addr)) => {
                        if noproxy_addr == &addr {
                            should_proxy = ShouldProxyResult::No;
                            break;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }

            match opt {
                NoProxyOption::Domain(domain) if !is_ip => {
                    if domain == host {
                        should_proxy = ShouldProxyResult::No;
                        break;
                    }
                }
                NoProxyOption::WildcardDomain(domain) if !is_ip => {
                    let domain = domain.trim_start_matches('*').trim_start_matches('.');
                    if host.contains(domain) {
                        should_proxy = ShouldProxyResult::No;
                        break;
                    }
                }
                _ => {}
            }
        }

        should_proxy
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::env;

    macro_rules! cleanup {
        () => {
            env::remove_var(HTTPS_PROXY_ENV);
            env::remove_var(HTTP_PROXY_ENV);
            env::remove_var(NO_PROXY_ENV);
            env::remove_var(HTTPS_PROXY_ENV.to_ascii_uppercase());
            env::remove_var(HTTP_PROXY_ENV.to_ascii_uppercase());
            env::remove_var(NO_PROXY_ENV.to_ascii_uppercase());
        };
    }

    #[test]
    fn parses_proxy_from_env() {
        cleanup!();
        env::set_var(HTTPS_PROXY_ENV, "http://proxy.test.com:80");
        env::set_var(NO_PROXY_ENV, "10.0.0.0/8,.test.com");

        let config = ProxyConfig::from_env();
        assert_eq!(config.http_proxy(), None);
        assert_eq!(
            config.https_proxy(),
            Some(&"http://proxy.test.com:80".parse().unwrap())
        );
        assert_eq!(
            config.no_proxy(),
            &[
                NoProxyOption::Ipv4Net("10.0.0.0/8".parse().unwrap()),
                NoProxyOption::WildcardDomain(".test.com".into()),
            ]
        );

        env::remove_var(HTTPS_PROXY_ENV);
        env::remove_var(NO_PROXY_ENV);
        env::set_var(
            HTTPS_PROXY_ENV.to_ascii_uppercase(),
            "http://proxy.test.com:80",
        );
        env::set_var(
            HTTP_PROXY_ENV.to_ascii_uppercase(),
            "http://proxy.test.com:80",
        );
        env::set_var(
            NO_PROXY_ENV.to_ascii_uppercase(),
            "10.0.0.1,*.test.com,test.com",
        );

        let config = ProxyConfig::from_env();
        assert_eq!(
            config.https_proxy(),
            Some(&"http://proxy.test.com:80".parse().unwrap())
        );
        assert_eq!(
            config.https_proxy(),
            Some(&"http://proxy.test.com:80".parse().unwrap())
        );
        assert_eq!(
            config.no_proxy(),
            &[
                NoProxyOption::IpAddr("10.0.0.1".parse().unwrap()),
                NoProxyOption::WildcardDomain("*.test.com".into()),
                NoProxyOption::Domain("test.com".into()),
            ]
        );
        cleanup!();
    }

    #[test]
    fn should_proxy() {
        cleanup!();
        env::set_var(
            HTTPS_PROXY_ENV.to_ascii_uppercase(),
            "http://proxy.test.com:80",
        );
        env::set_var(NO_PROXY_ENV.to_ascii_uppercase(), "10.0.0.0/8,.test.com");
        let config = ProxyConfig::from_env();

        assert_eq!(ShouldProxyResult::No, config.should_proxy("10.0.0.1:443"));
        assert_eq!(
            ShouldProxyResult::Https,
            config.should_proxy("16.9.9.1:443")
        );
        assert_eq!(
            ShouldProxyResult::Https,
            config.should_proxy("https://16.9.9.1/test")
        );
        assert_eq!(ShouldProxyResult::No, config.should_proxy("16.9.9.1:80"));
        assert_eq!(
            ShouldProxyResult::No,
            config.should_proxy("http://16.9.9.1")
        );
        assert_eq!(
            ShouldProxyResult::No,
            config.should_proxy("https://test.com")
        );
        assert_eq!(
            ShouldProxyResult::No,
            config.should_proxy("https://some.test.com")
        );
        assert_eq!(
            ShouldProxyResult::No,
            config.should_proxy("https://some.more.test.com")
        );
        assert_eq!(
            ShouldProxyResult::Https,
            config.should_proxy("https://other.com")
        );
        assert_eq!(
            ShouldProxyResult::Https,
            config.should_proxy("https://some.other.com")
        );
        assert_eq!(
            ShouldProxyResult::Https,
            config.should_proxy("https://some.more.other.com")
        );

        env::set_var(
            HTTP_PROXY_ENV.to_ascii_uppercase(),
            "http://proxy.test.com:80",
        );
        env::set_var(NO_PROXY_ENV.to_ascii_uppercase(), "*.some.test.com");
        let config = ProxyConfig::from_env();
        assert_eq!(
            ShouldProxyResult::Http,
            config.should_proxy("http://some.more.other.com")
        );
        assert_eq!(
            ShouldProxyResult::Http,
            config.should_proxy("http://16.9.9.1")
        );
        assert_eq!(
            ShouldProxyResult::Http,
            config.should_proxy("http://test.com")
        );
        assert_eq!(
            ShouldProxyResult::No,
            config.should_proxy("http://some.test.com")
        );
        assert_eq!(
            ShouldProxyResult::No,
            config.should_proxy("http://more.some.test.com")
        );
        assert_eq!(
            ShouldProxyResult::Http,
            config.should_proxy("http://other.com")
        );
        assert_eq!(
            ShouldProxyResult::Http,
            config.should_proxy("http://some.other.com")
        );
        assert_eq!(
            ShouldProxyResult::Http,
            config.should_proxy("http://some.more.other.com")
        );
        cleanup!();
    }
}
