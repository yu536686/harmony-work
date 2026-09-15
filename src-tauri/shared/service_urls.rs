#![allow(dead_code)]
pub const HARMONY_SERVER: &str = "https://api.harmonywatch.cn";
pub const HARMONY_API_BASE: &str = "https://api.harmonywatch.cn/v1";
pub const MARKETING_BASE: &str = "https://portal.harmonywatch.cn";
pub const MARKET_BASE: &str = "https://market.harmonywatch.cn";

/// Migrate known public origins only. Never rewrite LAN or arbitrary custom hosts.
pub fn migrate_public_service_url(raw: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw) else {
        return raw.to_string();
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return raw.to_string();
    }
    let target = match (url.host_str().unwrap_or(""), url.port()) {
        ("api.harmonywatch.cn", None | Some(13001)) => "api.harmonywatch.cn",
        ("www.harmonywatch.cn" | "portal.harmonywatch.cn" | "61.150.31.150", Some(8090)) => {
            "portal.harmonywatch.cn"
        }
        ("portal.harmonywatch.cn", None) => "portal.harmonywatch.cn",
        ("www.harmonywatch.cn" | "market.harmonywatch.cn" | "61.150.31.150", Some(8787)) => {
            "market.harmonywatch.cn"
        }
        ("market.harmonywatch.cn", None) => "market.harmonywatch.cn",
        _ => return raw.to_string(),
    };
    if url.scheme() == "https" && url.host_str() == Some(target) && url.port().is_none() {
        return raw.to_string();
    }
    let omit_slash = url.path() == "/"
        && !raw.ends_with('/')
        && url.query().is_none()
        && url.fragment().is_none();
    let _ = url.set_scheme("https");
    let _ = url.set_host(Some(target));
    let _ = url.set_port(None);
    let result = url.to_string();
    if omit_slash {
        result.trim_end_matches('/').to_string()
    } else {
        result
    }
}

pub fn marketing_base_url() -> String {
    std::env::var("WORKBUDDY_MARKETING_URL")
        .ok()
        .map(|value| {
            migrate_public_service_url(value.trim())
                .trim_end_matches('/')
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| MARKETING_BASE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrades_public_origins_preserving_paths_and_tokens() {
        for (input, output) in [
            ("https://api.harmonywatch.cn:13001/v1", HARMONY_API_BASE),
            (
                "http://61.150.31.150:8090/api/assets/a?token=x%2Fy#p",
                "https://portal.harmonywatch.cn/api/assets/a?token=x%2Fy#p",
            ),
            ("http://www.harmonywatch.cn:8090", MARKETING_BASE),
            (
                "http://61.150.31.150:8787/api/skills",
                "https://market.harmonywatch.cn/api/skills",
            ),
        ] {
            assert_eq!(migrate_public_service_url(input), output);
            assert_eq!(migrate_public_service_url(output), output);
        }
    }
    #[test]
    fn preserves_lan_custom_and_unrelated_urls() {
        for url in [
            "http://192.168.11.29:8090",
            "http://192.168.11.33:5244/file",
            "https://custom.example:13001/v1",
            "https://api.harmonywatch.cn.evil.example:13001/v1",
            "https://api.harmonywatch.cn:9443/v1",
            "",
            "/api/assets/x",
        ] {
            assert_eq!(migrate_public_service_url(url), url);
        }
    }
}
