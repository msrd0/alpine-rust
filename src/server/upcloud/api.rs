use super::{UPCLOUD_CORES, UPCLOUD_MEMORY, UPCLOUD_STORAGE};
use serde::{Deserialize, Serialize};
use std::{env, time::Duration};
use tokio::time::delay_for;

lazy_static! {
	static ref CLIENT: reqwest::Client = reqwest::Client::new();
}

#[derive(Serialize)]
pub struct CreateServerRequest<'a> {
	server: CreateServer<'a>
}

#[derive(Serialize)]
struct CreateServer<'a> {
	title: &'a str,
	hostname: &'a str,
	plan: &'a str,
	core_number: u16,
	memory_amount: u16,
	zone: &'a str,
	timezone: &'a str,
	password_delivery: &'a str,
	firewall: &'a str,
	networking: Networking<'a>,
	storage_devices: StorageDevices<'a>
}

#[derive(Serialize)]
struct Networking<'a> {
	interfaces: Interfaces<'a>
}

#[derive(Serialize)]
struct Interfaces<'a> {
	interface: Vec<Interface<'a>>
}

#[derive(Serialize)]
struct Interface<'a> {
	ip_addresses: IpAddresses,
	#[serde(rename = "type")]
	ty: &'a str
}

#[derive(Serialize)]
struct StorageDevices<'a> {
	storage_device: Vec<StorageDevice<'a>>
}

#[derive(Serialize)]
struct StorageDevice<'a> {
	action: &'a str,
	storage: &'a str,
	title: String,
	size: u16,
	tier: &'a str
}

#[derive(Deserialize)]
pub struct ServerResponse {
	server: Server
}

#[derive(Deserialize)]
struct Server {
	ip_addresses: IpAddresses,
	password: String,
	uuid: String
}

#[derive(Deserialize, Serialize)]
struct IpAddresses {
	ip_address: Vec<IpAddress>
}

#[derive(Deserialize, Serialize)]
struct IpAddress {
	#[serde(skip_serializing)]
	access: String,
	#[serde(skip_serializing)]
	address: String,
	family: IpFamily
}

#[derive(Deserialize, PartialEq, Serialize)]
enum IpFamily {
	IPv4,
	IPv6
}

impl ServerResponse {
	pub fn ip_addr(&self) -> Option<&str> {
		self.server
			.ip_addresses
			.ip_address
			.iter()
			.filter(|addr| addr.access == "public" && addr.family == IpFamily::IPv6)
			.map(|addr| addr.address.as_ref())
			.next()
	}

	pub fn password(&self) -> &str {
		&self.server.password
	}

	pub fn uuid(&self) -> &str {
		&self.server.uuid
	}
}

impl<'a> CreateServerRequest<'a> {
	pub fn new(title: &'a str, hostname: &'a str) -> Self {
		let storage_title = format!("{} (Debian Buster)", title);
		Self {
			server: CreateServer {
				title,
				hostname,
				plan: "custom",
				core_number: UPCLOUD_CORES,
				memory_amount: UPCLOUD_MEMORY,
				zone: "de-fra1",
				timezone: "Europe/Berlin",
				password_delivery: "none",
				firewall: "off",
				networking: Networking {
					interfaces: Interfaces {
						interface: vec![
							// TODO remove the IPv4 address once docker's registry finally
							// supports IPv6: https://github.com/docker/roadmap/issues/89
							Interface {
								ip_addresses: IpAddresses {
									ip_address: vec![IpAddress {
										access: String::new(),
										address: String::new(),
										family: IpFamily::IPv4
									}]
								},
								ty: "public"
							},
							Interface {
								ip_addresses: IpAddresses {
									ip_address: vec![IpAddress {
										access: String::new(),
										address: String::new(),
										family: IpFamily::IPv6
									}]
								},
								ty: "public"
							},
						]
					}
				},
				storage_devices: StorageDevices {
					storage_device: vec![StorageDevice {
						action: "clone",
						storage: "01000000-0000-4000-8000-000020050100",
						title: storage_title,
						size: UPCLOUD_STORAGE,
						tier: "maxiops"
					}]
				}
			}
		}
	}

	pub async fn send(&self, username: &str, password: &str) -> anyhow::Result<ServerResponse> {
		let auth = format!("{}:{}", username, password);
		let value: serde_json::Value = CLIENT
			.post("https://api.upcloud.com/1.3/server")
			.header("authorization", format!("Basic {}", base64::encode(auth.as_bytes())))
			.json(self)
			.send()
			.await?
			.json()
			.await?;
		debug!("Response: {:?}", value);
		Ok(serde_json::from_value(value)?)
	}
}

#[derive(Serialize)]
pub struct StopServerRequest {
	stop_server: StopServer
}

#[derive(Serialize)]
struct StopServer {
	stop_type: &'static str,
	timeout: &'static str
}

impl StopServerRequest {
	pub fn new() -> Self {
		Self {
			stop_server: StopServer {
				stop_type: "soft",
				timeout: "30"
			}
		}
	}

	pub async fn send(&self, username: &str, password: &str, server_uuid: &str) -> anyhow::Result<serde_json::Value> {
		let auth = format!("{}:{}", username, password);
		let value: serde_json::Value = CLIENT
			.post(&format!("https://api.upcloud.com/1.3/server/{}/stop", server_uuid))
			.header("authorization", format!("Basic {}", base64::encode(auth.as_bytes())))
			.json(self)
			.send()
			.await?
			.json()
			.await?;
		debug!("Response: {:?}", value);
		Ok(value)
	}
}

#[derive(Serialize)]
pub struct DeleteServerRequest {
	storages: u8
}

impl DeleteServerRequest {
	pub fn new() -> Self {
		Self { storages: 1 }
	}

	pub async fn send(&self, username: &str, password: &str, server_uuid: &str) -> anyhow::Result<()> {
		let auth = format!("{}:{}", username, password);
		let bytes = CLIENT
			.delete(&format!("https://api.upcloud.com/1.3/server/{}", server_uuid))
			.query(self)
			.header("authorization", format!("Basic {}", base64::encode(auth.as_bytes())))
			.send()
			.await?;
		debug!("Response: {:?}", bytes);
		Ok(())
	}
}

pub async fn destroy_server(uuid: &str) -> anyhow::Result<()> {
	info!("Removing Server {}", uuid);

	let username = "msrd0";
	let password = env::var("UPCLOUD_PASSWORD")?;

	StopServerRequest::new().send(username, &password, uuid).await?;
	delay_for(Duration::new(30, 0)).await;
	DeleteServerRequest::new().send(username, &password, uuid).await?;

	Ok(())
}
