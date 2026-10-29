use super::*;
use serde::{Serialize, Deserialize};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};
use tokio::{io::{AsyncReadExt, AsyncSeekExt}, sync::Semaphore};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct UploadTask {
    id: String, file_name: String, source: String, folder: String, size: u64,
    uploaded: u64, status: String, error: Option<String>, bytes_per_second: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Entry { task: UploadTask, owner: String, #[serde(default)] portal_owner: Option<String>, path: PathBuf, modified: u64, fingerprint: Option<String>, remote: Option<String>, #[serde(default)] idempotent_create:bool, #[serde(default)] revision:u64 }

struct PortalAccount { id: String, email: String }
impl PortalAccount {
    fn owns(&self, source: &str, owner: &str) -> bool {
        match source { "marketing" => self.id == owner, "watch" => self.email.eq_ignore_ascii_case(owner), _ => false }
    }
    fn owns_entry(&self, entry:&Entry) -> bool {
        self.owns(&entry.task.source,&entry.owner) && entry.portal_owner.as_deref()==Some(self.id.as_str())
    }
}
async fn portal_snapshot() -> Result<(PortalAccount,u64,String),String> {
    let (generation,cookie) = super::super::marketing::cookie_snapshot().await;
    let cookie = cookie.ok_or("请先登录营销账号")?;
    let response = client()?.get(format!("{}/api/auth/me", super::super::marketing::marketing_base_url()))
        .header(COOKIE,&cookie).send().await.map_err(|_| "无法验证营销账号".to_string())?;
    super::super::marketing::ensure_current_session(generation)?;
    if !response.status().is_success() { return Err("营销账号验证失败".into()); }
    let value:Value = response.json().await.map_err(|_| "营销账号信息无效".to_string())?;
    super::super::marketing::ensure_current_session(generation)?;
    let user=&value["user"];
    let id=user["id"].as_str().filter(|s|!s.is_empty()).ok_or("营销账号标识无效")?.to_owned();
    let email=user["email"].as_str().filter(|s|!s.is_empty()).ok_or("营销账号邮箱无效")?.trim().to_owned();
    Ok((PortalAccount{id,email},generation,cookie))
}
pub(crate) fn marketing_origin(raw:&str) -> Result<reqwest::Url,String> {
    let url=reqwest::Url::parse(raw).map_err(|_|"素材直连地址无效")?;
    if url.scheme()!="https" || url.host_str()!=Some("media.harmonywatch.cn") || url.port()!=Some(8443) ||
        !url.username().is_empty() || url.password().is_some() || url.path()!="/" || url.query().is_some() || url.fragment().is_some() {
        return Err("素材直连地址不受信任".into());
    }
    Ok(url)
}
fn marketing_status(value:&Value,size:u64) -> Result<(u64,bool),String> {
    let offset=value["offset"].as_u64().ok_or("素材上传状态无效")?;
    if value["size"].as_u64()!=Some(size) || offset>size {return Err("素材上传大小或偏移无效".into());}
    match value["state"].as_str() {
        Some("uploading") => Ok((offset,false)),
        Some("index_pending") if offset==size => Ok((offset,false)),
        Some("complete") if offset==size && value["indexed"]==true => Ok((offset,true)),
        _ => Err("素材上传状态无效".into()),
    }
}
fn valid_marketing_id(id:&str) -> bool {id.len()==32 && id.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))}
fn marketing_request_id(id:&str) -> Result<String,String> {
    let uuid=uuid::Uuid::parse_str(id).map_err(|_|"上传任务标识无效")?;
    if uuid.to_string()!=id {return Err("上传任务标识无效".into());}
    Ok(uuid.simple().to_string())
}
fn marketing_creation_recoverable(entry:&Entry) -> Result<(),String> {
    if entry.remote.as_deref()==Some("creating") && !entry.idempotent_create {
        return Err("旧版上传创建结果未知，请联系管理员核查".into());
    }
    Ok(())
}
fn marketing_create_body(entry:&Entry, fingerprint:&str) -> Result<Value,String> {
    if fingerprint.len()!=64 || !fingerprint.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {return Err("上传文件校验值无效".into());}
    Ok(json!({"filename":entry.task.file_name,"folder":entry.task.folder,"size":entry.task.size,"sha256":fingerprint,"requestId":marketing_request_id(&entry.task.id)?}))
}
fn watch_prepare_create(entry:&mut Entry, fingerprint:&str) -> Result<Value,String> {
    let request_id=marketing_request_id(&entry.task.id)?;
    let body=json!({"filename":entry.task.file_name,"folder":entry.task.folder,"size":entry.task.size,"fingerprint":fingerprint,"content_sha256":fingerprint,"client_request_id":request_id});
    entry.fingerprint=Some(fingerprint.to_owned());
    Ok(body)
}
fn watch_creation_needs_recovery(entry:&Entry) -> bool {
    entry.task.source=="watch" && entry.remote.is_none() && entry.fingerprint.is_some()
}
fn watch_cancelled_transition(entry:&mut Entry, remote:&Value) -> bool {
    if remote["status"]!="cancelled" {return false;}
    entry.task.status="cancelled".into();
    entry.task.error=None;
    entry.task.bytes_per_second=None;
    true
}
fn marketing_file_allowed(config:&Value,ext:&str,size:u64) -> bool {
    size > 0 && config["max_file_bytes"].as_u64().is_some_and(|max| max > 0 && max <= 8_589_934_592 && size <= max) &&
        config["extensions"].as_array().is_some_and(|items|items.iter().any(|v|v.as_str()==Some(&format!(".{ext}"))))
}
fn marketing_upload_request(http:&reqwest::Client,mut url:reqwest::Url,path:&str,ticket:&str) -> reqwest::RequestBuilder {
    url.set_path(path);
    http.post(url).bearer_auth(ticket)
}
async fn marketing_request(path:&str, body:Option<&Value>, chunk:Option<(u64,Vec<u8>)>, generation:u64, cookie:&str) -> Result<Value,String> {
    super::super::marketing::ensure_current_session(generation)?;
    let http=client()?;
    let ticket=http.get(format!("{}/api/assets/transfer",super::super::marketing::marketing_base_url()))
        .header(COOKIE,cookie).send().await.map_err(|_|"无法获取素材上传凭证".to_string())?;
    super::super::marketing::ensure_current_session(generation)?;
    if !ticket.status().is_success(){return Err("无法获取素材上传凭证".into());}
    let ticket:Value=ticket.json().await.map_err(|_|"素材上传凭证无效".to_string())?;
    let url=marketing_origin(ticket["origin"].as_str().ok_or("素材直连地址缺失")?)?;
    let credential=ticket["uploadTicket"].as_str().filter(|v|!v.is_empty()).ok_or("素材上传凭证缺失")?;
    super::super::marketing::ensure_current_session(generation)?;
    let mut request=marketing_upload_request(&http,url,path,credential);
    if let Some(value)=body {request=request.json(value);}
    if let Some((offset,data))=chunk {request=request.header("content-type","application/octet-stream").header("upload-offset",offset).header("upload-sha256",format!("{:x}",Sha256::digest(&data))).body(data);}
    let response=request.send().await.map_err(|_|"素材上传网络中断，请重试".to_string())?;
    super::super::marketing::ensure_current_session(generation)?;
    if !response.status().is_success(){return Err(format!("素材上传请求失败（{}）",response.status()));}
    let value=response.json().await.map_err(|_|"素材上传响应无效".to_string())?;
    super::super::marketing::ensure_current_session(generation)?;
    Ok(value)
}
static QUEUE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
static LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();
static WORKERS: OnceLock<Mutex<BTreeMap<String,Arc<Mutex<()>>>>> = OnceLock::new();
fn queue_path() -> PathBuf { crate::app::logging::default_app_data_dir().join("watch-uploads.json") }
async fn queue() -> &'static Mutex<Vec<Entry>> {
    if let Some(value) = QUEUE.get() { return value; }
    let path=queue_path();
    let primary=tokio::fs::read(&path).await.ok().and_then(|x|serde_json::from_slice(&x).ok());
    let mut rows: Vec<Entry> = primary.or_else(||std::fs::read(path.with_extension("bak")).ok().and_then(|x|serde_json::from_slice(&x).ok())).unwrap_or_default();
    for row in &mut rows { if ["uploading","queued","verifying","saving","cancelling"].contains(&row.task.status.as_str()) {row.task.status="paused".into();} }
    QUEUE.get_or_init(||Mutex::new(rows))
}
async fn persist(rows: &[Entry]) -> Result<(),String> {
    let path=queue_path();
    tokio::fs::create_dir_all(path.parent().unwrap()).await.map_err(|_|"无法保存上传任务".to_string())?;
    let bytes=serde_json::to_vec(rows).map_err(|e|e.to_string())?;
    let temp=path.with_extension("tmp");
    tokio::fs::write(&temp,bytes).await.map_err(|_|"无法保存上传任务".to_string())?;
    // Windows rename cannot replace an existing destination; keep a backup until replacement succeeds.
    let backup=path.with_extension("bak");
    if path.exists() {tokio::fs::copy(&path,&backup).await.map_err(|_|"无法备份上传任务".to_string())?;tokio::fs::remove_file(&path).await.map_err(|_|"无法更新上传任务".to_string())?;}
    tokio::fs::rename(temp,path).await.map_err(|_|"无法保存上传任务".to_string())
}
async fn owner_snapshot() -> Result<(String,u64,String),String> {
    let (generation,cookie)=session().await?;
    let value=send_snapshot(Method::GET,"/api/auth/me",&BTreeMap::new(),None,generation,&cookie).await?;
    let owner=value["username"].as_str().map(str::to_owned).ok_or("请重新登录资料库")?;
    Ok((owner,generation,cookie))
}
fn modified(meta: &std::fs::Metadata) -> u64 {meta.modified().ok().and_then(|v|v.duration_since(std::time::UNIX_EPOCH).ok()).map(|v|v.as_millis() as u64).unwrap_or(0)}
fn action_state(status:&str,action:&str)->Result<&'static str,String>{
    match (status,action) {
        ("queued"|"uploading"|"verifying","pause")=>Ok("paused"),
        ("queued"|"uploading"|"verifying"|"paused"|"failed","cancel")=>Ok("cancelling"),
        ("paused"|"failed","resume"|"retry")=>Ok("queued"),
        _=>Err("当前任务无法执行此操作".into()),
    }
}
fn completion_state(value:&Value, fingerprint:&str)->Result<bool,String>{
    if value["index_task"]["status"]=="failed" {return Err("文件已传输，但 NAS 保存或入库失败，请重试".into());}
    if value["status"]=="saved" {
        if value["sha256"].as_str()!=Some(fingerprint) {return Err("NAS 文件完整校验失败，请联系管理员处理".into());}
        return Ok(!value["asset_id"].is_null() && value["index_task"]["status"]=="succeeded");
    }
    Ok(false)
}
fn allow_index_wait(attempt:usize)->Result<(),String>{
    if attempt >= 30 {Err("文件已上传，但入库暂未完成；请稍后重试，无需重新选择文件".into())} else {Ok(())}
}
async fn update(id:&str, f:impl FnOnce(&mut Entry)) -> Result<(),String> {
    let mut rows=queue().await.lock().await;
    let row=rows.iter_mut().find(|r|r.task.id==id).ok_or("上传任务不存在")?; f(row);persist(&rows).await
}
async fn update_active(id:&str, revision:u64, f:impl FnOnce(&mut Entry)) -> Result<(),String> {
    let mut rows=queue().await.lock().await;
    let row=rows.iter_mut().find(|r|r.task.id==id).ok_or("上传任务不存在")?;
    if row.revision!=revision || ["paused","cancelled"].contains(&row.task.status.as_str()) {return Err("上传已暂停或取消".into());}
    f(row);persist(&rows).await
}
async fn running(id:&str, generation:u64, revision:u64) -> Result<Entry,String> {
    super::super::marketing::ensure_current_session(generation)?;
    let rows=queue().await.lock().await;
    let row=rows.iter().find(|r|r.task.id==id).ok_or("上传任务不存在")?;
    if row.revision!=revision || ["paused","cancelled"].contains(&row.task.status.as_str()) {return Err("上传已暂停或取消".into());}
    Ok(row.clone())
}

#[tauri::command]
pub async fn watch_library_uploads() -> Result<Vec<UploadTask>,String> {
    let (account,generation,_)=portal_snapshot().await?;
    let rows=queue().await.lock().await;
    super::super::marketing::ensure_current_session(generation)?;
    Ok(rows.iter().filter(|r|account.owns_entry(r)).map(|r|r.task.clone()).collect())
}

#[tauri::command]
pub async fn marketing_resumable_upload_start(file_paths:Vec<String>,folder:String) -> Result<Vec<UploadTask>,String> {
    if file_paths.is_empty() || file_paths.len()>100 || folder.len()>200 {return Err("请选择 1 至 100 个文件和有效目录".into());}
    let (account,generation,cookie)=portal_snapshot().await?;
    let config=marketing_request("/api/assets/direct/uploads/config",None,None,generation,&cookie).await?;
    let mut created=Vec::new();
    for path in file_paths {
        let path=tokio::fs::canonicalize(path).await.map_err(|_|"无法读取所选文件")?;
        let meta=tokio::fs::metadata(&path).await.map_err(|_|"无法读取文件信息")?;
        let name=path.file_name().and_then(|s|s.to_str()).ok_or("文件名无效")?;
        let ext=path.extension().and_then(|s|s.to_str()).unwrap_or("").to_ascii_lowercase();
        if !meta.is_file() || name.len()>200 || name.chars().any(|c|c.is_control() || c=='/' || c=='\\') ||
            !marketing_file_allowed(&config,&ext,meta.len()) {return Err("文件格式、名称或大小不符合要求".into());}
        let task=UploadTask{id:uuid::Uuid::new_v4().to_string(),file_name:name.to_owned(),source:"marketing".into(),folder:folder.clone(),size:meta.len(),uploaded:0,status:"queued".into(),error:None,bytes_per_second:None};
        created.push(Entry{task,owner:account.id.clone(),portal_owner:Some(account.id.clone()),path,modified:modified(&meta),fingerprint:None,remote:None,idempotent_create:false,revision:0});
    }
    super::super::marketing::ensure_current_session(generation)?;
    let result=created.iter().map(|e|e.task.clone()).collect();
    {let mut rows=queue().await.lock().await;super::super::marketing::ensure_current_session(generation)?;rows.extend(created.clone());persist(&rows).await?;}
    if let Err(error)=super::super::marketing::ensure_current_session(generation) {
        let ids:Vec<_>=created.iter().map(|entry|entry.task.id.clone()).collect();
        let mut rows=queue().await.lock().await;
        for row in rows.iter_mut().filter(|row|ids.contains(&row.task.id)) {row.task.status="paused".into();}
        persist(&rows).await?;
        return Err(error);
    }
    for entry in created {spawn(entry.task.id,generation,entry.revision,cookie.clone());}
    Ok(result)
}
#[tauri::command]
pub async fn watch_library_upload_start(file_paths:Vec<String>,folder:String) -> Result<Vec<UploadTask>,String> {
    if file_paths.is_empty() || file_paths.len()>100 {return Err("请选择 1 至 100 个文件".into());}
    let (account,generation,cookie)=owner_snapshot().await?;
    let (portal,portal_generation,_)=portal_snapshot().await?;
    if portal_generation!=generation || !portal.email.eq_ignore_ascii_case(&account) {return Err("账号身份不一致，请重新登录".into());}
    let config=send_snapshot(Method::GET,"/api/uploads/config",&BTreeMap::new(),None,generation,&cookie).await?;
    if config["enabled"]!=true || !config["folders"].as_array().is_some_and(|a|a.iter().any(|x|x==&json!(folder))) {return Err("上传目录不可用".into());}
    let mut created=vec![];
    for path in file_paths {
        let path=tokio::fs::canonicalize(path).await.map_err(|_|"无法读取所选文件".to_string())?;
        let meta=tokio::fs::metadata(&path).await.map_err(|_|"无法读取文件信息".to_string())?;
        let extension=path.extension().and_then(|v|v.to_str()).unwrap_or("").to_lowercase();
        if !meta.is_file() || meta.len()==0 || meta.len()>config["max_file_bytes"].as_u64().unwrap_or(0) ||
            !config["extensions"].as_array().is_some_and(|a|a.iter().any(|v|v.as_str()==Some(&format!(".{extension}")))) {return Err("文件格式或大小不符合资料库要求".into());}
        let task=UploadTask{id:uuid::Uuid::new_v4().to_string(),file_name:path.file_name().unwrap().to_string_lossy().to_string(),source:"watch".into(),folder:folder.clone(),size:meta.len(),uploaded:0,status:"queued".into(),error:None,bytes_per_second:None};
        created.push(Entry{task,owner:account.clone(),portal_owner:Some(portal.id.clone()),path,modified:modified(&meta),fingerprint:None,remote:None,idempotent_create:false,revision:0});
    }
    super::super::marketing::ensure_current_session(generation)?;
    let result=created.iter().map(|e|e.task.clone()).collect();
    {let mut rows=queue().await.lock().await;rows.extend(created.clone());persist(&rows).await?;}
    for entry in created {spawn(entry.task.id,generation,entry.revision,cookie.clone());}
    Ok(result)
}
fn spawn(id:String,generation:u64,revision:u64,cookie:String){tokio::spawn(async move {
    let lock={let mut locks=WORKERS.get_or_init(||Mutex::new(BTreeMap::new())).lock().await;locks.entry(id.clone()).or_insert_with(||Arc::new(Mutex::new(()))).clone()};
    let _job=lock.lock().await;
    let semaphore=LIMIT.get_or_init(||Arc::new(Semaphore::new(2))).clone();
    let _permit=semaphore.acquire().await;
    let source={let rows=queue().await.lock().await;rows.iter().find(|r|r.task.id==id).map(|r|r.task.source.clone())};
    let outcome=if source.as_deref()==Some("marketing") {marketing_transfer(&id,generation,revision,&cookie).await} else {transfer(&id,generation,revision,&cookie).await};
    if let Err(error)=outcome {
        let _=update(&id,|r|{if r.revision==revision&&! ["paused","cancelled"].contains(&r.task.status.as_str()){r.task.status="failed".into();r.task.error=Some(error);}}).await;
    }
});}
async fn transfer(id:&str,generation:u64,revision:u64,cookie:&str) -> Result<(),String> {
    let mut entry=running(id,generation,revision).await?;
    let mut file=tokio::fs::File::open(&entry.path).await.map_err(|_|"源文件已移动或无法读取".to_string())?;
    let meta=file.metadata().await.map_err(|e|e.to_string())?;
    if meta.len()!=entry.task.size || modified(&meta)!=entry.modified {return Err("源文件已经变化，请重新选择文件".into());}
    update_active(id,revision,|r|r.task.status="verifying".into()).await?;
    let mut hasher=Sha256::new();let mut buffer=vec![0;1024*1024];
    loop {running(id,generation,revision).await?;let n=file.read(&mut buffer).await.map_err(|e|e.to_string())?;if n==0{break;}hasher.update(&buffer[..n]);}
    let fingerprint=format!("{:x}",hasher.finalize());
    if entry.fingerprint.as_ref().is_some_and(|v|v!=&fingerprint){return Err("源文件内容已经变化，请重新创建上传".into());}
    let body=watch_prepare_create(&mut entry,&fingerprint)?;
    // A lost create response must still leave enough durable metadata to cancel remotely.
    update_active(id,revision,|r|r.fingerprint=entry.fingerprint.clone()).await?;
    running(id,generation,revision).await?;
    let remote=send_snapshot(Method::POST,"/api/uploads",&BTreeMap::new(),Some(&body),generation,cookie).await?;
    let remote_id=remote["id"].as_str().filter(|s|s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')).ok_or("上传记录无效")?.to_string();
    update(id,|r|r.remote=Some(remote_id.clone())).await?;
    if remote["status"]=="cancelled" {
        return update_active(id,revision,|r|{watch_cancelled_transition(r,&remote);}).await;
    }
    let mut offset=remote["offset"].as_u64().ok_or("上传偏移无效")?;
    if offset>entry.task.size{return Err("服务器上传偏移超过文件大小".into());}
    running(id,generation,revision).await?;
    update_active(id,revision,|r|{r.task.uploaded=offset;r.task.status="uploading".into();}).await?;
    let mut chunk=vec![0;8*1024*1024];let start=std::time::Instant::now();let initial=offset;
    while offset<entry.task.size {
        running(id,generation,revision).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e|e.to_string())?;
        let wanted=((entry.task.size-offset) as usize).min(chunk.len());
        file.read_exact(&mut chunk[..wanted]).await.map_err(|_|"源文件读取失败".to_string())?;
        super::super::marketing::ensure_current_session(generation)?;
        let response=client()?.patch(format!("{BASE}/api/uploads/{remote_id}")).header(COOKIE,cookie).header("Upload-Offset",offset)
            .header("X-Chunk-SHA256",format!("{:x}",Sha256::digest(&chunk[..wanted]))).body(chunk[..wanted].to_vec()).send().await.map_err(|_|"网络中断，可点击重试续传".to_string())?;
        running(id,generation,revision).await?;
        if !response.status().is_success(){return Err(format!("分片上传失败（{}），可重试续传",response.status()));}
        let value:Value=response.json().await.map_err(|_|"上传返回格式无效".to_string())?;
        let next=value["offset"].as_u64().ok_or("服务器上传偏移无效")?;
        if next!=offset+wanted as u64{return Err("上传偏移不一致，请重试同步进度".into());}offset=next;
        update_active(id,revision,|r|{r.task.uploaded=offset;r.task.bytes_per_second=Some(((offset-initial) as f64/start.elapsed().as_secs_f64().max(0.01)) as u64);}).await?;
    }
    running(id,generation,revision).await?;
    let final_meta=file.metadata().await.map_err(|e|e.to_string())?;
    if final_meta.len()!=entry.task.size || modified(&final_meta)!=entry.modified{return Err("源文件已变化，请重新选择后上传".into());}
    update_active(id,revision,|r|r.task.status="saving".into()).await?;
    let mut complete=send_snapshot(Method::POST,&format!("/api/uploads/{remote_id}/complete"),&BTreeMap::new(),None,generation,cookie).await?;
    let mut index_attempt=0;
    while !completion_state(&complete,&fingerprint)? {
        allow_index_wait(index_attempt)?;
        index_attempt+=1;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        running(id,generation,revision).await?;
        complete=send_snapshot(Method::GET,&format!("/api/uploads/{remote_id}"),&BTreeMap::new(),None,generation,cookie).await?;
    }
    running(id,generation,revision).await?;
    update_active(id,revision,|r|{r.task.status="completed".into();r.task.error=None;}).await
}

async fn marketing_transfer(id:&str,generation:u64,revision:u64,cookie:&str) -> Result<(),String> {
    let entry=running(id,generation,revision).await?;
    marketing_creation_recoverable(&entry)?;
    let mut file=tokio::fs::File::open(&entry.path).await.map_err(|_|"源文件已移动或无法读取")?;
    let meta=file.metadata().await.map_err(|_|"无法读取源文件")?;
    if meta.len()!=entry.task.size || modified(&meta)!=entry.modified {return Err("源文件已经变化，请重新选择文件".into());}
    update_active(id,revision,|r|r.task.status="verifying".into()).await?;
    let mut hasher=Sha256::new();let mut buffer=vec![0;1024*1024];
    loop {running(id,generation,revision).await?;let n=file.read(&mut buffer).await.map_err(|_|"源文件读取失败")?;if n==0{break;}hasher.update(&buffer[..n]);}
    let fingerprint=format!("{:x}",hasher.finalize());
    if entry.fingerprint.as_ref().is_some_and(|v|v!=&fingerprint) {return Err("源文件内容已经变化，请重新创建上传".into());}
    let remote_id=if let Some(remote)=entry.remote.as_ref().filter(|v|v.as_str()!="creating") {
        if !valid_marketing_id(remote) {return Err("上传记录无效".into());} remote.clone()
    } else {
        let body=marketing_create_body(&entry,&fingerprint)?;
        update_active(id,revision,|r|{r.remote=Some("creating".into());r.fingerprint=Some(fingerprint.clone());r.idempotent_create=true;}).await?;
        running(id,generation,revision).await?;
        let response=marketing_request("/api/assets/direct/uploads/create",Some(&body),None,generation,cookie).await?;
        let remote=response["id"].as_str().filter(|s|valid_marketing_id(s)).ok_or("上传记录无效")?.to_owned();
        // Record the server id even if pause or cancel raced the create response.
        update(id,|r|{r.remote=Some(remote.clone());}).await?;
        remote
    };
    let status_path=format!("/api/assets/direct/uploads/{remote_id}/status");
    let mut index_attempt=0;
    loop {
        running(id,generation,revision).await?;
        let status=marketing_request(&status_path,None,None,generation,cookie).await?;
        let (mut offset,complete)=marketing_status(&status,entry.task.size)?;
        if complete {return update_active(id,revision,|r|{r.task.uploaded=entry.task.size;r.task.status="completed".into();r.task.error=None;r.task.bytes_per_second=None;}).await;}
        if status["state"]=="index_pending" {
            allow_index_wait(index_attempt)?;
            index_attempt+=1;
            update_active(id,revision,|r|{r.task.uploaded=offset;r.task.status="saving".into();}).await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }
        update_active(id,revision,|r|{r.task.uploaded=offset;r.task.status="uploading".into();}).await?;
        let mut chunk=vec![0;8*1024*1024];let start=std::time::Instant::now();let initial=offset;
        while offset<entry.task.size {
            running(id,generation,revision).await?;
            file.seek(std::io::SeekFrom::Start(offset)).await.map_err(|_|"源文件定位失败")?;
            let wanted=((entry.task.size-offset) as usize).min(chunk.len());
            file.read_exact(&mut chunk[..wanted]).await.map_err(|_|"源文件读取失败")?;
            let response=marketing_request(&format!("/api/assets/direct/uploads/{remote_id}/chunk"),None,Some((offset,chunk[..wanted].to_vec())),generation,cookie).await?;
            running(id,generation,revision).await?;
            let next=response["offset"].as_u64().ok_or("素材上传偏移无效")?;
            if next!=offset+wanted as u64{return Err("素材上传偏移不一致，请重试同步".into());}
            offset=next;
            update_active(id,revision,|r|{r.task.uploaded=offset;r.task.bytes_per_second=Some(((offset-initial) as f64/start.elapsed().as_secs_f64().max(0.01)) as u64);}).await?;
        }
        let meta=file.metadata().await.map_err(|_|"无法读取源文件")?;
        if meta.len()!=entry.task.size || modified(&meta)!=entry.modified {return Err("源文件已变化，请重新选择后上传".into());}
        running(id,generation,revision).await?;
        update_active(id,revision,|r|r.task.status="saving".into()).await?;
        marketing_request(&format!("/api/assets/direct/uploads/{remote_id}/complete"),None,None,generation,cookie).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn watch_entry() -> Entry {
        serde_json::from_value(json!({"task":{"id":"01234567-89ab-cdef-0123-456789abcdef","fileName":"a.mp4","source":"watch","folder":"x","size":10,"uploaded":3,"status":"paused","error":"network lost","bytesPerSecond":42},"owner":"a@example.com","portal_owner":"id-a","path":"/tmp/a","modified":1,"fingerprint":null,"remote":null})).unwrap()
    }
    #[test]
    fn watch_create_metadata_survives_lost_response() {
        let mut entry=watch_entry();
        let hash="a".repeat(64);
        let body=watch_prepare_create(&mut entry,&hash).unwrap();
        assert_eq!(body,json!({"filename":"a.mp4","folder":"x","size":10,"fingerprint":hash,"content_sha256":hash,"client_request_id":"0123456789abcdef0123456789abcdef"}));
        let mut restored:Entry=serde_json::from_slice(&serde_json::to_vec(&entry).unwrap()).unwrap();
        assert_eq!(restored.fingerprint.as_deref(),Some(hash.as_str()));
        assert!(restored.remote.is_none());
        assert!(watch_creation_needs_recovery(&restored));
        let fingerprint=restored.fingerprint.clone().unwrap();
        assert_eq!(watch_prepare_create(&mut restored,&fingerprint).unwrap(),body);
    }
    #[test]
    fn watch_cancel_only_recovers_a_started_create() {
        let mut entry=watch_entry();
        assert!(!watch_creation_needs_recovery(&entry));
        watch_prepare_create(&mut entry,&"a".repeat(64)).unwrap();
        assert!(watch_creation_needs_recovery(&entry));
        entry.remote=Some("remote-id".into());
        assert!(!watch_creation_needs_recovery(&entry));
        entry.remote=None;
        entry.task.source="marketing".into();
        assert!(!watch_creation_needs_recovery(&entry));
    }
    #[test]
    fn watch_remote_cancelled_is_terminal_on_resume() {
        let mut entry=watch_entry();
        entry.task.status="verifying".into();
        assert!(!watch_cancelled_transition(&mut entry,&json!({"status":"uploading"})));
        assert_eq!(entry.task.status,"verifying");
        assert!(watch_cancelled_transition(&mut entry,&json!({"status":"cancelled"})));
        assert_eq!(entry.task.status,"cancelled");
        assert!(entry.task.error.is_none());
        assert!(entry.task.bytes_per_second.is_none());
        assert_eq!(entry.task.uploaded,3);
        assert!(action_state(&entry.task.status,"resume").is_err());
        assert!(action_state(&entry.task.status,"retry").is_err());
    }
    #[tokio::test]
    #[ignore = "writes a named integration PNG using the signed-in account to the live company NAS"]
    async fn live_marketing_upload_and_index() {
        use base64::Engine;
        let path=std::env::temp_dir().join(format!("harmony114-integration-{}.png",uuid::Uuid::new_v4()));
        let data=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aKXcAAAAASUVORK5CYII=").unwrap();
        tokio::fs::write(&path,data).await.unwrap();
        let tasks=marketing_resumable_upload_start(vec![path.to_string_lossy().into_owned()],String::new()).await.expect("live upload creation");
        let id=tasks[0].id.clone();
        let outcome=tokio::time::timeout(std::time::Duration::from_secs(150),async {
            loop {
                let rows=watch_library_uploads().await.expect("live upload status");
                let task=rows.iter().find(|row|row.id==id).expect("created task");
                if task.status=="failed" {panic!("live upload failed: {:?}",task.error);}
                if task.status=="completed" {return;}
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }).await;
        let _=tokio::fs::remove_file(&path).await;
        outcome.expect("live upload/index timeout");
    }
    #[test]
    fn index_failure_eventually_releases_the_transfer_slot() {
        assert!(allow_index_wait(0).is_ok());
        assert!(allow_index_wait(29).is_ok());
        assert!(allow_index_wait(30).is_err());
        assert!(allow_index_wait(300).is_err());
    }
    #[test]
    fn marketing_ticket_is_a_header_not_a_url_parameter() {
        let url=marketing_origin("https://media.harmonywatch.cn:8443").unwrap();
        let request=marketing_upload_request(&client().unwrap(),url,"/api/assets/direct/uploads/create","secret-ticket").build().unwrap();
        assert_eq!(request.headers().get(reqwest::header::AUTHORIZATION).unwrap(),"Bearer secret-ticket");
        assert!(request.url().query().is_none());
        assert!(!request.url().as_str().contains("secret-ticket"));
    }
    #[test]
    fn marketing_extensions_match_target_policy() {
        let extensions=["jpg","jpeg","png","webp","pdf","mp4","mov","m4a","mp3","wav","docx","xlsx","pptx","zip"];
        let config=json!({"extensions":extensions.map(|ext|format!(".{ext}")),"max_file_bytes":8_589_934_592u64});
        for ext in extensions {assert!(marketing_file_allowed(&config,ext,8_589_934_592),"{ext}");}
        for ext in ["exe",""] {assert!(!marketing_file_allowed(&config,ext,1));}
        for size in [0,8_589_934_593,9_007_199_254_740_991] {assert!(!marketing_file_allowed(&config,"zip",size));}
        assert!(!marketing_file_allowed(&json!({}),"zip",1));
        assert!(!marketing_file_allowed(&json!({"extensions":[".zip"],"max_file_bytes":9_007_199_254_740_991u64}),"zip",1));
        assert!(!marketing_file_allowed(&json!({"extensions":[".zip"],"max_file_bytes":10}),"zip",11));
        assert!(marketing_file_allowed(&json!({"extensions":[".zip"],"max_file_bytes":10}),"zip",10));
    }
    #[test]
    fn marketing_origin_is_strict() {
        assert!(marketing_origin("https://media.harmonywatch.cn:8443").is_ok());
        for value in ["http://media.harmonywatch.cn:8443", "https://media.harmonywatch.cn.evil.test:8443", "https://media.harmonywatch.cn:8443/path", "https://user@media.harmonywatch.cn:8443", "https://media.harmonywatch.cn:8443/?ticket=x"] {
            assert!(marketing_origin(value).is_err(), "{value}");
        }
    }
    #[test]
    fn marketing_resume_validates_status() {
        assert_eq!(marketing_status(&json!({"state":"uploading","offset":3,"size":10}), 10).unwrap(), (3, false));
        assert_eq!(marketing_status(&json!({"state":"complete","offset":10,"size":10,"indexed":true}), 10).unwrap(), (10, true));
        assert!(marketing_status(&json!({"state":"uploading","offset":11,"size":10}), 10).is_err());
        assert!(marketing_status(&json!({"state":"complete","offset":10,"size":10,"indexed":false}), 10).is_err());
        assert_eq!(marketing_status(&json!({"state":"index_pending","offset":10,"size":10,"indexed":false}), 10).unwrap(), (10, false));
        assert!(marketing_status(&json!({"state":"uploading","offset":3,"size":9}), 10).is_err());
    }
    #[test]
    fn marketing_remote_ids_are_not_paths() {
        assert!(valid_marketing_id("0123456789abcdef0123456789abcdef"));
        for value in ["creating", "../../status", "0123456789ABCDEF0123456789abcdef", "0123456789abcdef0123456789abcde/"] {
            assert!(!valid_marketing_id(value));
        }
    }
    #[test]
    fn creation_metadata_is_stable_and_validated() {
        let entry:Entry=serde_json::from_value(json!({"task":{"id":"01234567-89ab-cdef-0123-456789abcdef","fileName":"a.mp4","source":"marketing","folder":"x","size":1,"uploaded":0,"status":"paused","error":null,"bytesPerSecond":null},"owner":"id-a","portal_owner":"id-a","path":"/tmp/a","modified":1,"fingerprint":"abc","remote":"creating","idempotent_create":true})).unwrap();
        let hash="a".repeat(64);
        let body=marketing_create_body(&entry,&hash).unwrap();
        assert_eq!(body["requestId"],"0123456789abcdef0123456789abcdef");
        assert_eq!(body["filename"],"a.mp4");
        assert_eq!(body["folder"],"x");
        assert_eq!(body["sha256"],hash);
        assert_eq!(body,marketing_create_body(&entry,&hash).unwrap());
        assert!(marketing_request_id("../../invalid").is_err());
        assert!(marketing_creation_recoverable(&entry).is_ok());
        let restored:Entry=serde_json::from_slice(&serde_json::to_vec(&entry).unwrap()).unwrap();
        assert!(restored.idempotent_create);
        assert_eq!(marketing_create_body(&restored,&hash).unwrap(),body);
        assert_eq!(action_state("verifying","pause").unwrap(),"paused");
        assert_eq!(action_state("paused","retry").unwrap(),"queued");
        assert_eq!(action_state("paused","cancel").unwrap(),"cancelling");
        let mut old=entry.clone(); old.idempotent_create=false;
        assert!(marketing_creation_recoverable(&old).is_err());
        assert!(marketing_create_body(&entry,"abc").is_err());
    }
    #[test]
    fn portal_account_filters_each_source() {
        let account = PortalAccount { id: "id-a".into(), email: "a@example.com".into() };
        assert!(account.owns("marketing", "id-a"));
        assert!(!account.owns("marketing", "id-b"));
        assert!(account.owns("watch", "A@EXAMPLE.COM"));
        assert!(!account.owns("watch", "b@example.com"));
        let mut entry:Entry=serde_json::from_value(json!({"task":{"id":"1","fileName":"a.mp4","source":"watch","folder":"x","size":1,"uploaded":0,"status":"paused","error":null,"bytesPerSecond":null},"owner":"a@example.com","portal_owner":"id-b","path":"/tmp/a","modified":1,"fingerprint":null,"remote":null})).unwrap();
        assert!(!account.owns_entry(&entry));
        entry.portal_owner=Some("id-a".into());
        assert!(account.owns_entry(&entry));
        entry.portal_owner=None;
        assert!(!account.owns_entry(&entry));
    }
    #[test]
    fn saved_needs_matching_hash_and_successful_index() {
        assert!(!completion_state(&json!({"status":"saved","sha256":"abc","asset_id":null,"index_task":{"status":"running"}}),"abc").unwrap());
        assert!(completion_state(&json!({"status":"saved","sha256":"bad","asset_id":1,"index_task":{"status":"succeeded"}}),"abc").is_err());
        assert!(completion_state(&json!({"status":"saved","sha256":"abc","asset_id":1,"index_task":{"status":"succeeded"}}),"abc").unwrap());
        assert!(completion_state(&json!({"status":"saved","sha256":"abc","asset_id":null,"index_task":{"status":"failed"}}),"abc").is_err());
    }
    #[test]
    fn upload_actions_reject_saving_pause_and_resume_during_cancel() {
        assert!(action_state("saving", "pause").is_err());
        assert!(action_state("cancelling", "resume").is_err());
        assert_eq!(action_state("uploading", "cancel").unwrap(), "cancelling");
        assert_eq!(action_state("paused", "resume").unwrap(), "queued");
    }
}
#[tauri::command]
pub async fn watch_library_upload_action(task_id:String,action:String) -> Result<(),String> {
    let (account,generation,portal_cookie)=portal_snapshot().await?;
    let source={let rows=queue().await.lock().await;rows.iter().find(|e|e.task.id==task_id&&account.owns_entry(e)).ok_or("上传任务不存在")?.task.source.clone()};
    let cookie=if source=="watch" && action!="pause" {let (watch_generation,cookie)=session().await?;if generation!=watch_generation {return Err("登录状态已改变".into())};cookie} else {portal_cookie};
    let mut rows=queue().await.lock().await;
    super::super::marketing::ensure_current_session(generation)?;
    let row=rows.iter_mut().find(|e|e.task.id==task_id&&account.owns_entry(e)).ok_or("上传任务不存在")?;
    if source=="marketing" {marketing_creation_recoverable(row)?;}
    row.task.status=action_state(&row.task.status,&action)?.into();
    row.task.error=None;
    row.revision+=1;
    let revision=row.revision;
    let restart=row.task.status=="queued";persist(&rows).await?;drop(rows);
    if action=="cancel" {
        let worker={let mut locks=WORKERS.get_or_init(||Mutex::new(BTreeMap::new())).lock().await;locks.entry(task_id.clone()).or_insert_with(||Arc::new(Mutex::new(()))).clone()};
        let _guard=worker.lock().await;
        let mut entry={let rows=queue().await.lock().await;rows.iter().find(|r|r.task.id==task_id&&account.owns_entry(r)).cloned().ok_or("上传任务不存在")?};
        let remote=if source=="marketing" && entry.remote.as_deref()==Some("creating") {
            let recovered=async {
                marketing_creation_recoverable(&entry)?;
                let fingerprint=entry.fingerprint.as_deref().ok_or("上传文件校验值缺失")?;
                let body=marketing_create_body(&entry,fingerprint)?;
                let response=marketing_request("/api/assets/direct/uploads/create",Some(&body),None,generation,&cookie).await?;
                let remote=response["id"].as_str().filter(|s|valid_marketing_id(s)).ok_or("上传记录无效")?.to_owned();
                update(&task_id,|r|{if r.revision==revision {r.remote=Some(remote.clone());}}).await?;
                Ok::<_,String>(remote)
            }.await;
            match recovered {
                Ok(remote)=>Some(remote),
                Err(error)=>{
                    update(&task_id,|r|{if r.revision==revision {r.task.status="paused".into();r.task.error=Some(error.clone());}}).await?;
                    return Err(error);
                }
            }
        } else if watch_creation_needs_recovery(&entry) {
            let recovered=async {
                let fingerprint=entry.fingerprint.clone().ok_or("上传文件校验值缺失")?;
                let body=watch_prepare_create(&mut entry,&fingerprint)?;
                let response=send_snapshot(Method::POST,"/api/uploads",&BTreeMap::new(),Some(&body),generation,&cookie).await?;
                let remote=response["id"].as_str().filter(|s|!s.is_empty() && s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')).ok_or("上传记录无效")?.to_owned();
                update(&task_id,|r|{if r.revision==revision {r.remote=Some(remote.clone());}}).await?;
                Ok::<_,String>(remote)
            }.await;
            match recovered {
                Ok(remote)=>Some(remote),
                Err(error)=>{
                    update(&task_id,|r|{if r.revision==revision {r.task.status="paused".into();r.task.error=Some(error.clone());}}).await?;
                    return Err(error);
                }
            }
        } else {entry.remote};
        if let Some(remote)=remote {
            if source=="marketing" && !valid_marketing_id(&remote) {
                update(&task_id,|r|{if r.revision==revision {r.task.status="paused".into();r.task.error=Some("上传记录无效".into());}}).await?;
                return Err("上传记录无效".into());
            }
            let result=if source=="marketing" {marketing_request(&format!("/api/assets/direct/uploads/{remote}/cancel"),None,None,generation,&cookie).await} else {send_snapshot(Method::DELETE,&format!("/api/uploads/{remote}"),&BTreeMap::new(),None,generation,&cookie).await};
            if let Err(error)=result {
                update(&task_id,|r|{if r.revision==revision {r.task.status="paused".into();r.task.error=Some(error.clone());}}).await?;
                return Err(error);
            }
        }
        super::super::marketing::ensure_current_session(generation)?;
        update(&task_id,|r|{if r.revision==revision {r.task.status="cancelled".into();}}).await?;
    }
    if restart {spawn(task_id,generation,revision,cookie);}
    Ok(())
}
