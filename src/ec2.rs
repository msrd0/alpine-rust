use openssl::{
	asn1::Asn1Time,
	ec::{EcGroup, EcKey},
	hash::MessageDigest,
	nid::Nid,
	pkey::{HasPrivate, PKey},
	x509::{
		extension::{BasicConstraints, ExtendedKeyUsage, SubjectAlternativeName, SubjectKeyIdentifier},
		X509Builder, X509NameBuilder, X509
	}
};
use std::path::Path;
use tempfile::{tempdir, TempDir};
use tokio::{fs::File, io::AsyncWriteExt};

async fn write_privkey_pem<T: HasPrivate>(key: &EcKey<T>, path: &Path) -> anyhow::Result<()> {
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

pub(super) async fn gen_docker_keys() -> anyhow::Result<TempDir> {
	info!("Generating Docker Keys");
	let domain = "thisdoesnotexist.org";
	let ip = "127.0.0.1";

	let secp384r1 = EcGroup::from_curve_name(Nid::SECP384R1)?;
	let sha256 = MessageDigest::from_nid(Nid::SHA256).ok_or(anyhow::Error::msg("SHA256 unknown to openssl"))?;

	let keydir = tempdir()?;
	let dir = keydir.path();

	let mut x509_name = X509NameBuilder::new()?;
	x509_name.append_entry_by_text("C", "DE")?;
	x509_name.append_entry_by_text("O", "https://github.com/msrd0/alpine-rust")?;
	x509_name.append_entry_by_text("CN", domain)?;
	let x509_name = x509_name.build();

	let now = Asn1Time::days_from_now(0)?;
	let next_week = Asn1Time::days_from_now(7)?;

	let ca_key = EcKey::generate(&secp384r1)?;
	write_privkey_pem(&ca_key, &dir.join("ca-key.pem")).await?;
	let ca_pubkey = PKey::from_ec_key(EcKey::from_public_key(&secp384r1, ca_key.public_key())?)?;
	let ca_privkey = PKey::from_ec_key(ca_key)?;

	let mut ca_cert = X509Builder::new()?;
	ca_cert.set_not_before(&now)?;
	ca_cert.set_not_after(&next_week)?;
	ca_cert.set_version(2)?;
	ca_cert.set_issuer_name(&x509_name)?;
	ca_cert.set_subject_name(&x509_name)?;
	ca_cert.set_pubkey(&ca_pubkey)?;
	let ca_cert_ctx = ca_cert.x509v3_context(None, None);
	let ca_cert_ext_ski = SubjectKeyIdentifier::new().build(&ca_cert_ctx)?;
	let mut ca_cert_ext_bc = BasicConstraints::new();
	ca_cert_ext_bc.critical();
	ca_cert_ext_bc.ca();
	ca_cert.append_extension(ca_cert_ext_ski)?;
	ca_cert.append_extension(ca_cert_ext_bc.build()?)?;
	ca_cert.sign(&ca_privkey, sha256)?;
	let ca_cert = ca_cert.build();
	write_x509_pem(&ca_cert, &dir.join("ca.pem")).await?;

	let server_key = EcKey::generate(&secp384r1)?;
	write_privkey_pem(&server_key, &dir.join("server-key.pem")).await?;
	let server_pubkey = PKey::from_ec_key(EcKey::from_public_key(&secp384r1, server_key.public_key())?)?;

	let mut server_cert = X509Builder::new()?;
	server_cert.set_not_before(&now)?;
	server_cert.set_not_after(&next_week)?;
	server_cert.set_version(2)?;
	server_cert.set_issuer_name(&x509_name)?;
	server_cert.set_subject_name(&x509_name)?;
	server_cert.set_pubkey(&server_pubkey)?;
	let server_cert_ctx = server_cert.x509v3_context(None, None);
	let server_cert_ext_alt = SubjectAlternativeName::new().dns(domain).ip(ip).build(&server_cert_ctx)?;
	let server_cert_ext_usage = ExtendedKeyUsage::new().server_auth().build()?;
	server_cert.append_extension(server_cert_ext_alt)?;
	server_cert.append_extension(server_cert_ext_usage)?;
	server_cert.sign(&ca_privkey, sha256)?;
	let server_cert = server_cert.build();
	write_x509_pem(&server_cert, &dir.join("server.pem")).await?;

	let client_key = EcKey::generate(&secp384r1)?;
	write_privkey_pem(&client_key, &dir.join("client-key.pem")).await?;
	let client_pubkey = PKey::from_ec_key(EcKey::from_public_key(&secp384r1, client_key.public_key())?)?;

	let mut client_cert = X509Builder::new()?;
	client_cert.set_not_before(&now)?;
	client_cert.set_not_after(&next_week)?;
	client_cert.set_version(2)?;
	client_cert.set_issuer_name(&x509_name)?;
	client_cert.set_subject_name(&x509_name)?;
	client_cert.set_pubkey(&client_pubkey)?;
	let client_cert_ext_usage = ExtendedKeyUsage::new().client_auth().build()?;
	client_cert.append_extension(client_cert_ext_usage)?;
	client_cert.sign(&ca_privkey, sha256)?;
	let client_cert = client_cert.build();
	write_x509_pem(&client_cert, &dir.join("client.pem")).await?;

	Ok(keydir)
}
