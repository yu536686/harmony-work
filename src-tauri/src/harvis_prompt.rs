use crate::store::Settings;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarvisPromptInput {
    pub business_line: String,
    pub brand: String,
    pub watch_collection: String,
    pub watch_style: Option<String>,
    pub desired_style: Option<String>,
    pub bpm: Option<u32>,
    pub duration: u32,
    #[serde(default)]
    pub campaign_context: Vec<String>,
    #[serde(default)]
    pub evidence_cases: Vec<HarvisEvidenceCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarvisEvidenceCase {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub mood: String,
    #[serde(default)]
    pub instruments: String,
    #[serde(default)]
    pub bpm_range: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarvisPromptResult {
    pub prompt: String,
    pub analysis_zh: String,
    pub genre: Vec<String>,
    pub mood: Vec<String>,
    pub instruments: Vec<String>,
    pub bpm: HarvisBpm,
    pub duration_seconds: u32,
    pub prompt_zh: String,
    pub negative_tags: Vec<String>,
    pub evidence_case_ids: Vec<String>,
    pub confidence: f64,
    pub warnings: Vec<String>,
    pub model: String,
    pub evidence_cases: Vec<HarvisEvidenceCase>,
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarvisBpm {
    pub min: u32,
    pub max: u32,
    pub recommended: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawHarvisPromptResult {
    analysis_zh: String,
    genre: Vec<String>,
    mood: Vec<String>,
    instruments: Vec<String>,
    bpm: HarvisBpm,
    duration_seconds: u32,
    suno_prompt_en: String,
    suno_prompt_zh: String,
    negative_tags: Vec<String>,
    evidence_case_ids: Vec<String>,
    confidence: f64,
    warnings: Vec<String>,
}

const RESPONSE_TIMEOUT_SECONDS: u64 = 40;

fn responses_endpoint(provider: &str) -> Result<&'static str, String> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "deepseek" => Ok("https://api.deepseek.com/responses"),
        "openai" => Ok("https://api.openai.com/v1/responses"),
        other => Err(format!(
            "Harvis 提示词助手需要 OpenAI 或 DeepSeek Responses API，当前提供商“{other}”不受支持"
        )),
    }
}

fn trim_field(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .chars()
        .take(160)
        .collect()
}

fn assistant_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "analysisZh", "genre", "mood", "instruments", "bpm", "durationSeconds",
            "sunoPromptEn", "sunoPromptZh", "negativeTags", "evidenceCaseIds", "confidence", "warnings"
        ],
        "properties": {
            "analysisZh": { "type": "string", "minLength": 8, "maxLength": 480 },
            "genre": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 4 },
            "mood": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 4 },
            "instruments": { "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 5 },
            "bpm": {
                "type": "object",
                "additionalProperties": false,
                "required": ["min", "max", "recommended"],
                "properties": {
                    "min": { "type": "integer", "minimum": 40, "maximum": 220 },
                    "max": { "type": "integer", "minimum": 40, "maximum": 220 },
                    "recommended": { "type": "integer", "minimum": 40, "maximum": 220 }
                }
            },
            "durationSeconds": { "type": "integer", "minimum": 10, "maximum": 360 },
            "sunoPromptEn": { "type": "string", "minLength": 30, "maxLength": 1400 },
            "sunoPromptZh": { "type": "string", "minLength": 12, "maxLength": 900 },
            "negativeTags": { "type": "array", "items": { "type": "string" }, "maxItems": 12 },
            "evidenceCaseIds": { "type": "array", "items": { "type": "string" }, "maxItems": 6 },
            "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
            "warnings": { "type": "array", "items": { "type": "string" }, "maxItems": 6 }
        }
    })
}

fn request_body(model: &str, input: &HarvisPromptInput, repair: Option<&str>) -> Value {
    let allowed_case_ids = input
        .evidence_cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<Vec<_>>();
    let brief = json!({
        "businessLine": input.business_line,
        "brand": input.brand,
        "watchCollection": input.watch_collection,
        "watchStyle": trim_field(input.watch_style.as_deref(), "由助手判断"),
        "desiredStyle": trim_field(input.desired_style.as_deref(), "由助手判断"),
        "requestedBpm": input.bpm,
        "durationSeconds": input.duration,
        "campaignContext": input.campaign_context,
        "evidenceCases": input.evidence_cases,
        "allowedEvidenceCaseIds": allowed_case_ids,
    });
    let repair_note = repair
        .map(|value| format!("\n上一次输出未能通过结构校验。仅修复 JSON 结构和无效字段：{value}"))
        .unwrap_or_default();
    let system = format!(
        "You are Xiaoheng WorkBuddy's Harvis music prompt assistant. Create a premium, instrumental-only Suno V5.5 BGM plan for a watch or brand campaign. Do not imitate named artists. Do not use lyrics, brand names as sung phrases, vocals, humming, speech, chanting, or markdown. Produce only JSON that matches the supplied schema. Evidence case IDs may only come from allowedEvidenceCaseIds. The English prompt must include genre, mood, BPM, 2-5 instruments, a compact arrangement arc, premium advertising mix, and instrumental-only wording. The Chinese prompt is an accurate business explanation, not a literal translation. Match durationSeconds to the requested value.{repair_note}"
    );
    json!({
        "model": model,
        "input": [
            { "role": "system", "content": [{ "type": "input_text", "text": system }] },
            { "role": "user", "content": [{ "type": "input_text", "text": brief.to_string() }] }
        ],
        "text": {
            "format": {
                "type": "json_schema",
                "name": "harvis_music_prompt",
                "strict": true,
                "schema": assistant_schema()
            }
        },
        "max_output_tokens": 1100,
        "temperature": 0.45
    })
}

fn output_text(response: &Value) -> Option<String> {
    if let Some(text) = response.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|item| item.get("content").and_then(Value::as_array).into_iter().flatten())
        .find_map(|content| {
            content
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| content.get("output_text").and_then(Value::as_str))
                .map(str::to_string)
        })
}

fn normalize_result(raw: RawHarvisPromptResult, input: &HarvisPromptInput, model: &str) -> Result<HarvisPromptResult, String> {
    let prompt = raw.suno_prompt_en.trim().to_string();
    let analysis_zh = raw.analysis_zh.trim().to_string();
    if prompt.chars().count() < 30 || analysis_zh.chars().count() < 8 {
        return Err("模型返回的提示词或中文分析过短".to_string());
    }
    let duration = input.duration.clamp(10, 360);
    let mut warnings = raw.warnings;
    if raw.duration_seconds != duration {
        warnings.push("模型返回时长已按用户选择校正".to_string());
    }
    let mut evidence_case_ids = raw
        .evidence_case_ids
        .into_iter()
        .filter(|id| input.evidence_cases.iter().any(|case| case.id == *id))
        .take(6)
        .collect::<Vec<_>>();
    evidence_case_ids.sort();
    evidence_case_ids.dedup();
    let evidence_cases = input
        .evidence_cases
        .iter()
        .filter(|case| evidence_case_ids.iter().any(|id| id == &case.id))
        .cloned()
        .collect();
    let bpm_min = raw.bpm.min.clamp(40, 220);
    let bpm_max = raw.bpm.max.clamp(bpm_min, 220);
    Ok(HarvisPromptResult {
        prompt,
        analysis_zh,
        genre: raw.genre.into_iter().filter(|value| !value.trim().is_empty()).take(4).collect(),
        mood: raw.mood.into_iter().filter(|value| !value.trim().is_empty()).take(4).collect(),
        instruments: raw.instruments.into_iter().filter(|value| !value.trim().is_empty()).take(5).collect(),
        bpm: HarvisBpm { min: bpm_min, max: bpm_max, recommended: raw.bpm.recommended.clamp(bpm_min, bpm_max) },
        duration_seconds: duration,
        prompt_zh: raw.suno_prompt_zh.trim().to_string(),
        negative_tags: raw.negative_tags.into_iter().filter(|value| !value.trim().is_empty()).take(12).collect(),
        evidence_case_ids,
        confidence: raw.confidence.clamp(0.0, 1.0),
        warnings: warnings.into_iter().filter(|value| !value.trim().is_empty()).take(6).collect(),
        model: model.to_string(),
        evidence_cases,
        degraded: false,
    })
}

fn fallback_result(input: &HarvisPromptInput, model: &str, reason: &str) -> HarvisPromptResult {
    let recommended = input.bpm.unwrap_or(76).clamp(40, 220);
    let genre = trim_field(input.desired_style.as_deref(), "neo-classical luxury instrumental");
    let watch_style = trim_field(input.watch_style.as_deref(), "refined watch details");
    let evidence_cases = input.evidence_cases.iter().take(3).cloned().collect::<Vec<_>>();
    HarvisPromptResult {
        prompt: format!(
            "Instrumental only, {genre}, premium luxury watch campaign for {} {}, {watch_style}, {} BPM, grand piano, harp, subtle strings, elegant restrained emotional arc, clean advertising mix, no vocals, no spoken words",
            input.brand, input.watch_collection, recommended
        ),
        analysis_zh: "模型结果未通过完整结构校验，已根据已选业务信息和知识案例生成可编辑的保守模板。".to_string(),
        genre: vec![genre],
        mood: vec!["克制".to_string(), "高级".to_string()],
        instruments: vec!["钢琴".to_string(), "竖琴".to_string(), "弦乐".to_string()],
        bpm: HarvisBpm { min: recommended.saturating_sub(6).max(40), max: (recommended + 6).min(220), recommended },
        duration_seconds: input.duration.clamp(10, 360),
        prompt_zh: "纯音乐、奢华腕表广告配乐，以钢琴、竖琴和克制弦乐建立优雅递进，不使用人声或口白。".to_string(),
        negative_tags: vec!["vocals".to_string(), "speech".to_string(), "humming".to_string()],
        evidence_case_ids: evidence_cases.iter().map(|case| case.id.clone()).collect(),
        confidence: 0.45,
        warnings: vec![reason.to_string()],
        model: model.to_string(),
        evidence_cases,
        degraded: true,
    }
}

async fn request_once(client: &Client, endpoint: &str, api_key: &str, body: Value) -> Result<Value, String> {
    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Harvis 提示词助手请求失败：{error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("Harvis 提示词助手返回 HTTP {status}: {}", body.chars().take(600).collect::<String>()));
    }
    serde_json::from_str(&body).map_err(|error| format!("Harvis 提示词助手返回了无效 JSON：{error}"))
}

pub async fn compose_harvis_music_prompt(settings: &Settings, input: HarvisPromptInput) -> Result<HarvisPromptResult, String> {
    let provider = settings.provider.trim().to_ascii_lowercase();
    let endpoint = responses_endpoint(&provider)?;
    let model = settings.model.trim();
    let api_key = settings.active_api_key().trim();
    if model.is_empty() || api_key.is_empty() {
        return Err("请先在设置中配置 DeepSeek 或 OpenAI 的默认模型与 API 密钥，再使用 Harvis 提示词助手".to_string());
    }
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(RESPONSE_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| format!("无法创建 Harvis 提示词助手客户端：{error}"))?;
    let first = request_once(&client, endpoint, api_key, request_body(model, &input, None)).await?;
    let first_text = output_text(&first).ok_or_else(|| "Harvis 提示词助手没有返回文本内容".to_string())?;
    let first_result = serde_json::from_str::<RawHarvisPromptResult>(&first_text)
        .map_err(|error| error.to_string())
        .and_then(|raw| normalize_result(raw, &input, model));
    match first_result {
        Ok(result) => Ok(result),
        Err(error) => {
            let repaired = request_once(&client, endpoint, api_key, request_body(model, &input, Some(&error.to_string()))).await;
            let result = repaired
                .ok()
                .and_then(|response| output_text(&response))
                .and_then(|text| serde_json::from_str::<RawHarvisPromptResult>(&text).ok())
                .and_then(|raw| normalize_result(raw, &input, model).ok());
            Ok(result.unwrap_or_else(|| fallback_result(&input, model, "模型结构化输出异常，已使用可编辑模板降级")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_responses_output_text_from_standard_payload() {
        let response = json!({
            "output": [{ "content": [{ "type": "output_text", "text": "{\"analysisZh\":\"ok\"}" }] }]
        });
        assert_eq!(output_text(&response).as_deref(), Some("{\"analysisZh\":\"ok\"}"));
    }

    #[test]
    fn fallback_keeps_requested_duration_and_is_marked_degraded() {
        let input = HarvisPromptInput {
            business_line: "腕表如诗".to_string(),
            brand: "宝玑".to_string(),
            watch_collection: "Classique".to_string(),
            watch_style: None,
            desired_style: None,
            bpm: Some(72),
            duration: 45,
            campaign_context: vec![],
            evidence_cases: vec![],
        };
        let result = fallback_result(&input, "deepseek-v4-pro", "test");
        assert_eq!(result.duration_seconds, 45);
        assert!(result.degraded);
        assert!(result.prompt.contains("no vocals"));
    }
}
