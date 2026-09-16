//! SSH bootstrap only; routine administration uses the outbound agent.
use super::*;
use base64::{engine::general_purpose::STANDARD_NO_PAD,Engine as _};
use ssh2::Session;
use std::net::{TcpStream,ToSocketAddrs};
fn connect(p:&Value)->Result<Session>{
    let host=field(p,"host")?;anyhow::ensure!(!host.is_empty()&&host.len()<=253&&host.bytes().all(|b|b.is_ascii_alphanumeric()||b".-:".contains(&b)),"Invalid SSH host");
    let port:u16=p["port"].as_u64().unwrap_or(22).try_into()?;anyhow::ensure!(port>0,"Invalid SSH port");
    let address=(host,port).to_socket_addrs()?.next().context("SSH host did not resolve")?;
    let socket=TcpStream::connect_timeout(&address,Duration::from_secs(15))?;let mut session=Session::new()?;
    session.set_tcp_stream(socket);session.set_timeout(30_000);session.handshake()?;Ok(session)
}
fn fingerprint(s:&Session)->Result<String>{Ok(format!("SHA256:{}",STANDARD_NO_PAD.encode(s.host_key_hash(ssh2::HashType::Sha256).context("SSH host key unavailable")?)))}
pub(super) fn probe(p:&Value)->Result<Value>{let s=connect(p)?;Ok(json!({"fingerprint":fingerprint(&s)?}))}
fn run(s:&Session,command:&str)->Result<String>{
    let mut c=s.channel_session()?;c.exec(command)?;let mut output=String::new();c.read_to_string(&mut output)?;
    let mut errors=String::new();c.stderr().read_to_string(&mut errors)?;c.wait_close()?;
    anyhow::ensure!(c.exit_status()?==0,"Remote setup failed: {}",errors.chars().take(2000).collect::<String>());Ok(output)
}
fn upload(s:&Session,path:&str,bytes:&[u8],mode:i32)->Result<()>{let mut c=s.scp_send(Path::new(path),mode,bytes.len() as u64,None)?;c.write_all(bytes)?;c.send_eof()?;c.wait_eof()?;c.close()?;c.wait_close()?;Ok(())}
fn stage(e:&Engine,id:&str,message:&str,state:&str)->Result<()>{write_json(&e.ws.root().join(format!("deployment-{id}.json")),&json!({"hostId":id,"stage":message,"state":state,"updatedAt":auth::now()}))}
pub(super) fn deploy(e:&Arc<Engine>,p:&Value,report:game::Reporter)->Result<Value>{
    let id=field(p,"hostId")?;anyhow::ensure!(id.len()==32&&id.bytes().all(|b|b.is_ascii_hexdigit()),"Invalid host ID");
    let step=|m:&str|->Result<()>{stage(e,id,m,"running")?;report(m.into());Ok(())};
    let result=(||->Result<Value>{
        step("Verifying SSH host identity")?;let s=connect(p)?;
        anyhow::ensure!(fingerprint(&s)?==field(p,"fingerprint")?,"SSH host key changed. Verify the server identity before retrying.");
        let user=field(p,"username")?;anyhow::ensure!(!user.is_empty()&&user.len()<=64&&user.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b)),"Invalid SSH username");
        step("Authenticating with the server")?;
        if let Some(key)=p["privateKey"].as_str().filter(|s|!s.is_empty()){
            let mut f=tempfile::NamedTempFile::new_in(e.ws.root())?;f.write_all(key.as_bytes())?;f.flush()?;
            s.userauth_pubkey_file(user,None,f.path(),p["passphrase"].as_str().filter(|s|!s.is_empty()))?;
        }else{s.userauth_password(user,field(p,"password")?)?;}
        anyhow::ensure!(s.authenticated(),"SSH authentication failed");s.set_timeout(300_000);
        step("Checking Linux, architecture, systemd and administrator access")?;
        let facts=run(&s,"set -eu; test \"$(uname -s)\" = Linux; command -v systemctl >/dev/null; test -d /run/systemd/system; uname -m; id -u; df -Pk /var/lib | tail -1 | awk '{print $4}'")?;
        let lines=facts.lines().collect::<Vec<_>>();anyhow::ensure!(lines.len()==3&&lines[0]=="x86_64","Initial managed-host preview requires Linux x86_64 with systemd");
        anyhow::ensure!(lines[2].parse::<u64>()?>1_048_576,"At least 1 GiB of free space is required for the host service");
        let sudo=if lines[1]=="0"{""}else{run(&s,"sudo -n true")?;"sudo -n "};
        step("Registering the management identity")?;managed::enroll(e,id)?;let c=managed::credentials(e,id)?;
        let config=json!({"url":c["url"],"hostId":id,"agentToken":c["agentToken"]});
        step("Downloading and verifying the headless service")?;
        let version=p["releaseTag"].as_str().unwrap_or(concat!("v",env!("CARGO_PKG_VERSION")));
        anyhow::ensure!(version.starts_with('v')&&version.len()<=80&&version.bytes().all(|b|b.is_ascii_alphanumeric()||b".-".contains(&b)),"Invalid release tag");
        let url=format!("https://github.com/marcusyang-meta/blocklink/releases/download/{version}/blocklink-service-linux-x86_64");
        let checksum=client()?.get(format!("{url}.sha256")).send()?.error_for_status().context("This release has no headless service artifact yet")?.text()?;
        let expected=checksum.split_whitespace().next().context("Missing checksum")?;anyhow::ensure!(expected.len()==64&&expected.bytes().all(|b|b.is_ascii_hexdigit()),"Invalid release checksum");
        let mut file=tempfile::NamedTempFile::new_in(e.ws.root())?;let mut response=client()?.get(&url).send()?.error_for_status()?;
        let n=std::io::copy(&mut response.by_ref().take(268_435_457),&mut file)?;file.flush()?;
        anyhow::ensure!(n<=268_435_456&&hash_file(file.path(),"sha256")?.eq_ignore_ascii_case(expected),"Headless service checksum mismatch");
        let dir=format!("/tmp/blocklink-{}",uuid::Uuid::new_v4().simple());run(&s,&format!("umask 077; mkdir {dir}"))?;
        let installation=(||->Result<()>{
            step("Uploading the service and one-time agent configuration")?;
            upload(&s,&format!("{dir}/blocklink-service"),&fs::read(file.path())?,0o700)?;upload(&s,&format!("{dir}/agent.json"),&serde_json::to_vec(&config)?,0o600)?;
            upload(&s,&format!("{dir}/install.sh"),include_bytes!("managed-install.sh"),0o700)?;
            step("Installing the persistent system service")?;run(&s,&format!("{sudo}sh {dir}/install.sh {dir}"))?;Ok(())
        })();let _=run(&s,&format!("rm -rf -- {dir}"));installation?;
        step("Waiting for the agent to connect")?;
        for _ in 0..15{if let Ok(status)=managed::owner_request(e,"managed-status",&json!({"hostId":id})){if status["online"]==true{stage(e,id,"Agent connected","done")?;return Ok(json!({"hostId":id,"online":true}));}}std::thread::sleep(Duration::from_secs(2));}
        bail!("Service installed, but the agent has not connected. Check outbound HTTPS access and refresh host status before reinstalling.")
    })();if result.is_err(){let _=stage(e,id,"Deployment interrupted; retry from the app","error");}result
}
#[cfg(test)] mod tests{use super::*;#[test] fn rejects_host_shell_syntax_before_network_access(){assert!(connect(&json!({"host":"x;touch /tmp/pwn"})).is_err());assert!(connect(&json!({"host":"-oProxyCommand=sh"})).is_err());}}
