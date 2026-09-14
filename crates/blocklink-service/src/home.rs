use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::time::{SystemTime, UNIX_EPOCH};

fn millis(t: SystemTime) -> u64 { t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }

// Only serve bounded PNG files inside this game's directory, never caller supplied paths.
fn png(game: &Path, file: &Path, limit: u64) -> Option<String> {
    let base=game.canonicalize().ok()?;
    let file=file.canonicalize().ok()?;
    if !file.starts_with(&base) {return None}
    let mut bytes=Vec::new();
    fs::File::open(file).ok()?.take(limit+1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64>limit || bytes.len()<24 || &bytes[..8]!=b"\x89PNG\r\n\x1a\n" || &bytes[12..16]!=b"IHDR" {return None}
    let w=u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h=u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if w==0||h==0||w>8192||h>8192||u64::from(w)*u64::from(h)>40_000_000 {return None}
    Some(format!("data:image/png;base64,{}",STANDARD.encode(bytes)))
}

fn screenshots(game: &Path) -> Vec<(u64, PathBuf)> {
    let mut files=Vec::new();
    let Ok(base)=game.canonicalize() else {return files};
    let Ok(dir)=game.join("screenshots").canonicalize() else {return files};
    if !dir.starts_with(base) {return files}
    if let Ok(entries)=fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p=entry.path();
            if !p.extension().is_some_and(|e|e.eq_ignore_ascii_case("png")) {continue}
            if let Ok(m)=entry.metadata() {if m.is_file() && m.len()<=8*1024*1024 {
                files.push((m.modified().map(millis).unwrap_or(0),p));
            }}
        }
    }
    files.sort_by(|a,b|b.0.cmp(&a.0).then_with(||b.1.cmp(&a.1)));
    files
}

pub(super) fn record(engine: &Engine,id: &str) -> Result<()> {
    write_json(&engine.ws.instance_dir(id)?.join("last-played.json"),&json!({"launchedAt":millis(SystemTime::now())}))
}

pub(super) fn summary(engine: &Engine) -> Result<Value> {
    let mut items=Vec::new();
    for i in engine.ws.instances()? {
        let id=&i.instance_id;
        if engine.config(id)["server"]==true {continue}
        let dir=engine.ws.instance_dir(id)?;
        let game=dir.join("game");
        let worlds=worlds::list(engine,id).unwrap_or(json!({"worlds":[]}));
        let world=worlds["worlds"].as_array().and_then(|v|v.iter().filter(|w|w["lastPlayed"].as_u64().unwrap_or(0)>0).max_by_key(|w|w["lastPlayed"].as_u64().unwrap_or(0)));
        let cover=world.and_then(|w|w["path"].as_str()).and_then(|p|png(&game,&Path::new(p).join("icon.png"),256*1024));
        let shots=screenshots(&game);
        let art_key=shots.first().map(|(t,p)|format!("{t}:{}",p.file_name().unwrap_or_default().to_string_lossy()));
        let launched=read_json(&dir.join("last-played.json")).ok().and_then(|v|v["launchedAt"].as_u64());
        items.push(json!({"id":id,"lastLaunchedAt":launched,"world":world.map(|w|json!({"name":w["name"],"lastPlayed":w["lastPlayed"]})),"cover":cover,"artKey":art_key}));
    }
    Ok(json!({"items":items}))
}

pub(super) fn artwork(engine: &Engine,id: &str) -> Result<Value> {
    engine.ws.instance(id)?;
    let game=engine.ws.instance_dir(id)?.join("game");
    for (_,file) in screenshots(&game).into_iter().take(10) {
        if let Some(image)=png(&game,&file,8*1024*1024) {return Ok(json!({"image":image}))}
    }
    Ok(json!({"image":null}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn png_is_bounded_and_cannot_escape_game_directory()->Result<()> {
        let root=tempfile::tempdir()?;let game=root.path().join("game");fs::create_dir(&game)?;
        let mut bytes=b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();bytes.extend(64u32.to_be_bytes());bytes.extend(64u32.to_be_bytes());
        fs::write(game.join("icon.png"),&bytes)?;fs::write(root.path().join("outside.png"),&bytes)?;
        assert!(png(&game,&game.join("icon.png"),1024).is_some());
        assert!(png(&game,&game.join("icon.png"),8).is_none());
        assert!(png(&game,&root.path().join("outside.png"),1024).is_none());
        bytes[16..20].copy_from_slice(&9000u32.to_be_bytes());fs::write(game.join("icon.png"),bytes)?;
        assert!(png(&game,&game.join("icon.png"),1024).is_none());Ok(())
    }
    #[test] fn history_survives_reload_and_missing_art_is_optional()->Result<()> {
        let temp=tempfile::tempdir()?;let e=Engine::new(temp.path())?;
        let i:Instance=serde_json::from_value(json!({"schemaVersion":1,"instanceId":uuid::Uuid::new_v4().to_string(),"name":"QA","minecraft":"1.21.4","loader":{"kind":"vanilla"},"runtime":{"java":"auto","memoryMiB":4096},"storage":{"linkMode":"auto"},"mods":[]}))?;
        e.ws.create_instance(&i)?;
        assert!(summary(&e)?["items"][0]["lastLaunchedAt"].is_null());
        record(&e,&i.instance_id)?;drop(e);let e=Engine::new(temp.path())?;
        assert!(summary(&e)?["items"][0]["lastLaunchedAt"].as_u64().unwrap()>0);
        assert!(artwork(&e,&i.instance_id)?["image"].is_null());Ok(())
    }
}
