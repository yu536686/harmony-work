use crate::store::Settings;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tracing::warn;

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
    #[serde(default)]
    negative_tags: Vec<String>,
    #[serde(default)]
    evidence_case_ids: Vec<String>,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default)]
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
        "max_output_tokens": 1600,
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

fn json_payload(text: &str) -> &str {
    let trimmed = text.trim().trim_start_matches('\u{feff}').trim();
    if let Some(fenced) = trimmed.strip_prefix("```") {
        let body = fenced
            .split_once('\n')
            .map(|(_, body)| body)
            .unwrap_or(fenced);
        return body.rsplit_once("```").map(|(body, _)| body.trim()).unwrap_or(body.trim());
    }
    if trimmed.starts_with('"') {
        return trimmed;
    }
    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(start), Some(end)) if start < end => &trimmed[start..=end],
        _ => trimmed,
    }
}

fn first_field<'a>(object: &'a Map<String, Value>, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| object.get(*name))
}

fn scalar_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => (!value.trim().is_empty()).then(|| value.trim().to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| scalar_text(Some(value)))
            .filter(|value| !value.is_empty())
            .collect(),
        Some(Value::String(value)) => value
            .split(|character| matches!(character, ',' | '，' | ';' | '；' | '\n' | '|'))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(value) => scalar_text(Some(value)).into_iter().collect(),
        None => Vec::new(),
    }
}

fn numeric_values(value: &str) -> Vec<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|value| !value.is_empty())
        .filter_map(|value| value.parse::<u32>().ok())
        .collect()
}

fn u32_value(value: Option<&Value>) -> Option<u32> {
    match value? {
        Value::Number(value) => value.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(value) => numeric_values(value).into_iter().next(),
        _ => None,
    }
}

fn loose_bpm(value: Option<&Value>) -> HarvisBpm {
    let values = match value {
        Some(Value::Object(object)) => {
            let recommended = u32_value(first_field(object, &["recommended", "value", "bpm"]));
            let min = u32_value(first_field(object, &["min", "from"]));
            let max = u32_value(first_field(object, &["max", "to"]));
            let recommended = recommended.or_else(|| match (min, max) {
                (Some(min), Some(max)) => Some((min + max) / 2),
                (Some(value), None) | (None, Some(value)) => Some(value),
                (None, None) => None,
            });
            vec![min, max, recommended]
        }
        Some(Value::String(value)) => numeric_values(value).into_iter().map(Some).collect(),
        Some(Value::Number(_)) => vec![u32_value(value)],
        _ => Vec::new(),
    };
    let mut numbers = values.into_iter().flatten().collect::<Vec<_>>();
    let recommended = match numbers.len() {
        0 => 72,
        1 => numbers[0],
        2 => (numbers[0] + numbers[1]) / 2,
        _ => numbers.pop().unwrap_or(72),
    }
    .clamp(40, 220);
    let min = numbers.first().copied().unwrap_or(recommended.saturating_sub(6)).clamp(40, 220);
    let max = numbers.get(1).copied().unwrap_or(recommended + 6).clamp(min, 220);
    HarvisBpm { min, max, recommended: recommended.clamp(min, max) }
}

fn decode_response_value(text: &str) -> Result<Value, String> {
    let mut value = serde_json::from_str::<Value>(json_payload(text))
        .map_err(|error| format!("模型结果不是可解析的 JSON：{error}"))?;
    for _ in 0..2 {
        let Value::String(encoded_json) = value else { break };
        value = serde_json::from_str::<Value>(json_payload(&encoded_json))
            .map_err(|error| format!("模型结果中的 JSON 字符串无法解析：{error}"))?;
    }
    Ok(value)
}

fn coerce_loose_harvis_prompt_result(value: Value) -> Result<RawHarvisPromptResult, String> {
    let object = value.as_object().ok_or_else(|| "模型结果 JSON 不是对象".to_string())?;
    let analysis_zh = scalar_text(first_field(object, &["analysisZh", "analysis", "analysis_zh", "reasoning"]))
        .ok_or_else(|| "模型结果缺少中文创作分析".to_string())?;
    let suno_prompt_en = scalar_text(first_field(object, &["sunoPromptEn", "sunoPrompt", "prompt", "promptEn", "suno_prompt_en"]))
        .ok_or_else(|| "模型结果缺少英文 Suno 提示词".to_string())?;
    let suno_prompt_zh = scalar_text(first_field(object, &["sunoPromptZh", "promptZh", "prompt_zh", "suno_prompt_zh"]))
        .unwrap_or_else(|| analysis_zh.clone());
    let confidence = first_field(object, &["confidence", "score"])
        .and_then(|value| match value {
            Value::Number(value) => value.as_f64(),
            Value::String(value) => value.trim_end_matches('%').parse::<f64>().ok().map(|value| if value > 1.0 { value / 100.0 } else { value }),
            _ => None,
        });
    serde_json::from_value(json!({
        "analysisZh": analysis_zh,
        "genre": string_list(first_field(object, &["genre", "genres", "style"])),
        "mood": string_list(first_field(object, &["mood", "moods"])),
        "instruments": string_list(first_field(object, &["instruments", "instrument"])),
        "bpm": loose_bpm(first_field(object, &["bpm", "tempo"])),
        "durationSeconds": u32_value(first_field(object, &["durationSeconds", "duration", "length"])).unwrap_or(45),
        "sunoPromptEn": suno_prompt_en,
        "sunoPromptZh": suno_prompt_zh,
        "negativeTags": string_list(first_field(object, &["negativeTags", "negative", "exclude"])),
        "evidenceCaseIds": string_list(first_field(object, &["evidenceCaseIds", "evidence", "caseIds"])),
        "confidence": confidence,
        "warnings": string_list(first_field(object, &["warnings", "warning"])),
    }))
    .map_err(|error| format!("模型结果兼容归一化失败：{error}"))
}

fn parse_raw_harvis_prompt_result(text: &str) -> Result<RawHarvisPromptResult, String> {
    let value = decode_response_value(text)?;
    serde_json::from_value(value.clone())
        .or_else(|_| coerce_loose_harvis_prompt_result(value))
        .map_err(|error| format!("模型结果字段不符合提示词结构：{error}"))
}

fn compact_validation_reason(reason: &str) -> String {
    let compact = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(180).collect()
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
        confidence: raw.confidence.unwrap_or(0.68).clamp(0.0, 1.0),
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
    let first_result = parse_raw_harvis_prompt_result(&first_text)
        .and_then(|raw| normalize_result(raw, &input, model));
    match first_result {
        Ok(result) => Ok(result),
        Err(error) => {
            let repaired_result = request_once(&client, endpoint, api_key, request_body(model, &input, Some(&error))).await
                .and_then(|response| output_text(&response).ok_or_else(|| "修复请求没有返回文本内容".to_string()))
                .and_then(|text| parse_raw_harvis_prompt_result(&text))
                .and_then(|raw| normalize_result(raw, &input, model));
            match repaired_result {
                Ok(result) => Ok(result),
                Err(repair_error) => {
                    warn!(
                        provider = %provider,
                        model = %model,
                        first_error = %compact_validation_reason(&error),
                        repair_error = %compact_validation_reason(&repair_error),
                        "Harvis music prompt remained degraded after structured-output repair"
                    );
                    Ok(fallback_result(
                        &input,
                        model,
                        &format!(
                            "模型结构化输出异常，已使用可编辑模板降级：首次 {}；修复请求 {}",
                            compact_validation_reason(&error),
                            compact_validation_reason(&repair_error),
                        ),
                    ))
                }
            }
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
    fn parses_a_schema_result_wrapped_in_a_json_code_fence() {
        let raw = parse_raw_harvis_prompt_result(
            "```json\n{\"analysisZh\":\"这是一段可用的中文创作说明。\",\"genre\":[\"neo-classical\"],\"mood\":[\"elegant\"],\"instruments\":[\"piano\",\"harp\"],\"bpm\":{\"min\":68,\"max\":76,\"recommended\":72},\"durationSeconds\":45,\"sunoPromptEn\":\"Instrumental luxury watch campaign, piano and harp, no vocals, elegant restrained arc.\",\"sunoPromptZh\":\"适用于高端腕表广告的纯音乐，钢琴与竖琴营造优雅克制的情绪递进。\"}\n```",
        )
        .expect("code fence should not force a fallback");

        assert_eq!(raw.duration_seconds, 45);
        assert!(raw.negative_tags.is_empty());
        assert!(raw.warnings.is_empty());
        assert_eq!(raw.confidence, None);
    }

    #[test]
    fn parses_a_schema_result_when_a_compatibility_gateway_double_encodes_json() {
        let raw = parse_raw_harvis_prompt_result(
            "\"{\\\"analysisZh\\\":\\\"这是一段可用的中文创作说明。\\\",\\\"genre\\\":[\\\"neo-classical\\\"],\\\"mood\\\":[\\\"elegant\\\"],\\\"instruments\\\":[\\\"piano\\\",\\\"harp\\\"],\\\"bpm\\\":{\\\"min\\\":68,\\\"max\\\":76,\\\"recommended\\\":72},\\\"durationSeconds\\\":45,\\\"sunoPromptEn\\\":\\\"Instrumental luxury watch campaign, piano and harp, no vocals, elegant restrained arc.\\\",\\\"sunoPromptZh\\\":\\\"适用于高端腕表广告的纯音乐，钢琴与竖琴营造优雅克制的情绪递进。\\\"}\"",
        )
        .expect("double-encoded JSON should not force a fallback");

        assert_eq!(raw.bpm.recommended, 72);
    }

    #[test]
    fn recovers_a_usable_prompt_from_a_common_loose_model_shape() {
        let raw = parse_raw_harvis_prompt_result(
            r#"{
              "analysis": "以偏心陀飞轮的机械细节为重点，采用克制的高级感配乐衬托镜头节奏。",
              "genres": "neo-classical, luxury instrumental",
              "moods": ["elegant", "restrained"],
              "instruments": "grand piano, harp, subtle strings",
              "bpm": "70-78 BPM",
              "duration": "45 seconds",
              "prompt": "Instrumental only luxury watch campaign, neo-classical, 74 BPM, grand piano, harp and subtle strings, restrained elegant build, premium advertising mix, no vocals, no spoken words.",
              "promptZh": "高端腕表广告纯音乐，以钢琴、竖琴和克制弦乐建立优雅递进，不使用任何人声。"
            }"#,
        )
        .expect("a usable loose model shape should not force fallback");

        assert_eq!(raw.duration_seconds, 45);
        assert_eq!(raw.bpm.recommended, 74);
        assert_eq!(raw.genre, vec!["neo-classical", "luxury instrumental"]);
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
