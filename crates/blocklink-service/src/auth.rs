use crate::net::*;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
/// Public identifier of Blocklink's own desktop application (not a secret).
pub fn product_client_id() -> &'static str {
    option_env!("BLOCKLINK_MICROSOFT_CLIENT_ID")
        .unwrap_or("a8080fd5-fcd6-4dbf-811f-91d0f10759c9")
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn entry(root: &Path) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(
        "app.blocklink.launcher",
        &format!("minecraft-{}", root.display()),
    )?)
}
pub fn start(client_id: &str) -> Result<Value> {
    if uuid::Uuid::parse_str(client_id).is_err(){bail!("微软登录暂未开放，请先使用本地玩家")}
    let response=client()?
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .form(&[
            ("client_id", client_id),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()?;
    let status=response.status();let body:Value=response.json()?;
    if !status.is_success(){bail!("{}",oauth_error(&body))}
    Ok(body)
}
fn oauth_error(body:&Value)->&'static str{
    match body["error"].as_str().unwrap_or("") {
        "authorization_declined"|"access_denied"=>"已取消微软登录，可以重新尝试",
        "expired_token"=>"登录代码已过期，请重新获取",
        "invalid_client"|"unauthorized_client"=>"Blocklink 的微软登录配置暂不可用，请联系开发者",
        "invalid_grant"=>"登录已过期，请重新登录",
        _=>"微软登录暂时不可用，请稍后重试",
    }
}
fn auth_response(response:reqwest::blocking::Response,stage:&str)->Result<Value>{
    let status=response.status();let body:Value=response.json()?;
    if !status.is_success(){
        if stage=="游戏档案"&&status.as_u16()==404{bail!("此账户还没有 Minecraft Java 版游戏档案，请确认已购买并创建游戏角色")}
        if stage=="Minecraft"&&status.as_u16()==403{bail!("Minecraft 拒绝了登录请求，Blocklink 的应用访问资格需要开发者确认")}
        if stage=="Xbox"{match body["XErr"].as_u64(){Some(2148916233)=>bail!("请先在 Xbox 官网创建玩家档案，再回来登录"),Some(2148916238)=>bail!("此账户需要家长在微软家庭设置中允许游戏访问"),_=>{}}}
        bail!("{stage} 登录暂时不可用，请稍后重试（{}）",status.as_u16())
    }
    Ok(body)
}
pub fn poll(root: &Path, client_id: &str, code: &str) -> Result<Value> {
    let ms: Value = client()?
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("client_id", client_id),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", code),
        ])
        .send()?
        .json()?;
    if let Some(e) = ms["error"].as_str() {
        if e == "authorization_pending" || e == "slow_down" {
            return Ok(json!({"pending":true,"slow":e=="slow_down"}));
        }
        bail!("{}",oauth_error(&ms))
    }
    let account = exchange(client_id, ms)?;
    entry(root)?.set_password(&serde_json::to_string(&account)?)?;
    Ok(public(&account))
}
fn exchange(client_id: &str, ms: Value) -> Result<Value> {
    let c = client()?;
    let xbox:Value=auth_response(c.post("https://user.auth.xboxlive.com/user/authenticate").json(&json!({"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={}",field(&ms,"access_token")?)},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"})).send()?,"Xbox")?;
    let xsts:Value=auth_response(c.post("https://xsts.auth.xboxlive.com/xsts/authorize").json(&json!({"Properties":{"SandboxId":"RETAIL","UserTokens":[xbox["Token"]]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"})).send()?,"Xbox")?;
    let mc:Value=auth_response(c.post("https://api.minecraftservices.com/authentication/login_with_xbox").json(&json!({"identityToken":format!("XBL3.0 x={};{}",field(&xsts["DisplayClaims"]["xui"][0],"uhs")?,field(&xsts,"Token")?)})).send()?,"Minecraft")?;
    let profile: Value = auth_response(c
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(field(&mc, "access_token")?)
        .send()?
        ,"游戏档案")?;
    field(&profile,"id")?;field(&profile,"name")?;field(&mc,"access_token")?;
    Ok(
        json!({"id":profile["id"],"name":profile["name"],"token":mc["access_token"],"refresh":ms["refresh_token"],"clientId":client_id,"expires":now()+mc["expires_in"].as_u64().unwrap_or(3600)}),
    )
}
pub fn account(root: &Path) -> Result<Value> {
    let e = entry(root)?;
    let mut a: Value = serde_json::from_str(
        &e.get_password()
            .map_err(|_| anyhow::anyhow!("请先登录拥有 Minecraft Java 版的微软账户"))?,
    )?;
    if a["expires"].as_u64().unwrap_or(0) < now() + 120 {
        let ms: Value = client()?
            .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
            .form(&[
                ("client_id", field(&a, "clientId")?),
                ("grant_type", "refresh_token"),
                ("refresh_token", field(&a, "refresh")?),
                ("scope", "XboxLive.signin offline_access"),
            ])
            .send()?
            .error_for_status()?
            .json()?;
        if !ms["error"].is_null() {
            bail!("登录已过期，请重新登录")
        }
        a = exchange(field(&a, "clientId")?, ms)?;
        e.set_password(&serde_json::to_string(&a)?)?;
    }
    Ok(a)
}
pub fn public(a: &Value) -> Value {
    json!({"name":a["name"],"id":a["id"],"expires":a["expires"],"offline":a["offline"] == true})
}
pub fn offline(name: &str) -> Result<Value> {
    if name.is_empty()
        || name.len() > 16
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        bail!("离线名称需要 1–16 位英文字母、数字或下划线")
    }
    // Match Java UUID.nameUUIDFromBytes("OfflinePlayer:" + name), including case.
    let mut bytes = md5::compute(format!("OfflinePlayer:{name}")).0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(
        json!({"name":name,"id":uuid::Uuid::from_bytes(bytes).simple().to_string(),"token":"0","offline":true}),
    )
}

pub fn status(root: &Path) -> Value {
    entry(root)
        .and_then(|e| Ok(serde_json::from_str::<Value>(&e.get_password()?)?))
        .map(|v| public(&v))
        .unwrap_or(Value::Null)
}
pub fn logout(root: &Path) -> Result<()> {
    match entry(root)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod offline_tests {
    use super::*;
    #[test]
    fn matches_java_identity_and_rejects_invalid_names() {
        assert_eq!(
            offline("Notch").unwrap()["id"],
            "b50ad385829d3141a2167e7d7539ba7f"
        );
        assert_ne!(
            offline("Notch").unwrap()["id"],
            offline("notch").unwrap()["id"]
        );
        for name in ["", "a b", "../user", "玩家", "abcdefghijklmnopq"] {
            assert!(offline(name).is_err());
        }
    }
}
