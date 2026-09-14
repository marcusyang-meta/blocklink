//! Reliable, bounded byte streams over native WebRTC. No WebView lifetime dependency.
use super::*;
use bytes::BytesMut;
use std::sync::Weak;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::{mpsc, watch, Semaphore};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
    RTCIceGatheringState, RTCIceServer, RTCIceTransportPolicy,
};
pub(super) type Io = Compat<yamux::Stream>;
pub(super) struct Mux(
    mpsc::Sender<tokio::sync::oneshot::Sender<Result<Io>>>,
    tokio::task::JoinHandle<()>,
);
impl Drop for Mux {
    fn drop(&mut self) {
        self.1.abort();
    }
}
impl Mux {
    pub async fn open(&self) -> Result<Io> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.0.send(tx).await.context("联机已断开")?;
        tokio::time::timeout(Duration::from_secs(15), rx).await??
    }
}
fn multiplex(dc: Arc<dyn DataChannel>, host: Option<(Weak<Engine>, String)>) -> Mux {
    let mut config = yamux::Config::default();
    config
        .set_max_num_streams(32)
        .set_max_connection_receive_window(Some(8 * 1024 * 1024));
    let mut connection = yamux::Connection::new(
        stream(dc).compat(),
        config,
        if host.is_some() {
            yamux::Mode::Server
        } else {
            yamux::Mode::Client
        },
    );
    let (tx, mut rx) = mpsc::channel::<tokio::sync::oneshot::Sender<Result<Io>>>(32);
    let task = tokio::spawn(async move {
        let limit = Arc::new(Semaphore::new(16));
        loop {
            tokio::select! {
                incoming=std::future::poll_fn(|cx|connection.poll_next_inbound(cx)) => {
                    let Some(Ok(stream))=incoming else {break};
                    if let Some((weak,id))=&host {
                        let Ok(permit)=limit.clone().try_acquire_owned() else {drop(stream);continue};
                        let weak=weak.clone();let id=id.clone();
                        tokio::spawn(async move {let _permit=permit;let mut io=stream.compat();if let Err(error)=serve_request(weak,&id,&mut io).await {let _=write_frame(&mut io,&json!({"ok":false,"error":error.to_string()})).await;} let _=io.shutdown().await;});
                    }
                }
                request=rx.recv(), if !rx.is_closed() => {
                    if let Some(request)=request {let stream=std::future::poll_fn(|cx|connection.poll_new_outbound(cx)).await.map(|s|s.compat()).map_err(anyhow::Error::from);let _=request.send(stream);}
                }
            }
        }
    });
    Mux(tx, task)
}
pub(super) fn client(dc: Arc<dyn DataChannel>) -> Mux {
    multiplex(dc, None)
}

pub(super) struct Handler {
    gathered: watch::Sender<bool>,
    incoming: mpsc::Sender<Arc<dyn DataChannel>>,
}
#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_connection_state_change(
        &self,
        state: webrtc::peer_connection::RTCPeerConnectionState,
    ) {
        #[cfg(test)]
        eprintln!("RTC state: {state}");
        let _ = state;
    }
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gathered.send(true);
        }
    }
    async fn on_data_channel(&self, dc: Arc<dyn DataChannel>) {
        #[cfg(test)]
        eprintln!("RTC incoming channel");
        if let Err(error) = self.incoming.try_send(dc) {
            let dc = error.into_inner();
            tokio::spawn(async move {
                let _ = dc.close().await;
            });
        }
    }
}
pub(super) async fn connection(
    servers: Vec<RTCIceServer>,
    relay_only: bool,
) -> Result<(
    Arc<dyn PeerConnection>,
    watch::Receiver<bool>,
    mpsc::Receiver<Arc<dyn DataChannel>>,
)> {
    let (gathered, receiver) = watch::channel(false);
    let (incoming, channels) = mpsc::channel(16);
    let config = RTCConfigurationBuilder::default()
        .with_ice_servers(servers)
        .with_ice_transport_policy(if relay_only {
            RTCIceTransportPolicy::Relay
        } else {
            RTCIceTransportPolicy::All
        })
        .build();
    let pc = PeerConnectionBuilder::new()
        .with_configuration(config)
        .with_handler(Arc::new(Handler { gathered, incoming }))
        .with_data_channel_send_buffer_limit(256 * 1024)
        .with_udp_addrs(vec!["0.0.0.0:0"])
        .build()
        .await?;
    Ok((Arc::new(pc), receiver, channels))
}
pub(super) async fn description(
    pc: &Arc<dyn PeerConnection>,
    mut gathered: watch::Receiver<bool>,
    offer: bool,
) -> Result<Value> {
    let sdp = if offer {
        pc.create_offer(None).await?
    } else {
        pc.create_answer(None).await?
    };
    pc.set_local_description(sdp).await?;
    tokio::time::timeout(Duration::from_secs(25), async {
        while !*gathered.borrow() {
            gathered.changed().await?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("等待网络候选地址超时")??;
    Ok(serde_json::to_value(
        pc.local_description().await.context("连接描述尚未生成")?,
    )?)
}

pub(super) fn stream(dc: Arc<dyn DataChannel>) -> DuplexStream {
    let (application, transport) = tokio::io::duplex(65536);
    tokio::spawn(async move {
        let result = async {
            let mut early_messages = std::collections::VecDeque::new();
            tokio::time::timeout(Duration::from_secs(40), async {
                loop {
                    if dc.ready_state().await? == webrtc::data_channel::RTCDataChannelState::Open {return Ok(())}
                    tokio::select! {
                        event = dc.poll() => match event {Some(DataChannelEvent::OnOpen)=>return Ok(()),Some(DataChannelEvent::OnClose)|None=>bail!("联机通道已关闭"),Some(DataChannelEvent::OnMessage(message))=>{early_messages.push_back(DataChannelEvent::OnMessage(message));return Ok(())},_=>{}},
                        _ = tokio::time::sleep(Duration::from_millis(20)) => {}
                    }
                }
            }).await??;
            #[cfg(test)] eprintln!("RTC channel open");
            let (mut input, mut output) = tokio::io::split(transport);
            let (ready_tx, mut ready_rx) = watch::channel(false);
            dc.send(BytesMut::from(&[2u8][..])).await?;
            let send = async {
                tokio::time::timeout(Duration::from_secs(20), async { while !*ready_rx.borrow() { ready_rx.changed().await?; } Ok::<_, anyhow::Error>(()) }).await??;
                let mut buffer = [0u8; 16385];
                loop {
                    let count = input.read(&mut buffer[1..]).await?;
                    if count == 0 { dc.send(BytesMut::from(&[1u8][..])).await?; break; }
                    #[cfg(test)] eprintln!("RTC sending {} bytes", count);
                    dc.send(BytesMut::from(&buffer[..count + 1])).await?;
                }
                Ok::<_, anyhow::Error>(())
            };
            let receive = async {
                while let Some(event) = if let Some(event) = early_messages.pop_front() { Some(event) } else { dc.poll().await } {
                    match event {
                        DataChannelEvent::OnMessage(message) => {

                            #[cfg(test)] eprintln!("RTC received {} bytes", message.data.len());
                            if message.data.len() > 16385 { bail!("联机数据块过大") }
                            match message.data.first() {
                                Some(2) if message.data.len() == 1 => { if !ready_tx.send_replace(true) { dc.send(BytesMut::from(&[2u8][..])).await?; } },
                                Some(0) => output.write_all(&message.data[1..]).await?,
                                Some(1) if message.data.len() == 1 => { output.shutdown().await?; return Ok(()); },
                                _ => bail!("联机数据格式无效"),
                            }
                        }
                        DataChannelEvent::OnClose => bail!("联机通道意外断开"),
                        _ => {}
                    }
                }
                bail!("联机通道已断开")
            };
            tokio::try_join!(send, receive)?;
            // Let SCTP acknowledge the EOF before closing the channel.
            tokio::time::timeout(Duration::from_secs(10), async { while dc.outstanding_bytes().await.unwrap_or(0) > 0 {tokio::time::sleep(Duration::from_millis(10)).await;} }).await?;
            Ok::<_, anyhow::Error>(())
        }.await;
        #[cfg(test)]
        if let Err(error) = &result {
            eprintln!("RTC stream: {error:#}");
        }
        let _ = result;
        let _ = dc.close().await;
    });
    application
}
pub(super) async fn write_frame(io: &mut Io, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > 8192 {
        bail!("请求过大")
    }
    io.write_u32(bytes.len() as u32).await?;
    io.write_all(&bytes).await?;
    Ok(())
}
pub(super) async fn read_frame(io: &mut Io) -> Result<Value> {
    let size = io.read_u32().await? as usize;
    if size > 8192 {
        bail!("请求过大")
    }
    let mut data = vec![0; size];
    io.read_exact(&mut data).await?;
    Ok(serde_json::from_slice(&data)?)
}
pub(super) fn serve(
    weak: Weak<Engine>,
    id: String,
    mut channels: mpsc::Receiver<Arc<dyn DataChannel>>,
    mut cancelled: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // One persistent DataChannel per peer. Yamux owns stream IDs, flow control and resets.
        let dc = tokio::select! {_=cancelled.changed()=>return,dc=channels.recv()=>dc};
        if let Some(dc) = dc {
            let _mux = multiplex(dc, Some((weak, id)));
            loop {
                tokio::select! {_=cancelled.changed()=>break,extra=channels.recv()=>{let Some(extra)=extra else{break};let _=extra.close().await;}}
            }
        }
    })
}
async fn serve_request(weak: Weak<Engine>, id: &str, io: &mut Io) -> Result<()> {
    let request = tokio::time::timeout(Duration::from_secs(45), read_frame(io)).await??;
    #[cfg(test)]
    eprintln!("RTC request {}", request["kind"]);
    let engine = weak.upgrade().context("后台已退出")?;
    match field(&request, "kind")? {
        "manifest" => {
            let lock = engine.publish_lock(id)?;
            write_frame(io, &json!({"ok":true})).await?;
            io.write_all(&serde_json::to_vec(&lock)?).await?;
        }
        "blob" => {
            let lock = engine.publish_lock(id)?;
            let artifact = lock
                .mods
                .iter()
                .find(|m| m.side != Side::Server && request["hash"] == m.sha512);
            let (hash,size)=if let Some(artifact)=artifact{(artifact.sha512.clone(),artifact.bytes)}else if let Some(bundle)=lock.content.as_ref().filter(|b|request["hash"]==b.sha512){(bundle.sha512.clone(),bundle.bytes)}else{bail!("文件不在已发布环境中")};
            let path = tokio::task::spawn_blocking(move || {
                engine.ws.verify_blob(&hash, size)?;
                engine.ws.blob_path(&hash)
            })
            .await??;
            let mut file = tokio::fs::File::open(path).await?;
            write_frame(io, &json!({"ok":true})).await?;
            tokio::time::timeout(Duration::from_secs(300), tokio::io::copy(&mut file, io))
                .await??;
        }
        "game" | "probe" => {
            if !engine.is_running(id) {
                bail!("房主尚未启动游戏服务器")
            }
            let port = engine.config(id)["port"]
                .as_u64()
                .filter(|p| *p > 0 && *p <= 65535)
                .context("游戏端口无效")?;
            let mut socket = tokio::time::timeout(
                Duration::from_secs(5),
                tokio::net::TcpStream::connect(("127.0.0.1", port as u16)),
            )
            .await??;
            socket.set_nodelay(true)?;
            write_frame(io, &json!({"ok":true})).await?;
            if request["kind"] == "game" {
                tokio::io::copy_bidirectional(io, &mut socket).await?;
            }
        }
        _ => bail!("不支持的联机操作"),
    }
    Ok(())
}
