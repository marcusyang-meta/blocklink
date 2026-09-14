"""Probe documented public STUN/TURN transports without credentials or allocations."""
import concurrent.futures,json,os,pathlib,socket,ssl,struct,time

def probe(target):
 host,port,transport=target
 result=dict(host=host,port=port,transport=transport)
 start=time.monotonic()
 try:
  if transport=='udp':
   addresses=socket.getaddrinfo(host,port,socket.AF_INET,socket.SOCK_DGRAM)
   addr=addresses[0][4]
   with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as sock:
    sock.settimeout(3)
    transaction=os.urandom(12)
    message=struct.pack('!HHI',1,0,0x2112a442)+transaction
    for attempt in range(3):
     sock.sendto(message,addr)
     try:
      response,_=sock.recvfrom(2048)
      assert len(response)>=20 and response[8:20]==transaction
      assert struct.unpack('!H',response[:2])[0]==0x101
      result['result']='binding response received';break
     except socket.timeout:
      if attempt==2:raise
  else:
   with socket.create_connection((host,port),timeout=5) as sock:
    with ssl.create_default_context().wrap_socket(sock,server_hostname=host):
     result['result']='TLS handshake verified'
 except Exception as e:result['error']=type(e).__name__+': '+str(e)
 result['seconds']=round(time.monotonic()-start,2)
 return result

targets=[(host,port,'udp') for host in ('stun.cloudflare.com','turn.cloudflare.com') for port in (3478,53)]
targets += [('turn.cloudflare.com',port,'tls') for port in (443,5349)]
with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:results=list(pool.map(probe,targets))
pathlib.Path('cloud-checks').mkdir(exist_ok=True)
pathlib.Path('cloud-checks/network.json').write_text(json.dumps(results,indent=2))
print(json.dumps(results,indent=2))
