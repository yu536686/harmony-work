use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

pub fn configured_model() -> Option<String> {
    std::env::var("WORKBUDDY_MEMORY_EMBEDDING_MODEL")
        .ok()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
}

pub async fn embed_many(model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
    let key = std::env::var("HARMONY_API_KEY")
        .context("Harmony API Key is not configured for memory embeddings")?;
    if key.trim().is_empty() {
        return Err(anyhow!("Harmony API Key is empty"));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()?;
    let response = client
        .post(format!("{}/embeddings", crate::service_urls::HARMONY_API_BASE))
        .bearer_auth(key)
        .json(&json!({"model": model, "input": inputs}))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!("Harmony embedding request failed with HTTP {status}"));
    }
    parse_embeddings(&response.json::<Value>().await?, inputs.len())
}

fn parse_embeddings(response: &Value, expected: usize) -> Result<Vec<Vec<f32>>> {
    let rows = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("embedding response has no data array"))?;
    if rows.len() != expected {
        return Err(anyhow!("embedding response count mismatch"));
    }
    let mut vectors = vec![Vec::new(); expected];
    for row in rows {
        let index = row
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("embedding response is missing an index"))? as usize;
        if index >= expected || !vectors[index].is_empty() {
            return Err(anyhow!("embedding response has duplicate or invalid indices"));
        }
        let values = row
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("embedding response has no vector"))?;
        vectors[index] = values
            .iter()
            .map(|value| {
                let number = value.as_f64().ok_or_else(|| anyhow!("invalid embedding value"))?;
                if !number.is_finite() {
                    return Err(anyhow!("non-finite embedding value"));
                }
                Ok(number as f32)
            })
            .collect::<Result<Vec<_>>>()?;
        if vectors[index].is_empty() {
            return Err(anyhow!("empty embedding vector"));
        }
    }
    if vectors.iter().any(Vec::is_empty) {
        return Err(anyhow!("embedding response is incomplete"));
    }
    Ok(vectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_indexed_embedding_response_in_input_order() {
        let response = json!({"data":[
            {"index":1,"embedding":[0.0,1.0]},
            {"index":0,"embedding":[1.0,0.0]}
        ]});
        assert_eq!(parse_embeddings(&response, 2).unwrap(), vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        assert!(parse_embeddings(&response, 1).is_err());
    }
}
