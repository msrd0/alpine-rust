use serde::{
	de::{self, Visitor},
	Deserialize, Deserializer, Serialize, Serializer
};
use std::fmt::{self, Display};

pub mod docker_keys;
pub mod upcloud;

#[derive(Clone, Copy, Debug)]
pub struct IPv6CIDR<P> {
	prefix: P,
	netmask: u8
}

impl<P> IPv6CIDR<P> {
	const fn new(prefix: P, netmask: u8) -> Self {
		Self { prefix, netmask }
	}
}

impl<P: Display> IPv6CIDR<P> {
	pub fn first_ip<'a>(&'a self) -> impl Display + 'a {
		struct FirstIp<'p, P>(&'p P);

		impl<'p, P: Display> Display for FirstIp<'p, P> {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				write!(f, "{}1", self.0)
			}
		}

		FirstIp(&self.prefix)
	}
}

// TODO convert this to a proper ToOwned implementation
impl IPv6CIDR<&str> {
	pub fn to_owned(&self) -> IPv6CIDR<String> {
		IPv6CIDR::new(self.prefix.to_owned(), self.netmask)
	}
}

impl<P: Display> Serialize for IPv6CIDR<P> {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer
	{
		serializer.serialize_str(&format!("{}/{}", self.prefix, self.netmask))
	}
}

struct IPv6CIDRVisitor;

impl<'de> Visitor<'de> for IPv6CIDRVisitor {
	type Value = IPv6CIDR<String>;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("an IPv6 CIDR, e.g. 'fd00::dead::beef::/48'")
	}

	fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
	where
		E: de::Error
	{
		// Only checking for "::/" does not guarantee a correct IPv6 CIDR but should be good
		// enough for our use case.
		let index = match v.find("::/") {
			Some(index) => index,
			None => return Err(E::invalid_value(de::Unexpected::Str(v), &self))
		};
		let prefix = &v[0..index + 2];
		let netmask = match v[index + 3..].parse() {
			Ok(netmask) => netmask,
			Err(_) => return Err(E::invalid_value(de::Unexpected::Str(v), &self))
		};
		Ok(Self::Value {
			prefix: prefix.to_owned(),
			netmask
		})
	}
}

impl<'de> Deserialize<'de> for IPv6CIDR<String> {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>
	{
		deserializer.deserialize_str(IPv6CIDRVisitor)
	}
}
