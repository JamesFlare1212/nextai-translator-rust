pub struct Provider {
    pub name: &'static str,
    pub endpoint: &'static str,
    pub model: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        name: "OpenAI",
        endpoint: "https://api.openai.com/v1/chat/completions",
        model: "gpt-4o-mini",
    },
    Provider {
        name: "Ollama（本地模型）",
        endpoint: "http://127.0.0.1:11434/v1/chat/completions",
        model: "mistral",
    },
    Provider {
        name: "DeepSeek",
        endpoint: "https://api.deepseek.com/chat/completions",
        model: "deepseek-chat",
    },
    Provider {
        name: "Gemini",
        endpoint: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
        model: "gemini-2.5-flash",
    },
    Provider {
        name: "Groq",
        endpoint: "https://api.groq.com/openai/v1/chat/completions",
        model: "llama-3.3-70b-versatile",
    },
];

pub const CUSTOM: &str = "自定义（OpenAI 兼容）";

pub fn identify(url: &str) -> &'static str {
    PROVIDERS
        .iter()
        .find(|p| p.endpoint == url.trim())
        .map_or(CUSTOM, |p| p.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Settings, validate};

    #[test]
    fn all_presets_have_valid_translation_endpoints() {
        for p in PROVIDERS {
            let s = Settings {
                endpoint: p.endpoint.into(),
                model: p.model.into(),
                ..Settings::default()
            };
            assert!(validate(&s).is_ok());
            assert_eq!(identify(&s.endpoint), p.name);
        }
        assert_eq!(
            identify("http://localhost:9090/v1/chat/completions"),
            CUSTOM
        );
    }
}
