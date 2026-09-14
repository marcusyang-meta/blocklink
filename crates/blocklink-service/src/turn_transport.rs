//! Adapt TURN stream transports to the UDP interface of the ICE library.
//! TURN messages and credentials remain unchanged; only RFC 5766 framing changes.
use anyhow::{bail, ensure, Context, Result};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use std::time::Duration;
use webrtc::peer_connection::RTCIceServer;

pub(crate) struct Guard(JoinHandle<()>);
impl Drop for Guard { fn drop(&mut self) { self.0.abort(); } }

pub(crate) async fn prepare(servers: Vec<RTCIceServer>) -> Result<(Vec<RTCIceServer>, Vec<Guard>)> {
    let mut output = Vec::new();
    let mut guards = Vec::new();
    for mut server in servers {
        let mut urls = Vec::new();
        for original in server.urls {
            let secure = original.starts_with("turns:");
            let tcp = original.starts_with("turn:") && original.contains("transport=tcp");
            if !secure && !tcp { urls.push(original); continue; }
            let (_, tail) = original.split_once(':').context("Invalid TURN URL")?;
            let parsed = reqwest::Url::parse(&format!("turn-transport://{tail}"))?;
            ensure!(parsed.username().is_empty() && parsed.password().is_none(), "Invalid TURN authority");
            let host = parsed.host_str().context("Missing TURN host")?.to_owned();
            let port = parsed.port().unwrap_or(if secure {5349} else {3478});
            let udp = UdpSocket::bind("127.0.0.1:0").await?;
            urls.push(format!("turn:127.0.0.1:{}?transport=udp", udp.local_addr()?.port()));
            guards.push(Guard(tokio::spawn(async move {
                let result = async {
                    let mut first = vec![0; 65536];
                    let (size, peer) = udp.recv_from(&mut first).await?;
                    #[cfg(test)] eprintln!("TURN adapter received initial datagram");
                    udp.connect(peer).await?;
                    let socket = tokio::time::timeout(Duration::from_secs(8), TcpStream::connect((host.as_str(),port))).await??;
                    socket.set_nodelay(true)?;
                    if secure {
                        let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                        let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                            .with_safe_default_protocol_versions()?.with_root_certificates(roots).with_no_client_auth();
                        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
                        let tls = tokio::time::timeout(Duration::from_secs(8),connector.connect(host.try_into()?,socket)).await??;
                        #[cfg(test)] eprintln!("TURN TLS certificate verified on port {port}");
                        bridge(udp,tls,&first[..size]).await
                    } else { bridge(udp,socket,&first[..size]).await }
                }.await;
                if let Err(_error) = result { eprintln!("TURN stream transport unavailable"); #[cfg(test)] eprintln!("TURN adapter: {_error:#}"); }
            })));
        }
        server.urls = urls;
        output.push(server);
    }
    Ok((output, guards))
}

fn frame_lengths(header: &[u8]) -> Result<(usize,usize)> {
    ensure!(header.len() >= 4, "Truncated TURN header");
    let length = u16::from_be_bytes([header[2],header[3]]) as usize;
    match header[0] >> 6 {
        0 => { ensure!(length % 4 == 0, "Invalid STUN length"); Ok((20+length,20+length)) },
        1 => { let size=4+length; Ok((size,(size+3)&!3)) },
        _ => bail!("Invalid TURN frame"),
    }
}
async fn write_packet<W: AsyncWrite + Unpin>(writer: &mut W, packet: &[u8]) -> Result<()> {
    let (size,padded) = frame_lengths(packet)?;
    ensure!(packet.len() >= size && packet.len() <= padded, "Invalid TURN datagram length");
    writer.write_all(&packet[..size]).await?;
    writer.write_all(&[0;3][..padded-size]).await?;
    Ok(())
}
async fn read_packet<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let mut header=[0;4]; reader.read_exact(&mut header).await?;
    let (size,padded)=frame_lengths(&header)?;
    let mut packet=vec![0;padded];packet[..4].copy_from_slice(&header);
    reader.read_exact(&mut packet[4..]).await?;
    packet.truncate(size); Ok(packet)
}
async fn bridge<S: AsyncRead + AsyncWrite + Unpin>(udp: UdpSocket, stream: S, first: &[u8]) -> Result<()> {
    let (mut reader,mut writer)=tokio::io::split(stream);
    write_packet(&mut writer,first).await?;
    let send=async {
        let mut packet=vec![0;65536];
        loop { let size=udp.recv(&mut packet).await?;write_packet(&mut writer,&packet[..size]).await?; }
        #[allow(unreachable_code)] Ok::<(),anyhow::Error>(())
    };
    let receive=async {
        loop { let packet=read_packet(&mut reader).await?;udp.send(&packet).await?; }
        #[allow(unreachable_code)] Ok::<(),anyhow::Error>(())
    };
    tokio::try_join!(send,receive)?;Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    #[tokio::test]
    async fn stream_frames_preserve_datagrams_and_padding() -> Result<()> {
        let (mut writer,mut reader)=tokio::io::duplex(1024);
        let mut stun=vec![0;20];stun[1]=1;
        let channel=vec![0x40,1,0,3,8,9,10];
        write_packet(&mut writer,&stun).await?;
        write_packet(&mut writer,&channel).await?;
        write_packet(&mut writer,&stun).await?;
        assert_eq!(read_packet(&mut reader).await?,stun);
        assert_eq!(read_packet(&mut reader).await?,channel);
        assert_eq!(read_packet(&mut reader).await?,stun);
        assert!(frame_lengths(&[0x80,0,0,0]).is_err());
        assert!(write_packet(&mut writer,&[0x40,0,0,3,1]).await.is_err());
        Ok(())
    }
}
