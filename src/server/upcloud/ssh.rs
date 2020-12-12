use ssh2::Session;
use std::{
	collections::HashMap,
	io::{BufRead, BufReader, Read, Write},
	net::{TcpStream, ToSocketAddrs},
	path::Path,
	time::Duration
};
use tokio::{
	fs::File,
	io::{AsyncReadExt, AsyncWriteExt},
	time::delay_for
};

fn try_connect(domain: &str, password: &str) -> anyhow::Result<Session> {
	let addr = (domain, 22).to_socket_addrs()?.next().unwrap();
	let tcp = TcpStream::connect(addr)?;
	let mut sess = Session::new()?;
	sess.set_tcp_stream(tcp);
	sess.handshake()?;
	sess.userauth_password("root", password)?;
	Ok(sess)
}

pub async fn connect(domain: &str, password: &str) -> anyhow::Result<Session> {
	let mut error = None;
	for i in 0..10 {
		info!("Connecting to {}:22", domain);
		let err = match try_connect(domain, password) {
			Ok(sess) => return Ok(sess),
			Err(err) => err
		};
		error!("{}", err);
		error = Some(err);

		let wait = 2 * (i + 1);
		info!("Retrying SSH connection in {}s", wait);
		delay_for(Duration::new(wait, 0)).await;
	}
	Err(error.unwrap())
}

pub fn run(sess: &mut Session, cmd: &str) -> anyhow::Result<()> {
	info!("SSH: Running `{}`", cmd);
	let mut channel = sess.channel_session()?;
	channel.exec(cmd)?;

	let reader = BufReader::new(&mut channel);
	for line in reader.lines() {
		println!("[ssh] {}", line?);
	}

	channel.wait_close()?;
	let exit_code = channel.exit_status()?;
	info!("SSH: Command completed with exit code {}", exit_code);
	if exit_code == 0 {
		Ok(())
	} else {
		Err(anyhow::Error::msg(format!(
			"Command `{}` returned exit-code {}",
			cmd, exit_code
		)))
	}
}

pub fn index(sess: &mut Session, path: &str) -> anyhow::Result<HashMap<String, String>> {
	info!("SSH: Indexing {}", path);
	let mut channel = sess.channel_session()?;
	let cmd = format!("cd '{}' && test -z \"$(ls)\" || sha256sum *", path);
	channel.exec(&cmd)?;

	let mut index: HashMap<String, String> = HashMap::new();
	let reader = BufReader::new(&mut channel);
	for line in reader.lines() {
		let line = line?;
		let whitespace = match line.find(|c: char| c.is_whitespace()) {
			Some(index) => index,
			None => {
				warn!("SSH: Skipping unknown line {:?}", line);
				continue;
			}
		};
		let hash = line[0..whitespace].trim();
		let name = line[whitespace..].trim();
		index.insert(name.to_owned(), hash.to_owned());
	}

	channel.wait_close()?;
	let exit_code = channel.exit_status()?;
	info!("SSH: Command completed with exit code {}", exit_code);
	if exit_code == 0 {
		Ok(index)
	} else {
		Err(anyhow::Error::msg(format!(
			"Command `{}` returned exit-code {}",
			cmd, exit_code
		)))
	}
}

pub fn send(sess: &mut Session, path: &str, data: &[u8]) -> anyhow::Result<()> {
	info!("SSH: Uploading {}", path);
	let mut file = sess.scp_send(path.as_ref(), 0o600, data.len() as u64, None)?;
	file.write(data)?;

	file.send_eof()?;
	file.wait_eof()?;
	file.close()?;
	file.wait_close()?;

	Ok(())
}

pub async fn upload(sess: &mut Session, path: &str, host: &Path) -> anyhow::Result<()> {
	info!("SSH: Uploading {}", path);
	let mut host = File::open(host).await?;
	let mut file = sess.scp_send(path.as_ref(), 0o600, host.metadata().await?.len(), None)?;

	let mut buf = [0u8; 8192];
	loop {
		let count = host.read(&mut buf).await?;
		if count == 0 {
			break;
		}
		file.write(&buf[0..count])?;
	}

	file.send_eof()?;
	file.wait_eof()?;
	file.close()?;
	file.wait_close()?;

	Ok(())
}

pub async fn download(sess: &mut Session, path: &str, host: &Path) -> anyhow::Result<()> {
	info!("SSH: Downloading {}", path);
	let mut host = File::create(host).await?;
	let (mut file, _) = sess.scp_recv(path.as_ref())?;

	let mut buf = [0u8; 8192];
	loop {
		let count = file.read(&mut buf)?;
		if count == 0 {
			break;
		}
		host.write(&buf[0..count]).await?;
	}

	file.send_eof()?;
	file.wait_eof()?;
	file.close()?;
	file.wait_close()?;

	Ok(())
}
