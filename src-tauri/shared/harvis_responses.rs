use serde_json::{json, Value};

fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().map(str::trim).filter(|s| !s.is_empty())
}

pub fn normalize_status(raw: &str) -> &'static str {
    match raw.to_ascii_lowercase().as_str() {
        "success" | "succeeded" | "completed" | "complete" | "done" => "succeeded",
        "failed" | "failure" | "error" | "cancelled" | "canceled" | "incomplete" | "unsuccessful" => "failed",
        "queued" | "pending" | "submitted" => "pending",
        _ => "processing",
    }
}

pub fn media_url(value: &str) -> Option<String> {
    let value = value.trim().replace("&amp;", "&").replace("&#38;", "&");
    if value.starts_with("data:image/") || value.starts_with("data:audio/") || value.starts_with("data:video/") {
        return Some(value);
    }
    reqwest::Url::parse(&value).ok()
        .filter(|url| url.scheme() == "https" && url.username().is_empty() && url.password().is_none())
        .map(|_| value)
}

pub fn extract_media(value: &Value, kind: &str) -> Option<String> {
    if let Some(items) = value.as_array() {
        return items.iter().find_map(|item| extract_media(item, kind));
    }
    let keys: &[&str] = match kind {
        "image" => &["image_url", "imageUrl", "cld2ImageUrl", "result_url", "url"],
        "music" => &["audio_url", "audioUrl", "cld2AudioUrl", "mp3_url", "result_url", "url"],
        _ => &["video_url", "videoUrl", "cld2VideoUrl", "result_url", "url"],
    };
    for key in keys {
        if let Some(url) = value_str(value, key).and_then(media_url) {
            return Some(url);
        }
    }
    if let Some(text) = value_str(value, "text") {
        let tag = match kind { "image" => "img", "music" => "audio", _ => "video" };
        let pattern = format!(r#"(?i)<{tag}\b[^>]*\bsrc\s*=\s*["']([^"']+)["']"#);
        if let Some(url) = regex::Regex::new(&pattern).ok()?.captures(text).and_then(|c| media_url(&c[1])) {
            return Some(url);
        }
    }
    for key in ["output", "content", "data", "items", "artifacts"] {
        if let Some(url) = value.get(key).and_then(|item| extract_media(item, kind)) {
            return Some(url);
        }
    }
    None
}

fn reported_progress(response: &Value) -> Option<u8> {
    let value = ["progress", "progress_percent", "progressPercent", "percent_complete", "percentComplete"]
        .into_iter()
        .find_map(|key| response.get(key).and_then(|value| {
            value.as_f64().or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
        }))
        .or_else(|| response.get("data").and_then(reported_progress).map(f64::from))?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let percentage = if value > 0.0 && value < 1.0 { value * 100.0 } else { value };
    Some(percentage.clamp(0.0, 99.0).round() as u8)
}

pub fn task_result(response: &Value, kind: &str, task_id: &str) -> Value {
    let data = response.get("data").filter(|v| v.is_object()).unwrap_or(response);
    let mut status = normalize_status(value_str(response, "status").or_else(|| value_str(data, "status")).unwrap_or(""));
    let result_url = extract_media(response, kind);
    let mut error = response.pointer("/error/message").and_then(Value::as_str)
        .or_else(|| data.pointer("/error/message").and_then(Value::as_str))
        .unwrap_or("").to_string();
    if status == "succeeded" && result_url.is_none() {
        status = "failed";
        error = "渠道已结束任务，但没有返回可用成品；请联系管理员检查任务结果（无需重复提交）".into();
    }
    let progress = if status == "succeeded" { Some(100) } else { reported_progress(response) };
    json!({ "kind": kind, "task_id": task_id, "status": status,
        "progress": progress, "result_url": result_url, "error_message": error })
}

pub fn generation_body(model: &str, kind: &str, request: &Value) -> Result<Value, String> {
    let prompt = value_str(request, "prompt").ok_or("请填写生成提示词")?;
    let mut body = json!({"model": model, "input": prompt, "background": true});
    if kind == "music" {
        body["mvVersion"] = json!("chirp-v5-5");
        body["makeInstrumental"] = json!(true);
        body["title"] = json!(value_str(request, "title").unwrap_or("Harvis Music"));
        body["tags"] = json!(prompt);
        body["customMode"] = json!(true);
        body["inputType"] = json!("20");
        body["prompt"] = json!("[Instrumental]");
    } else if kind == "image" {
        let references = request.get("referenceUrls").and_then(Value::as_array).cloned().unwrap_or_default();
        if value_str(request, "mode") == Some("image") && references.is_empty() {
            return Err("参考图已失效，请重新添加参考图后生成".into());
        }
        if references.len() > 4 || references.iter().any(|v| v.as_str().and_then(media_url).is_none()) {
            return Err("参考图地址无效，最多可使用 4 张参考图".into());
        }
        body["images"] = json!(references);
        let width = request.get("outputWidth").and_then(Value::as_u64).unwrap_or(1024);
        let height = request.get("outputHeight").and_then(Value::as_u64).unwrap_or(1024);
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("图片尺寸须在 1 至 8192 像素之间".into());
        }
        body["size"] = json!(format!("{width}x{height}"));
    }
    if let Some(duration) = request.get("duration") { body["duration"] = duration.clone(); }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_nullable_until_the_provider_reports_it() {
        assert_eq!(task_result(&json!({"status":"queued"}), "image", "resp_1")["progress"], Value::Null);
        assert_eq!(task_result(&json!({"status":"in_progress","progress":0.42}), "image", "resp_1")["progress"], 42);
        assert_eq!(task_result(&json!({"status":"in_progress","progress":65}), "image", "resp_1")["progress"], 65);
    }

    #[test]
    fn completed_media_is_extracted_without_confusing_image_and_audio() {
        let response = json!({"status":"completed","output":[{"content":[{"type":"output_text","text":r#"<audio controls src="https://cdn.example/a.m4a?a=1&amp;b=2"></audio>"#}]}]});
        assert_eq!(extract_media(&response, "music").unwrap(), "https://cdn.example/a.m4a?a=1&b=2");
        assert!(extract_media(&response, "image").is_none());
        assert_eq!(task_result(&response, "music", "resp_1")["status"], "succeeded");
        assert_eq!(task_result(&response, "image", "resp_1")["status"], "failed");
    }
}
