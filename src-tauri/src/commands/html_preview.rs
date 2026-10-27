use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::http::{Request, Response};

const MAX_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;
const GRANT_TTL: Duration = Duration::from_secs(45 * 60);
const MAX_GRANTS: usize = 128;

struct Grant {
    root: PathBuf,
    file_only: Option<PathBuf>,
    network: bool,
    created: Instant,
}

fn grants() -> &'static Mutex<HashMap<String, Grant>> {
    static STORE: OnceLock<Mutex<HashMap<String, Grant>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn create(
    file: &Path,
    workspace: &str,
    network: bool,
    allow_directory: bool,
) -> Result<String, String> {
    let file = file.canonicalize().map_err(|_| "无法解析预览文件")?;
    let workspace = Path::new(workspace).canonicalize().ok();
    let in_workspace = workspace
        .as_ref()
        .is_some_and(|root| file.starts_with(root));
    let root = workspace
        .filter(|_| in_workspace)
        .or_else(|| file.parent().map(Path::to_path_buf))
        .ok_or("无法确定预览资源目录")?;
    let relative = file
        .strip_prefix(&root)
        .map_err(|_| "预览路径不在授权目录内")?;
    let path = relative
        .components()
        .map(|part| urlencoding::encode(&part.as_os_str().to_string_lossy()).into_owned())
        .collect::<Vec<_>>()
        .join("/");
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut store = grants().lock().map_err(|_| "预览授权不可用")?;
    store.retain(|_, grant| grant.created.elapsed() < GRANT_TTL);
    if store.len() >= MAX_GRANTS {
        if let Some(oldest) = store
            .iter()
            .min_by_key(|(_, grant)| grant.created)
            .map(|(key, _)| key.clone())
        {
            store.remove(&oldest);
        }
    }
    let file_only = if !in_workspace && !allow_directory {
        Some(file)
    } else {
        None
    };
    store.insert(
        token.clone(),
        Grant {
            root,
            file_only,
            network,
            created: Instant::now(),
        },
    );
    #[cfg(windows)]
    let origin = "http://harmony-preview.localhost";
    #[cfg(not(windows))]
    let origin = "harmony-preview://localhost";
    Ok(format!("{origin}/{token}/{path}"))
}

fn resolve(path: &str) -> Result<(PathBuf, bool), String> {
    let decoded = urlencoding::decode(path).map_err(|_| "Invalid preview encoding")?;
    let (token, relative) = decoded
        .trim_start_matches('/')
        .split_once('/')
        .ok_or("Missing preview grant")?;
    if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid preview grant".into());
    }
    if relative.is_empty()
        || relative.contains(['\\', ':', '\0'])
        || Path::new(relative)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Invalid preview resource path".into());
    }
    let store = grants().lock().map_err(|_| "Preview grants unavailable")?;
    let grant = store
        .get(token)
        .filter(|grant| grant.created.elapsed() < GRANT_TTL)
        .ok_or("Preview expired; reopen the artifact")?;
    let file = grant
        .root
        .join(relative)
        .canonicalize()
        .map_err(|_| "Preview resource not found")?;
    if !file.starts_with(&grant.root)
        || !file.is_file()
        || grant
            .file_only
            .as_ref()
            .is_some_and(|allowed| file != *allowed)
    {
        return Err("Preview resource is outside the authorized directory".into());
    }
    Ok((file, grant.network))
}

fn content_type(file: &Path) -> &'static str {
    match file
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn csp(network: bool) -> String {
    if network {
        // Interactive mode is explicitly trusted for scripts AND network/navigation.
        "default-src 'none'; script-src 'self' 'unsafe-inline' https: http:; style-src 'self' 'unsafe-inline' https: http:; img-src 'self' data: blob: https: http:; font-src 'self' data: https: http:; media-src 'self' data: blob: https: http:; connect-src 'self' https: http: wss: ws:; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'".into()
    } else {
        "default-src 'none'; script-src 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; sandbox".into()
    }
}

fn response(status: u16, body: Vec<u8>, mime: &str, network: bool) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Content-Security-Policy", csp(network))
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("Access-Control-Allow-Origin", "null")
        .header("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
        .header("Cache-Control", "no-store")
        .body(body)
        .expect("valid preview response")
}

pub(super) fn serve(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    if request.method() == "OPTIONS" {
        return response(204, Vec::new(), "text/plain", false);
    }
    if request.method() != "GET" && request.method() != "HEAD" {
        return response(405, b"Read-only preview".to_vec(), "text/plain", false);
    }
    let (path, network) = match resolve(request.uri().path()) {
        Ok(value) => value,
        Err(error) => return response(403, error.into_bytes(), "text/plain", false),
    };
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(_) => return response(404, b"Resource unavailable".to_vec(), "text/plain", false),
    };
    let length = match file.metadata() {
        Ok(meta) => meta.len(),
        Err(_) => return response(404, Vec::new(), "text/plain", false),
    };
    if length > MAX_RESOURCE_BYTES {
        return response(
            413,
            b"Preview resource exceeds 64 MB; open with the system application".to_vec(),
            "text/plain",
            false,
        );
    }
    let range = request
        .headers()
        .get("Range")
        .and_then(|value| value.to_str().ok());
    let (start, end, partial) = match range {
        None => (0, length.saturating_sub(1), false),
        Some(value) => match value
            .strip_prefix("bytes=")
            .and_then(|value| value.split_once('-'))
        {
            Some((start, end)) if !start.is_empty() => match (
                start.parse::<u64>(),
                if end.is_empty() {
                    Ok(length.saturating_sub(1))
                } else {
                    end.parse::<u64>()
                },
            ) {
                (Ok(start), Ok(end)) if start <= end && end < length => (start, end, true),
                _ => return response(416, Vec::new(), "text/plain", false),
            },
            _ => return response(416, Vec::new(), "text/plain", false),
        },
    };
    let mut bytes = Vec::new();
    if request.method() != "HEAD" && length > 0 {
        if file.seek(SeekFrom::Start(start)).is_err()
            || file.take(end - start + 1).read_to_end(&mut bytes).is_err()
        {
            return response(
                500,
                b"Cannot read preview resource".to_vec(),
                "text/plain",
                false,
            );
        }
    }
    let mut result = response(
        if partial { 206 } else { 200 },
        bytes,
        content_type(&path),
        network,
    );
    result
        .headers_mut()
        .insert("Accept-Ranges", "bytes".parse().unwrap());
    if partial {
        result.headers_mut().insert(
            "Content-Range",
            format!("bytes {start}-{end}/{length}").parse().unwrap(),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_grant_does_not_implicitly_authorize_siblings() {
        let temp = std::env::temp_dir().join(format!("external-grants-{}", uuid::Uuid::new_v4()));
        let workspace = temp.join("workspace");
        let outside = temp.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let file = outside.join("index.html");
        std::fs::write(&file, "<h1>Outside</h1>").unwrap();
        std::fs::write(outside.join("private.txt"), "synthetic sentinel").unwrap();
        let url = create(
            &file.canonicalize().unwrap(),
            workspace.to_str().unwrap(),
            false,
            false,
        )
        .unwrap();
        let uri: tauri::http::Uri = url.parse().unwrap();
        let base = uri.path().rsplit_once('/').unwrap().0;
        let sibling = serve(
            Request::builder()
                .uri(format!("{base}/private.txt"))
                .body(Vec::new())
                .unwrap(),
        );
        std::fs::remove_dir_all(&temp).unwrap();
        assert_eq!(sibling.status(), 403);
    }

    #[test]
    fn external_directory_grant_is_explicit_and_canonicalized() {
        let temp = std::env::temp_dir().join(format!("directory-grants-{}", uuid::Uuid::new_v4()));
        let workspace = temp.join("workspace");
        let outside = temp.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(outside.join("assets")).unwrap();
        let file = outside.join("index.html");
        std::fs::write(&file, "<script type=module src=assets/app.mjs></script>").unwrap();
        std::fs::write(outside.join("assets/app.mjs"), "window.ready=true").unwrap();
        std::fs::write(temp.join("denied.txt"), "synthetic outside sentinel").unwrap();
        for (network, directory, expected) in [
            (false, false, 403),
            (true, false, 403),
            (false, true, 200),
            (true, true, 200),
        ] {
            let url = create(&file, workspace.to_str().unwrap(), network, directory).unwrap();
            let uri: tauri::http::Uri = url.parse().unwrap();
            let base = uri.path().rsplit_once('/').unwrap().0;
            assert_eq!(
                serve(
                    Request::builder()
                        .uri(uri.clone())
                        .body(Vec::new())
                        .unwrap()
                )
                .status(),
                200
            );
            let resource = serve(
                Request::builder()
                    .uri(format!("{base}/assets/app.mjs"))
                    .body(Vec::new())
                    .unwrap(),
            );
            assert_eq!(resource.status(), expected);
            if expected == 200 {
                assert_eq!(resource.headers()["Access-Control-Allow-Origin"], "null");
                assert_eq!(
                    resource.headers()["Content-Type"],
                    "text/javascript; charset=utf-8"
                );
            }
            assert!(resolve(&format!("{base}/../denied.txt")).is_err());
            #[cfg(unix)]
            {
                let link = outside.join(format!("escape-{network}-{directory}.txt"));
                std::os::unix::fs::symlink(temp.join("denied.txt"), &link).unwrap();
                assert!(resolve(&format!(
                    "{base}/{}",
                    link.file_name().unwrap().to_str().unwrap()
                ))
                .is_err());
            }
        }
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn scoped_resources_work_but_traversal_and_symlink_escapes_do_not() {
        let temp = std::env::temp_dir().join(format!("html-grants-{}", uuid::Uuid::new_v4()));
        let workspace = temp.join("workspace");
        std::fs::create_dir_all(workspace.join("assets")).unwrap();
        std::fs::write(
            workspace.join("index.html"),
            "<script src='assets/app.js'></script>",
        )
        .unwrap();
        std::fs::write(workspace.join("assets/app.js"), "window.ready=true;").unwrap();
        std::fs::write(temp.join("outside.txt"), "outside sentinel").unwrap();
        let url = create(
            &workspace.join("index.html").canonicalize().unwrap(),
            workspace.to_str().unwrap(),
            true,
            false,
        )
        .unwrap();
        let uri: tauri::http::Uri = url.parse().unwrap();
        let base = uri.path().rsplit_once('/').unwrap().0;
        let resource = serve(
            Request::builder()
                .uri(format!("{base}/assets/app.js"))
                .body(Vec::new())
                .unwrap(),
        );
        assert_eq!(resource.status(), 200);
        assert_eq!(resource.headers()["Access-Control-Allow-Origin"], "null");
        assert!(resource.headers()["Content-Security-Policy"]
            .to_str()
            .unwrap()
            .contains("https:"));
        assert!(csp(false).contains("script-src 'none'"));
        assert!(csp(false).contains("connect-src 'none'"));
        assert!(!csp(false).contains("https:"));
        for relative in [
            "../outside.txt",
            "%2e%2e/outside.txt",
            "C:/outside.txt",
            "assets%5cfile",
        ] {
            assert!(resolve(&format!("{base}/{relative}")).is_err());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(temp.join("outside.txt"), workspace.join("link.txt"))
                .unwrap();
            assert!(resolve(&format!("{base}/link.txt")).is_err());
        }
        let denied = serve(
            Request::builder()
                .method("POST")
                .uri(uri)
                .body(Vec::new())
                .unwrap(),
        );
        assert_eq!(denied.status(), 405);
        assert!(csp(true).contains("https:"));
        std::fs::remove_dir_all(temp).unwrap();
    }
}
