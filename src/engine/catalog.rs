use super::PROVIDER_ERROR;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Deserialize)]
struct Catalog {
    data: Option<Vec<Model>>,
    error: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct Model {
    id: Option<String>,
    #[serde(default)]
    architecture: Value,
    #[serde(default)]
    output_modalities: Value,
    #[serde(default)]
    supported_endpoint_types: Value,
}

fn exclusively(value: &Value, kinds: &[&str]) -> bool {
    value.as_array().is_some_and(|items| {
        !items.is_empty()
            && items.iter().all(|v| {
                v.as_str()
                    .is_some_and(|s| kinds.iter().any(|kind| kind.eq_ignore_ascii_case(s.trim())))
            })
    })
}

impl Model {
    fn translation_id(&self) -> Option<&str> {
        let id = self.id.as_deref()?.trim();
        if id.is_empty() || id.chars().any(char::is_control) {
            return None;
        }
        let non_text = &["image", "audio", "video", "embedding", "embeddings"];
        if exclusively(&self.output_modalities, non_text)
            || exclusively(&self.architecture["output_modalities"], non_text)
            || exclusively(
                &self.supported_endpoint_types,
                &[
                    "openai-image",
                    "images",
                    "image-generation",
                    "openai-embedding",
                    "embeddings",
                    "rerank",
                    "moderations",
                    "audio",
                    "video",
                ],
            )
        {
            return None;
        }
        (!specialist_name(id)).then_some(id)
    }
}

fn specialist_name(id: &str) -> bool {
    let name = id
        .rsplit('/')
        .next()
        .unwrap_or(id)
        .to_ascii_lowercase()
        .replace('_', "-");
    let family = |prefix: &str| {
        name.strip_prefix(prefix).is_some_and(|tail| {
            tail.as_bytes()
                .first()
                .is_none_or(|c| !c.is_ascii_alphabetic())
        })
    };
    [
        "gpt-image",
        "chatgpt-image",
        "dall-e",
        "dalle",
        "text-to-image",
        "text-to-video",
        "image-to-video",
        "imagen",
        "stable-diffusion",
        "sdxl",
        "flux",
        "midjourney",
        "seedream",
        "sora",
        "veo",
        "whisper",
        "bge",
    ]
    .iter()
    .any(|prefix| family(prefix))
        || name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|part| {
                matches!(
                    part,
                    "embedding"
                        | "embeddings"
                        | "embed"
                        | "rerank"
                        | "reranker"
                        | "moderation"
                        | "tts"
                        | "asr"
                        | "transcribe"
                )
            })
        || (family("gemini") || family("qwen")) && name.split('-').any(|part| part == "image")
        || family("gpt")
            && name
                .split('-')
                .any(|part| matches!(part, "audio" | "realtime"))
}

pub(super) fn parse_models(bytes: &[u8]) -> Result<Vec<String>, String> {
    let malformed = "服务返回的模型列表格式无效。";
    let catalog: Catalog = serde_json::from_slice(bytes).map_err(|_| malformed)?;
    if catalog.error.is_some() {
        return Err(PROVIDER_ERROR.into());
    }
    let rows = catalog.data.ok_or(malformed)?;
    let mut seen = HashSet::new();
    Ok(rows
        .iter()
        .filter_map(Model::translation_id)
        .filter(|id| seen.insert(*id))
        .map(str::to_owned)
        .collect())
}
