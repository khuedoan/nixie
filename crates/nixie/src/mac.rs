//! MAC address type that formats like Go's `net.HardwareAddr`.

use std::fmt;
use std::str::FromStr;

/// An Ethernet MAC address, rendered lowercase and colon-separated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddr([u8; 6]);

impl MacAddr {
    pub fn new(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }

    pub fn octets(&self) -> [u8; 6] {
        self.0
    }
}

impl FromStr for MacAddr {
    type Err = macaddr::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed = macaddr::MacAddr6::from_str(value)?;
        let mut octets = [0u8; 6];
        octets.copy_from_slice(parsed.as_bytes());
        Ok(Self(octets))
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}
