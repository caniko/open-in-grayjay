use base64::{Engine, engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD}};
use serde::{Deserialize, Serialize};
use std::{env, error::Error, fs::{self, OpenOptions}, io::{self, Read, Write}, net::{SocketAddr, TcpListener, TcpStream}, os::unix::fs::OpenOptionsExt, path::PathBuf, process::{Command, Stdio}, thread, time::{Duration, Instant}};
use snow::{Builder, TransportState};
use url::Url;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const IK: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2b";
const N: &str = "Noise_N_25519_ChaChaPoly_BLAKE2b";
const MAX: usize = 65535;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct State { private_key: String, public_key: String, remote_public_key: String, port: u16 }
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device { public_key: String, addresses: Vec<String>, port: u16, pairing_code: String }
fn path() -> Result<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| env::var_os("HOME").map(|v| PathBuf::from(v).join(".config"))).ok_or("HOME not set")?;
    Ok(base.join("open-in-grayjay/state.json"))
}
fn save(path: &std::path::Path, state: &State) -> Result<()> {
    let dir = path.parent().ok_or("invalid state path")?;
    fs::create_dir_all(dir)?;
    let mut random = [0u8; 8]; getrandom::getrandom(&mut random)?;
    let tmp = dir.join(format!(".state-{}", u64::from_ne_bytes(random)));
    let result = (|| -> Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        serde_json::to_writer(&mut f, state)?; f.sync_all()?; fs::rename(&tmp, path)?; Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(tmp); }
    result
}
fn keypair() -> Result<(Vec<u8>, Vec<u8>)> {
    let kp = Builder::new(IK.parse()?).generate_keypair()?; Ok((kp.private, kp.public))
}
fn frame_write(w: &mut impl Write, bytes: &[u8]) -> Result<()> { w.write_all(&(bytes.len() as u32).to_le_bytes())?; w.write_all(bytes)?; Ok(()) }
fn frame_read(r: &mut impl Read, max: usize) -> Result<Vec<u8>> {
    let mut l = [0; 4]; r.read_exact(&mut l)?; let n = u32::from_le_bytes(l) as usize;
    if n == 0 || n > max { return Err("invalid frame length".into()); }
    let mut b = vec![0; n]; r.read_exact(&mut b)?; Ok(b)
}
fn version(s: &mut TcpStream) -> Result<()> {
    s.write_all(&5u32.to_le_bytes())?; let mut b = [0; 4]; s.read_exact(&mut b)?;
    if u32::from_le_bytes(b) < 4 { return Err("unsupported Sync version".into()); } Ok(())
}
fn send(s: &mut TcpStream, t: &mut TransportState, op: u8, sub: u8, data: &[u8]) -> Result<()> {
    let mut p = Vec::new(); p.extend_from_slice(&((data.len()+3) as u32).to_le_bytes()); p.extend_from_slice(&[op, sub, 0]); p.extend_from_slice(data);
    if p.len()+16 > MAX { return Err("Sync packet too large".into()); }
    let mut b = vec![0; p.len()+16]; let n = t.write_message(&p, &mut b)?; frame_write(s, &b[..n])
}
fn receive(s: &mut TcpStream, t: &mut TransportState) -> Result<(u8,u8,Vec<u8>)> {
    let b = frame_read(s, MAX)?; let mut p = vec![0; b.len()]; let n = t.read_message(&b, &mut p)?; p.truncate(n);
    if p.len() < 7 || u32::from_le_bytes(p[..4].try_into()?) as usize != p.len()-4 || p[6] != 0 { return Err("invalid Sync packet".into()); }
    Ok((p[4],p[5],p[7..].to_vec()))
}
fn authorize(s: &mut TcpStream, t: &mut TransportState) -> Result<()> {
    let mut id = [0u8;16]; getrandom::getrandom(&mut id)?;
    let id = format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}", id[0],id[1],id[2],id[3],id[4],id[5],id[6],id[7],id[8],id[9],id[10],id[11],id[12],id[13],id[14],id[15]);
    let name = b"open-in-grayjay"; let mut data = vec![id.len() as u8]; data.extend_from_slice(id.as_bytes()); data.push(name.len() as u8); data.extend_from_slice(name);
    send(s,t,2,0,&data)?;
    loop { let (op,sub,_) = receive(s,t)?; match (op,sub) { (2,0) => return Ok(()), (2,1) => return Err("Unauthorized by Grayjay; re-pair".into()), (0,_) => send(s,t,1,0,&[])?, _ => {} } }
}
fn responder(mut s: TcpStream, private: &[u8], code: &str) -> Result<Vec<u8>> {
    s.set_read_timeout(Some(Duration::from_secs(30)))?; s.set_write_timeout(Some(Duration::from_secs(30)))?; version(&mut s)?;
    let b = frame_read(&mut s, 512)?; if b.len()<8 { return Err("short handshake".into()); }
    let n = u32::from_le_bytes(b[4..8].try_into()?) as usize; if n == 0 || n > 128 || 8+n > b.len() { return Err("invalid pairing message".into()); }
    let mut noise = Builder::new(N.parse()?).local_private_key(private).build_responder()?;
    let mut plain = [0;512]; let size = noise.read_message(&b[8..8+n], &mut plain)?;
    if &plain[..size] != code.as_bytes() { return Err("incorrect pairing code".into()); }
    let mut noise = Builder::new(IK.parse()?).local_private_key(private).build_responder()?;
    noise.read_message(&b[8+n..], &mut plain)?; let remote = noise.get_remote_static().ok_or("missing remote key")?.to_vec();
    let mut out = [0;512]; let len = noise.write_message(&[], &mut out)?; frame_write(&mut s,&out[..len])?;
    let mut t = noise.into_transport_mode()?; authorize(&mut s,&mut t)?; Ok(remote)
}
fn initiator(mut s: TcpStream, state: &State, url: &str, position: u64) -> Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(10)))?; s.set_write_timeout(Some(Duration::from_secs(10)))?; version(&mut s)?;
    let private = STANDARD.decode(&state.private_key)?; let remote = STANDARD.decode(&state.remote_public_key)?;
    let mut noise = Builder::new(IK.parse()?).local_private_key(&private).remote_public_key(&remote).build_initiator()?;
    let mut b = [0;512]; let n = noise.write_message(&[], &mut b)?; let mut hello = vec![0;8]; hello.extend_from_slice(&b[..n]); frame_write(&mut s,&hello)?;
    let reply = frame_read(&mut s,512).map_err(|e| format!("Grayjay rejected handshake; re-pair: {e}"))?; noise.read_message(&reply,&mut b)?;
    let mut t = noise.into_transport_mode()?; authorize(&mut s,&mut t)?;
    let data = serde_json::to_vec(&serde_json::json!({"url":url,"position":position}))?; send(&mut s,&mut t,4,101,&data)?; Ok(())
}
fn pairing_link(d: &Device) -> Result<String> { Ok(format!("grayjay://sync/{}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(d)?))) }
fn pair(addr: &str) -> Result<()> {
    let p = path()?; let (private, public) = match fs::read(&p) { Ok(b) => { let s: State = serde_json::from_slice(&b)?; (STANDARD.decode(s.private_key)?,STANDARD.decode(s.public_key)?) }, Err(e) if e.kind()==io::ErrorKind::NotFound => keypair()?, Err(e) => return Err(e.into()) };
    let listener = TcpListener::bind(addr)?; let a = listener.local_addr()?;
    let ip = if a.ip().is_unspecified() { "127.0.0.1".to_string() } else { a.ip().to_string() };
    let mut random = [0;8]; getrandom::getrandom(&mut random)?; let code = URL_SAFE_NO_PAD.encode(random);
    let d = Device { public_key: STANDARD.encode(&public), addresses: vec![ip], port: a.port(), pairing_code: code.clone() };
    println!("Paste into Grayjay Desktop → Sync → add device manually:\n{}",pairing_link(&d)?);
    listener.set_nonblocking(true)?; let deadline = Instant::now()+Duration::from_secs(300);
    loop { match listener.accept() { Ok((stream,_)) => { let remote = responder(stream,&private,&code)?; save(&p,&State { private_key: STANDARD.encode(private), public_key: STANDARD.encode(public), remote_public_key: STANDARD.encode(remote), port:12315 })?; println!("Paired."); return Ok(()); }, Err(e) if e.kind()==io::ErrorKind::WouldBlock && Instant::now()<deadline => thread::sleep(Duration::from_millis(100)), Err(e) if e.kind()==io::ErrorKind::WouldBlock => return Err("Pairing timed out".into()), Err(e) => return Err(e.into()) } }
}
fn seconds(s: &str) -> Result<u64> {
    if let Ok(n) = s.parse() { return Ok(n); }
    let mut total = 0u64; let mut num = 0u64; let mut digits = false; let mut last = 4;
    for c in s.chars() { if let Some(d) = c.to_digit(10) { num = num.checked_mul(10).and_then(|n| n.checked_add(d as u64)).ok_or("timestamp overflow")?; digits=true; } else { let (rank, mult) = match c { 'h'=>(3,3600), 'm'=>(2,60), 's'=>(1,1), _=>return Err("invalid timestamp".into()) }; if !digits || rank>=last { return Err("invalid timestamp".into()); } total = total.checked_add(num.checked_mul(mult).ok_or("timestamp overflow")?).ok_or("timestamp overflow")?; num=0; digits=false; last=rank; } }
    if digits || last==4 { return Err("invalid timestamp".into()); } Ok(total)
}
fn youtube(input: &str) -> Result<(String,u64)> {
    let u = Url::parse(input)?; if !matches!(u.scheme(),"http"|"https") || !u.username().is_empty() || u.password().is_some() || u.port().is_some() { return Err("invalid YouTube URL".into()); }
    let host = u.host_str().ok_or("missing host")?; let parts: Vec<_> = u.path_segments().ok_or("invalid path")?.collect();
    let id = match (host, parts.as_slice()) { ("youtu.be",[id]) => Some((*id).to_string()), ("youtube.com"|"www.youtube.com"|"m.youtube.com"|"music.youtube.com", ["watch"]) => u.query_pairs().find(|(k,_)| k=="v").map(|(_,v)|v.into_owned()), ("youtube.com"|"www.youtube.com"|"m.youtube.com"|"music.youtube.com", ["shorts"|"live"|"embed",id]) => Some((*id).to_string()), _=>None }.ok_or("not a YouTube video URL")?;
    if id.len()!=11 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b==b'_' || b==b'-') { return Err("invalid YouTube video ID".into()); }
    let pos = u.query_pairs().find(|(k,_)| k=="t" || k=="start").map(|(_,v)| seconds(&v)).transpose()?.unwrap_or(0);
    if pos > i32::MAX as u64 { return Err("timestamp too large".into()); } Ok((format!("https://www.youtube.com/watch?v={id}"),pos))
}
fn open(input: &str, override_addr: Option<&str>) -> Result<()> {
    let (url,pos) = youtube(input)?; let state: State = serde_json::from_slice(&fs::read(path()?).map_err(|e| format!("Not paired; run open-in-grayjay pair: {e}"))?)?;
    let addr: SocketAddr = override_addr.map(str::to_owned).or_else(|| env::var("GRAYJAY_ADDR").ok()).unwrap_or_else(|| format!("127.0.0.1:{}",state.port)).parse()?;
    let connect = || TcpStream::connect_timeout(&addr,Duration::from_secs(1));
    let stream = match connect() { Ok(s)=>s, Err(_) => { let command = env::var_os("GRAYJAY_COMMAND").unwrap_or_else(|| "Grayjay".into()); Command::new(command).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("Cannot launch Grayjay: {e}. Enable Synchronization and Bind Listener in Grayjay settings"))?; let deadline=Instant::now()+Duration::from_secs(30); loop { match connect() { Ok(s)=>break s, Err(_) if Instant::now()<deadline=>thread::sleep(Duration::from_millis(500)), Err(e)=>return Err(format!("Grayjay Sync unavailable: {e}; enable Synchronization and Bind Listener in settings").into()) } } } };
    initiator(stream,&state,&url,pos)
}
fn native_read(r: &mut impl Read) -> Result<Vec<u8>> { let mut l=[0;4]; r.read_exact(&mut l)?; let n=u32::from_ne_bytes(l) as usize; if n>1024*1024 { return Err("native message exceeds 1 MiB".into()); } let mut b=vec![0;n]; r.read_exact(&mut b)?; Ok(b) }
fn native_write(w: &mut impl Write, b: &[u8]) -> Result<()> { w.write_all(&(b.len() as u32).to_ne_bytes())?; w.write_all(b)?; Ok(()) }
fn native() -> Result<()> {
    let response = (|| -> Result<()> { let b=native_read(&mut io::stdin())?; let v: serde_json::Value=serde_json::from_slice(&b)?; open(v.get("url").and_then(|v|v.as_str()).ok_or("missing url")?,None) })();
    let v=match response { Ok(())=>serde_json::json!({"ok":true}), Err(e)=>serde_json::json!({"ok":false,"error":e.to_string()}) }; let b=serde_json::to_vec(&v)?; native_write(&mut io::stdout().lock(),&b)
}
fn main() { let a:Vec<String>=env::args().collect(); let r=match a.get(1).map(String::as_str) { Some("pair") => { let addr=if a.get(2).map(String::as_str)==Some("--listen") { a.get(3).map(String::as_str).unwrap_or("") } else { "127.0.0.1:0" }; pair(addr) }, Some("open") => { if let Some(u)=a.get(2) { open(u,if a.get(3).map(String::as_str)==Some("--addr") {a.get(4).map(String::as_str)} else {None}) } else {Err("usage: open-in-grayjay open URL [--addr ADDR:PORT]".into())} }, _=>native() }; if let Err(e)=r { eprintln!("{e}"); std::process::exit(1); } }

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn urls() { for (input,id,t) in [("https://youtu.be/abcdefghijk?t=1m30s","abcdefghijk",90),("https://WWW.YouTube.com/watch?v=abcdefghijk&start=90s#x","abcdefghijk",90),("https://m.youtube.com/shorts/abcdefghijk","abcdefghijk",0)] { assert_eq!(youtube(input).unwrap(),(format!("https://www.youtube.com/watch?v={id}"),t)); } for u in ["https://youtube.com.evil/watch?v=abcdefghijk","https://youtube.com@evil/watch?v=abcdefghijk","https://youtu.be/short","ftp://youtu.be/abcdefghijk"] { assert!(youtube(u).is_err()); } }
    #[test] fn link() { let d=Device{public_key:"key".into(),addresses:vec!["127.0.0.1".into()],port:5,pairing_code:"123".into()}; let l=pairing_link(&d).unwrap(); let b=URL_SAFE_NO_PAD.decode(l.strip_prefix("grayjay://sync/").unwrap()).unwrap(); let decoded:Device=serde_json::from_slice(&b).unwrap(); assert_eq!(decoded.pairing_code,"123"); }
    #[test] fn frame() { let mut b=vec![]; frame_write(&mut b,b"hello").unwrap(); assert_eq!(frame_read(&mut &b[..],10).unwrap(),b"hello"); }
    #[test] fn native_frame() { let mut b=vec![]; native_write(&mut b,br#"{"url":"https://youtu.be/abcdefghijk"}"#).unwrap(); assert_eq!(native_read(&mut &b[..]).unwrap(),br#"{"url":"https://youtu.be/abcdefghijk"}"#); assert!(native_read(&mut &((1024u32*1024+1).to_ne_bytes())[..]).is_err()); }
    #[test] fn state_is_private() { use std::os::unix::fs::PermissionsExt; let mut r=[0;8]; getrandom::getrandom(&mut r).unwrap(); let dir=env::temp_dir().join(format!("open-in-grayjay-test-{}",u64::from_ne_bytes(r))); let p=dir.join("state.json"); let s=State { private_key:"private".into(),public_key:"public".into(),remote_public_key:"remote".into(),port:12315 }; save(&p,&s).unwrap(); let stored:State=serde_json::from_slice(&fs::read(&p).unwrap()).unwrap(); assert_eq!(stored.remote_public_key,"remote"); assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777,0o600); fs::remove_file(p).unwrap(); fs::remove_dir(dir).unwrap(); }
}

#[cfg(test)] mod desktop_tests {
    use super::*;
    fn read_frame(s: &mut TcpStream) -> Vec<u8> { let mut n=[0;4]; s.read_exact(&mut n).unwrap(); let mut b=vec![0;u32::from_le_bytes(n) as usize]; s.read_exact(&mut b).unwrap(); b }
    fn write_frame(s: &mut TcpStream, b: &[u8]) { s.write_all(&(b.len() as u32).to_le_bytes()).unwrap(); s.write_all(b).unwrap(); }
    fn check_version(s: &mut TcpStream) { s.write_all(&5u32.to_le_bytes()).unwrap(); let mut b=[0;4]; s.read_exact(&mut b).unwrap(); assert_eq!(u32::from_le_bytes(b),5); }
    fn packet_send(s: &mut TcpStream, t: &mut TransportState, op:u8, sub:u8, data:&[u8]) { let mut p=Vec::from(((data.len()+3) as u32).to_le_bytes()); p.extend_from_slice(&[op,sub,0]); p.extend_from_slice(data); let mut out=vec![0;p.len()+16]; let n=t.write_message(&p,&mut out).unwrap(); write_frame(s,&out[..n]); }
    fn packet_read(s: &mut TcpStream, t: &mut TransportState) -> Vec<u8> { let b=read_frame(s); let mut p=vec![0;b.len()]; let n=t.read_message(&b,&mut p).unwrap(); p.truncate(n); assert_eq!(u32::from_le_bytes(p[..4].try_into().unwrap()) as usize,p.len()-4); p }
    fn auth(s: &mut TcpStream,t: &mut TransportState) { let data=b"\x2401234567-89ab-cdef-0123-456789abcdef\x07Desktop"; packet_send(s,t,2,0,data); let p=packet_read(s,t); assert_eq!(&p[4..7], &[2,0,0]); assert_eq!(p[7],36); }
    #[test] fn desktop_initiator_pairs() {
        let (private,public)=keypair().unwrap(); let (desk_priv,desk_pub)=keypair().unwrap();
        let listener=TcpListener::bind("127.0.0.1:0").unwrap(); let addr=listener.local_addr().unwrap();
        let handle=thread::spawn(move || { let (s,_)=listener.accept().unwrap(); responder(s,&private,"abcd1234").unwrap() });
        let mut s=TcpStream::connect(addr).unwrap(); check_version(&mut s);
        let mut n=Builder::new(N.parse().unwrap()).remote_public_key(&public).build_initiator().unwrap(); let mut b=[0;512]; let len=n.write_message(b"abcd1234",&mut b).unwrap(); let mut first=vec![0;4]; first.extend_from_slice(&(len as u32).to_le_bytes()); first.extend_from_slice(&b[..len]);
        let mut ik=Builder::new(IK.parse().unwrap()).local_private_key(&desk_priv).remote_public_key(&public).build_initiator().unwrap(); let len=ik.write_message(&[],&mut b).unwrap(); first.extend_from_slice(&b[..len]); write_frame(&mut s,&first);
        let reply=read_frame(&mut s); ik.read_message(&reply,&mut b).unwrap(); let mut t=ik.into_transport_mode().unwrap(); auth(&mut s,&mut t); assert_eq!(handle.join().unwrap(),desk_pub);
    }
    #[test] fn desktop_responder_receives_video() {
        let (local_priv,local_pub)=keypair().unwrap(); let (desk_priv,desk_pub)=keypair().unwrap();
        let listener=TcpListener::bind("127.0.0.1:0").unwrap(); let addr=listener.local_addr().unwrap();
        let expected_pub=local_pub.clone();
        let handle=thread::spawn(move || { let (mut s,_)=listener.accept().unwrap(); check_version(&mut s); let first=read_frame(&mut s); assert_eq!(&first[..8], &[0;8]); let mut b=[0;512]; let mut ik=Builder::new(IK.parse().unwrap()).local_private_key(&desk_priv).build_responder().unwrap(); ik.read_message(&first[8..],&mut b).unwrap(); assert_eq!(ik.get_remote_static().unwrap(),expected_pub); let n=ik.write_message(&[],&mut b).unwrap(); write_frame(&mut s,&b[..n]); let mut t=ik.into_transport_mode().unwrap(); auth(&mut s,&mut t); let p=packet_read(&mut s,&mut t); assert_eq!(&p[4..7],&[4,101,0]); let value:serde_json::Value=serde_json::from_slice(&p[7..]).unwrap(); assert_eq!(value,serde_json::json!({"url":"https://www.youtube.com/watch?v=abcdefghijk","position":90})); });
        let state=State { private_key:STANDARD.encode(local_priv),public_key:STANDARD.encode(local_pub),remote_public_key:STANDARD.encode(desk_pub),port:addr.port() }; initiator(TcpStream::connect(addr).unwrap(),&state,"https://www.youtube.com/watch?v=abcdefghijk",90).unwrap(); handle.join().unwrap();
    }
}
