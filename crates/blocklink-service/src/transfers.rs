use super::*;
use std::{cell::RefCell,sync::Arc,time::Instant};
pub struct Context {pub cancelled:Arc<dyn Fn()->bool+Send+Sync>,pub event:Arc<dyn Fn(Value)+Send+Sync>}
thread_local! {static CURRENT:RefCell<Option<Context>>=const{RefCell::new(None)};}
pub fn attach(context:Context){CURRENT.with(|c|*c.borrow_mut()=Some(context));}
#[derive(Debug)] pub struct Cancelled;
impl std::fmt::Display for Cancelled {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{write!(f,"已取消")}}
impl std::error::Error for Cancelled {}
pub fn check()->Result<()> {if CURRENT.with(|c|c.borrow().as_ref().is_some_and(|c|(c.cancelled)())){return Err(Cancelled.into())}Ok(())}
pub fn event(value:Value){CURRENT.with(|c|{if let Some(c)=c.borrow().as_ref(){(c.event)(value)}});}
pub fn copy(reader:&mut impl Read,writer:&mut impl Write,total:Option<u64>,file:&str)->Result<u64>{
 let start=Instant::now();let mut tick=Instant::now();let mut bytes=0u64;let mut buffer=[0;65536];
 event(json!({"download":{"file":file,"received":0,"total":total,"speed":0}}));
 loop{check()?;let n=reader.read(&mut buffer)?;if n==0{break}bytes+=n as u64;if bytes>2_147_483_648{bail!("下载超过大小限制")};writer.write_all(&buffer[..n])?;
  if tick.elapsed().as_millis()>=250 {event(json!({"download":{"file":file,"received":bytes,"total":total,"speed":(bytes as f64/start.elapsed().as_secs_f64().max(0.001)) as u64}}));tick=Instant::now();}
 }
 check()?;event(json!({"download":{"file":file,"received":bytes,"total":total,"speed":0}}));Ok(bytes)
}
#[cfg(test)]mod tests{use super::*;#[test]fn cancellation_stops_before_writing(){attach(Context{cancelled:Arc::new(||true),event:Arc::new(|_|{})});let mut out=vec![];assert!(copy(&mut &b"abc"[..],&mut out,Some(3),"x").unwrap_err().is::<Cancelled>());assert!(out.is_empty());CURRENT.with(|c|*c.borrow_mut()=None);}}
