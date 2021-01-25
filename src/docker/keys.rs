use openssl::{
	asn1::{Asn1Integer, Asn1Time},
	bn::BigNum,
	ec::{EcGroup, EcKey},
	hash::MessageDigest,
	nid::Nid,
	pkey::{HasPrivate, PKey},
	rsa::Rsa,
	x509::{
		extension::{BasicConstraints, ExtendedKeyUsage, SubjectAlternativeName, SubjectKeyIdentifier},
		X509Builder, X509NameBuilder, X509
	}
};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::path::{Path, PathBuf};
use tempfile::{tempdir, TempDir};
use tokio::{fs::File, io::AsyncWriteExt};

fn serial_number() -> anyhow::Result<Asn1Integer> {
	let hex = thread_rng()
		.sample_iter(Alphanumeric)
		.map(char::from)
		.filter(|char| char.is_digit(16))
		.take(40)
		.collect::<String>();
	let bn = BigNum::from_hex_str(&hex)?;
	Ok(Asn1Integer::from_bn(&bn)?)
}

async fn write_privkey_pem<T: HasPrivate>(key: &Rsa<T>, path: &Path) -> anyhow::Result<()> {
	info!("Writing key file {}", path.to_string_lossy());
	let pem = key.private_key_to_pem()?;
	let mut file = File::create(path).await?;
	file.write_all(&pem).await?;
	Ok(())
}

async fn write_x509_pem(x509: &X509, path: &Path) -> anyhow::Result<()> {
	info!("Writing x509 file {}", path.to_string_lossy());
	let pem = x509.to_pem()?;
	let mut file = File::create(path).await?;
	file.write_all(&pem).await?;
	Ok(())
}

pub struct DockerKeys {
	tmpdir: TempDir,
	pub ca_pem: Vec<u8>,
	pub server_cert_pem: Vec<u8>,
	pub server_key_pem: Vec<u8>
}

impl DockerKeys {
	pub fn ca_path(&self) -> PathBuf {
		self.tmpdir.path().join("ca.pem")
	}

	pub fn client_key_path(&self) -> PathBuf {
		self.tmpdir.path().join("client-key.pem")
	}

	pub fn client_cert_path(&self) -> PathBuf {
		self.tmpdir.path().join("client.pem")
	}
}

pub async fn gen_docker_keys(_ip: &str, domain: &str) -> anyhow::Result<DockerKeys> {
	info!("Generating Docker Keys");

	let secp384r1 = EcGroup::from_curve_name(Nid::SECP384R1)?;
	let sha256 = MessageDigest::from_nid(Nid::SHA256).ok_or(anyhow::Error::msg("SHA256 unknown to openssl"))?;

	let keydir = tempdir()?;
	let dir = keydir.path();

	let mut ca_name = X509NameBuilder::new()?;
	ca_name.append_entry_by_text("C", "DE")?;
	ca_name.append_entry_by_text("O", "Temporary CA")?;
	ca_name.append_entry_by_text("CN", domain)?;
	let ca_name = ca_name.build();

	let mut server_name = X509NameBuilder::new()?;
	server_name.append_entry_by_text("C", "DE")?;
	server_name.append_entry_by_text("O", "Server Certificate")?;
	server_name.append_entry_by_text("CN", domain)?;
	let server_name = server_name.build();

	let mut client_name = X509NameBuilder::new()?;
	client_name.append_entry_by_text("C", "DE")?;
	client_name.append_entry_by_text("O", "Client Certificate")?;
	client_name.append_entry_by_text("CN", domain)?;
	let client_name = client_name.build();

	let now = Asn1Time::days_from_now(0)?;
	let next_week = Asn1Time::days_from_now(7)?;

	let ca_key = EcKey::generate(&secp384r1)?;
	let ca_keypair = PKey::from_ec_key(ca_key)?;

	let mut ca_cert = X509Builder::new()?;
	ca_cert.set_serial_number(serial_number()?.as_ref())?;
	ca_cert.set_not_before(&now)?;
	ca_cert.set_not_after(&next_week)?;
	ca_cert.set_version(2)?;
	ca_cert.set_issuer_name(&ca_name)?;
	ca_cert.set_subject_name(&ca_name)?;
	ca_cert.set_pubkey(&ca_keypair)?;
	let ca_cert_ctx = ca_cert.x509v3_context(None, None);
	let ca_cert_ext_alt = SubjectAlternativeName::new().dns(domain).build(&ca_cert_ctx)?;
	let ca_cert_ext_ski = SubjectKeyIdentifier::new().build(&ca_cert_ctx)?;
	let mut ca_cert_ext_bc = BasicConstraints::new();
	ca_cert_ext_bc.critical();
	ca_cert_ext_bc.ca();
	ca_cert.append_extension(ca_cert_ext_alt)?;
	ca_cert.append_extension(ca_cert_ext_ski)?;
	ca_cert.append_extension(ca_cert_ext_bc.build()?)?;
	ca_cert.sign(&ca_keypair, sha256)?;
	let ca_cert = ca_cert.build();
	let ca_pem = ca_cert.to_pem()?;
	write_x509_pem(&ca_cert, &dir.join("ca.pem")).await?;

	let server_key = EcKey::generate(&secp384r1)?;
	let server_key_pem = server_key.private_key_to_pem()?;
	let server_pubkey = PKey::from_ec_key(server_key)?;

	let mut server_cert = X509Builder::new()?;
	server_cert.set_serial_number(serial_number()?.as_ref())?;
	server_cert.set_not_before(&now)?;
	server_cert.set_not_after(&next_week)?;
	server_cert.set_version(2)?;
	server_cert.set_issuer_name(&ca_name)?;
	server_cert.set_subject_name(&server_name)?;
	server_cert.set_pubkey(&server_pubkey)?;
	let server_cert_ctx = server_cert.x509v3_context(None, None);
	let server_cert_ext_alt = SubjectAlternativeName::new().dns(domain).build(&server_cert_ctx)?;
	let server_cert_ext_usage = ExtendedKeyUsage::new().server_auth().build()?;
	server_cert.append_extension(server_cert_ext_alt)?;
	server_cert.append_extension(server_cert_ext_usage)?;
	server_cert.sign(&ca_keypair, sha256)?;
	let server_cert = server_cert.build();
	let mut server_cert_pem = server_cert.to_pem()?;
	server_cert_pem.extend_from_slice(&ca_pem);

	let client_key = Rsa::generate(4096)?;
	write_privkey_pem(&client_key, &dir.join("client-key.pem")).await?;
	let client_pubkey = PKey::from_rsa(client_key)?;

	let mut client_cert = X509Builder::new()?;
	client_cert.set_serial_number(serial_number()?.as_ref())?;
	client_cert.set_not_before(&now)?;
	client_cert.set_not_after(&next_week)?;
	client_cert.set_version(2)?;
	client_cert.set_issuer_name(&ca_name)?;
	client_cert.set_subject_name(&client_name)?;
	client_cert.set_pubkey(&client_pubkey)?;
	let client_cert_ctx = client_cert.x509v3_context(None, None);
	let client_cert_ext_alt = SubjectAlternativeName::new().dns(domain).build(&client_cert_ctx)?;
	let client_cert_ext_usage = ExtendedKeyUsage::new().client_auth().build()?;
	client_cert.append_extension(client_cert_ext_alt)?;
	client_cert.append_extension(client_cert_ext_usage)?;
	client_cert.sign(&ca_keypair, sha256)?;
	let client_cert = client_cert.build();
	write_x509_pem(&client_cert, &dir.join("client.pem")).await?;

	Ok(DockerKeys {
		tmpdir: keydir,
		ca_pem,
		server_cert_pem,
		server_key_pem
	})
}
