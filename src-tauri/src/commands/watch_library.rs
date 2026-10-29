//! Fixed-origin, account-scoped adapter for the watch library service.
use reqwest::{header::{COOKIE, SET_COOKIE}, Method};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::OnceLock};
use tokio::sync::Mutex;

pub mod uploads;
const BASE: &str = "https://library.harmonywork.com.cn:8443";
static SESSION: OnceLock<Mutex<Option<(u64, String)>>> = OnceLock::new();

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRequest {
    resource: String,
    id: Option<String>,
    #[serde(default)]
    query: BTreeMap<String, String>,
    body: Option<Value>,
}

fn endpoint(resource: &str, id: Option<&str>) -> Result<(Method, String), String> {
    let ident = || -> Result<&str, String> {
        let value = id.unwrap_or("");
        if value.is_empty() || value.len() > 128 || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err("资料标识无效".into());
        }
        Ok(value)
    };
    let (method, path) = match resource {
        "stats" => (Method::GET, "/api/stats".into()),
        "series" => (Method::GET, "/api/series".into()),
        "videos" => (Method::GET, "/api/videos".into()),
        "video" => (Method::GET, format!("/api/video/{}", ident()?)),
        "edit" => (Method::PATCH, format!("/api/video/{}", ident()?)),
        "profiles" => (Method::GET, "/api/profiles".into()),
        "profile" => (Method::GET, format!("/api/profiles/{}", ident()?)),
        "task" => (Method::GET, format!("/api/tasks/{}", ident()?)),
        "scan-status" => (Method::GET, "/api/scan/status".into()),
        "ai-status" => (Method::GET, "/api/ai/status".into()),
        "scan" => (Method::POST, "/api/scan".into()),
        "analyze" => (Method::POST, "/api/ai/analyze".into()),
        "match-text" => (Method::POST, "/api/match/text".into()),
        "uploads-config" => (Method::GET, "/api/uploads/config".into()),
        "uploads" => (Method::GET, "/api/uploads".into()),
        "upload-analyze" => (Method::POST, "/api/uploads/analyze".into()),
        _ => return Err("不支持的资料库操作".into()),
    };
    Ok((method, path))
}

pub(super) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(90)).build().map_err(|_| "无法初始化资料库连接".into())
}

pub(super) async fn session() -> Result<(u64, String), String> {
    let (generation, cookie) = super::marketing::cookie_snapshot().await;
    let cookie = cookie.ok_or("请先登录 Harmony Work 营销用户账号")?;
    let mut stored = SESSION.get_or_init(|| Mutex::new(None)).lock().await;
    if let Some((old, value)) = &*stored {
        if *old == generation { return Ok((generation, value.clone())); }
    }
    *stored = None;
    let token = cookie.split(';').map(str::trim).find_map(|part| part.strip_prefix("portal_token="))
        .filter(|v| !v.is_empty()).ok_or("客户端登录凭据无效，请重新登录")?;
    let response = client()?.post(format!("{BASE}/api/auth/desktop")).bearer_auth(token)
        .send().await.map_err(|_| "无法连接腕表资料库，请检查网络".to_string())?;
    let status = response.status();
    if !status.is_success() { return Err(format!("资料库登录失败（{status}），请检查账号权限或重试")); }
    let value = response.headers().get_all(SET_COOKIE).iter().filter_map(|x| x.to_str().ok())
        .filter_map(|x| x.split(';').next()).find(|x| x.starts_with("watch_session="))
        .ok_or("资料库未返回有效登录状态")?.to_string();
    super::marketing::ensure_current_session(generation)?;
    *stored = Some((generation, value.clone()));
    Ok((generation, value))
}

pub(super) async fn send(method: Method, path: &str, query: &BTreeMap<String,String>, body: Option<&Value>) -> Result<Value,String> {
    let (generation, cookie) = session().await?;
    send_snapshot(method,path,query,body,generation,&cookie).await
}

pub(super) async fn send_snapshot(method: Method, path: &str, query: &BTreeMap<String,String>, body: Option<&Value>, generation:u64, cookie:&str) -> Result<Value,String> {
    super::marketing::ensure_current_session(generation)?;
    let mut req = client()?.request(method,format!("{BASE}{path}")).header(COOKIE,cookie).query(query);
    if let Some(body) = body { req = req.json(body); }
    let response = req.send().await.map_err(|_| "资料库连接中断，请稍后重试".to_string())?;
    super::marketing::ensure_current_session(generation)?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        let mut stored=SESSION.get_or_init(|| Mutex::new(None)).lock().await;
        if stored.as_ref().is_some_and(|(current,_)|*current==generation) {*stored=None;}
        return Err(format!("资料库登录已失效或没有权限（{status}）"));
    }
    if !status.is_success() { return Err(format!("资料库请求失败（{status}）")); }
    let value = response.json().await.map_err(|_| "资料库返回格式无效".to_string())?;
    super::marketing::ensure_current_session(generation)?;
    Ok(value)
}

#[tauri::command]
pub async fn watch_library_request(request: LibraryRequest) -> Result<Value, String> {
    let (method, path) = endpoint(&request.resource, request.id.as_deref())?;
    send(method, &path, &request.query, request.body.as_ref()).await
}

#[tauri::command]
pub async fn watch_library_media(asset_id: u64, kind: String) -> Result<Value,String> {
    if asset_id == 0 || !["cover", "play", "download"].contains(&kind.as_str()) { return Err("媒体请求无效".into()); }
    let mut value = send(Method::POST,"/api/desktop/media",&BTreeMap::new(),Some(&json!({"asset_id":asset_id,"kind":kind}))).await?;
    let path = value["url"].as_str().ok_or("媒体地址无效")?;
    if !(path.starts_with("/api/desktop/media/") || path.starts_with("/api/desktop/hls/")) || path.contains("..") || path.contains(['\\','?','#']) {
        return Err("媒体地址不受信任".into());
    }
    value["url"] = json!(format!("{BASE}{path}"));
    Ok(value)
}

#[tauri::command]
pub async fn watch_library_image_query(file_path: String) -> Result<Value, String> {
    let (generation, cookie) = session().await?;
    let meta = tokio::fs::metadata(&file_path).await.map_err(|_| "无法读取图片".to_string())?;
    if !meta.is_file() || meta.len() > 8*1024*1024 { return Err("图片不能超过 8 MB".into()); }
    let extension = std::path::Path::new(&file_path).extension().and_then(|x|x.to_str()).unwrap_or("").to_lowercase();
    let mime = match extension.as_str() { "jpg"|"jpeg"=>"image/jpeg", "png"=>"image/png", "webp"=>"image/webp", _=>return Err("仅支持 JPG、PNG、WebP".into()) };
    let bytes = tokio::fs::read(&file_path).await.map_err(|_| "无法读取图片".to_string())?;
    if bytes.len() > 8*1024*1024 { return Err("图片不能超过 8 MB".into()); }
    let part = reqwest::multipart::Part::bytes(bytes).file_name(format!("query.{extension}")).mime_str(mime).map_err(|e|e.to_string())?;
    let response = client()?.post(format!("{BASE}/api/match/image")).header(COOKIE,cookie)
        .multipart(reqwest::multipart::Form::new().part("file",part)).send().await.map_err(|_| "图片查询连接失败".to_string())?;
    super::marketing::ensure_current_session(generation)?;
    if response.status().as_u16()==401 || response.status().as_u16()==403 {
        let mut stored=SESSION.get_or_init(||Mutex::new(None)).lock().await;
        if stored.as_ref().is_some_and(|(current,_)|*current==generation) {*stored=None;}
        return Err("资料库登录已失效或没有权限，请重试".into());
    }
    if !response.status().is_success() { return Err(format!("图片检索失败（{}）",response.status())); }
    response.json().await.map_err(|_| "图片查询返回格式无效".into())
}

#[tauri::command]
pub async fn watch_library_prepare_asset(asset_id:u64, workspace_root:Option<String>, state:tauri::State<'_,crate::store::AppState>) -> Result<Value,String> {
    use tokio::io::AsyncWriteExt;
    let generation=super::marketing::cookie_snapshot().await.0;
    let configured=state.settings.lock().await.workspace_root.clone();
    let root=tokio::fs::canonicalize(&configured).await.map_err(|_|"工作区不存在，请先在设置中选择有效工作区".to_string())?;
    if let Some(requested)=workspace_root.filter(|s|!s.is_empty()) {
        if tokio::fs::canonicalize(requested).await.map_err(|_|"工作区路径无效".to_string())?!=root {return Err("请使用当前已配置的工作区".into());}
    }
    let meta=send(Method::GET,&format!("/api/video/{asset_id}"),&BTreeMap::new(),None).await?;
    let filename=meta["filename"].as_str().ok_or("素材文件名无效")?;
    let safe=std::path::Path::new(filename).file_name().and_then(|s|s.to_str()).filter(|v|*v==filename&&!v.contains(['\\',':'])&&*v!="."&&*v!="..").ok_or("素材文件名无效")?;
    let grant=watch_library_media(asset_id,"download".into()).await?;
    let mut response=client()?.get(grant["url"].as_str().ok_or("下载地址无效")?).send().await.map_err(|_|"无法下载素材".to_string())?;
    if !response.status().is_success(){return Err(format!("下载失败（{}）",response.status()));}
    let mime=response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|x|x.to_str().ok()).unwrap_or("application/octet-stream").to_string();
    let dir=root.join("watch-library");tokio::fs::create_dir_all(&dir).await.map_err(|_|"无法创建素材目录".to_string())?;
    let dir=tokio::fs::canonicalize(dir).await.map_err(|_|"素材目录不可用".to_string())?;
    if !dir.starts_with(&root){return Err("素材目录不能指向工作区外部".into());}
    let name=format!("{}-{safe}",uuid::Uuid::new_v4());let path=dir.join(&name);
    let mut file=tokio::fs::OpenOptions::new().write(true).create_new(true).open(&path).await.map_err(|_|"无法创建素材文件".to_string())?;
    let result=async {
        let mut total=0_u64;
        while let Some(bytes)=response.chunk().await.map_err(|_|"下载连接中断".to_string())? {
            super::marketing::ensure_current_session(generation)?;
            total+=bytes.len() as u64;if total>20*1024*1024*1024{return Err("文件超过 20 GB 下载限制".to_string());}
            file.write_all(&bytes).await.map_err(|_|"工作区写入失败".to_string())?;
        }
        file.flush().await.map_err(|_|"工作区写入失败".to_string())?;
        super::marketing::ensure_current_session(generation)?;
        Ok::<(),String>(())
    }.await;
    drop(file);
    if let Err(error)=result{let _=tokio::fs::remove_file(&path).await;return Err(error);}
    Ok(json!({"localPath":path.to_string_lossy(),"fileName":name,"mediaType":mime}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_surface_is_allowlisted() {
        assert!(endpoint("https://evil.test",None).is_err());
        assert!(endpoint("profile",Some("../auth/logout")).is_err());
        assert!(endpoint("video",Some("1?key=x")).is_err());
        assert_eq!(endpoint("video",Some("23")).unwrap().1,"/api/video/23");
        assert_eq!(endpoint("match-text",None).unwrap().0,Method::POST);
    }
    #[tokio::test]
    #[ignore = "uses the signed-in local account and live company media service"]
    async fn live_signed_in_media_stream() {
        let stats=send(Method::GET,"/api/stats",&BTreeMap::new(),None).await.expect("live stats");
        assert!(stats["count"].as_u64().unwrap_or(0)>0);
        let videos=send(Method::GET,"/api/videos",&BTreeMap::from([("limit".into(),"1".into())]),None).await.expect("live videos");
        let id=videos[0]["id"].as_u64().expect("asset ID");
        let grant=watch_library_media(id,"play".into()).await.expect("media grant");
        let url=grant["url"].as_str().expect("media URL");
        let response=client().unwrap().get(url).send().await.expect("media response");
        assert!(response.status().is_success(),"media status {}",response.status());
        let media_type=response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v|v.to_str().ok()).unwrap_or("").to_string();
        if media_type.contains("mpegurl") {
            let playlist=response.text().await.unwrap();
            assert!(playlist.starts_with("#EXTM3U"));
            let path=playlist.lines().find(|x|!x.is_empty()&&!x.starts_with('#')).expect("media playlist");
            assert!(path.starts_with("/api/desktop/hls/"));
            let segment_playlist=client().unwrap().get(format!("{BASE}{path}")).send().await.unwrap();
            assert!(segment_playlist.status().is_success());
        }
    }
}
