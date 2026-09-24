use std::net::IpAddr;
use std::sync::{Arc, RwLock};

pub const TRUSTED_PROXIES_KEY: &str = "rate_limit.trusted_proxy_ips";

#[derive(Clone, Copy, Debug)]
pub struct IpMatcher {
    address: IpAddr,
    prefix: Option<u8>,
}

impl IpMatcher {
    pub fn parse(value: &str) -> Option<Self> {
        let (address, prefix) = match value.trim().split_once('/') {
            Some((address, prefix)) => (
                address.trim().parse::<IpAddr>().ok()?,
                Some(prefix.trim().parse::<u8>().ok()?),
            ),
            None => (value.trim().parse::<IpAddr>().ok()?, None),
        };
        if prefix.is_some_and(|prefix| prefix > if address.is_ipv4() { 32 } else { 128 }) {
            return None;
        }
        Some(Self { address, prefix })
    }

    pub fn matches(self, address: IpAddr) -> bool {
        let address = address.to_canonical();
        let Some(prefix) = self.prefix else {
            return self.address.to_canonical() == address;
        };
        match (self.address, address) {
            (IpAddr::V4(base), IpAddr::V4(address)) => {
                let mask = u32::MAX.checked_shl(u32::from(32 - prefix)).unwrap_or(0);
                u32::from(base) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(base), IpAddr::V6(address)) => {
                let mask = u128::MAX.checked_shl(u32::from(128 - prefix)).unwrap_or(0);
                u128::from(base) & mask == u128::from(address) & mask
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TrustedProxyPolicy {
    pub addresses: Vec<String>,
    pub override_addresses: Option<Vec<String>>,
    pub environment_addresses: Vec<String>,
    matchers: Vec<IpMatcher>,
}

impl TrustedProxyPolicy {
    pub fn new(environment: Vec<String>, saved: Option<Vec<String>>) -> Result<Self, String> {
        let addresses = saved.as_ref().unwrap_or(&environment).clone();
        let matchers = addresses
            .iter()
            .map(|value| {
                IpMatcher::parse(value).ok_or_else(|| {
                    "trusted proxies must contain only IPv4, IPv6, or CIDR addresses".to_string()
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            addresses,
            matchers,
            override_addresses: saved,
            environment_addresses: environment,
        })
    }

    pub fn matches(&self, address: IpAddr) -> bool {
        self.matchers.iter().any(|matcher| matcher.matches(address))
    }
}

#[derive(Clone, Default)]
pub struct TrustedProxyRuntime(Arc<RwLock<Arc<TrustedProxyPolicy>>>);

impl TrustedProxyRuntime {
    pub fn new(policy: TrustedProxyPolicy) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(policy))))
    }

    pub fn snapshot(&self) -> Arc<TrustedProxyPolicy> {
        self.0
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(crate) fn replace(&self, policy: TrustedProxyPolicy) {
        *self.0.write().unwrap_or_else(|error| error.into_inner()) = Arc::new(policy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_empty_overrides_environment_and_reset_restores_it() {
        let env = vec!["127.0.0.1".into()];
        assert!(
            !TrustedProxyPolicy::new(env.clone(), Some(vec![]))
                .unwrap()
                .matches("127.0.0.1".parse().unwrap())
        );
        assert!(
            TrustedProxyPolicy::new(env, None)
                .unwrap()
                .matches("127.0.0.1".parse().unwrap())
        );
    }

    #[test]
    fn requests_keep_one_snapshot_during_policy_replacement() {
        let runtime = TrustedProxyRuntime::default();
        runtime.replace(TrustedProxyPolicy::new(vec!["127.0.0.1".into()], None).unwrap());
        let request = runtime.snapshot();
        runtime.replace(TrustedProxyPolicy::default());
        assert!(request.matches("127.0.0.1".parse().unwrap()));
        assert!(!runtime.snapshot().matches("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn validates_addresses_and_prefixes() {
        for value in [
            "127.0.0.1",
            "::1",
            "192.0.2.0/24",
            "2001:db8::/32",
            "0.0.0.0/0",
            "::/0",
        ] {
            assert!(IpMatcher::parse(value).is_some());
        }
        for value in ["", "example.invalid", "127.0.0.1/33", "::1/129", "::1/-1"] {
            assert!(IpMatcher::parse(value).is_none());
        }
        assert!(
            IpMatcher::parse("192.0.2.0/24")
                .unwrap()
                .matches("::ffff:192.0.2.1".parse().unwrap())
        );
        assert!(
            !IpMatcher::parse("192.0.2.0/24")
                .unwrap()
                .matches("192.0.3.1".parse().unwrap())
        );
        assert!(
            IpMatcher::parse("2001:db8::/32")
                .unwrap()
                .matches("2001:db8::1".parse().unwrap())
        );
    }
}
