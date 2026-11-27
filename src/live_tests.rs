use crate::engine::{ApiMode, Settings, list_models, translate};
use std::{env, fs};
use tokio::sync::watch;

#[test]
#[ignore = "显式运行，使用真实 API 额度"]
fn new_api_gpt_6_luna() {
    let key = env::var("NEXTAI_TEST_API_KEY").unwrap_or_else(|_| {
        let path = env::var_os("NEXTAI_TEST_KEY_FILE")
            .expect("CI 需要 NEW_API_KEY；本地请通过 T3 提供 NEXTAI_TEST_KEY_FILE");
        fs::read_to_string(path).expect("无法读取临时密钥文件")
    });
    let mut settings = Settings {
        endpoint: "https://new-api.jamesflare.com".into(),
        model: "gpt-6-luna".into(),
        api_key: key.trim().into(),
        ..Settings::default()
    };
    assert!(
        !settings.api_key.is_empty(),
        "实网测试密钥为空；请检查 NEW_API_KEY 或本地密钥文件"
    );
    let models = list_models(&settings).expect("列出模型失败");
    assert!(
        models.contains(&settings.model),
        "端点的模型列表不含 gpt-6-luna"
    );
    let mut failures = Vec::new();
    for api_mode in [ApiMode::ChatCompletions, ApiMode::Responses] {
        for reasoning in [false, true] {
            settings.api_mode = api_mode;
            settings.reasoning = reasoning;
            let mut out = String::new();
            let result = translate(
                &settings,
                "English",
                "Simplified Chinese",
                "The cat is sleeping.",
                &watch::channel(false).1,
                |s| out.push_str(s),
            );
            let outcome = match result {
                Ok(()) if out.contains('猫') && out.contains('睡') => continue,
                Ok(()) => "未收到预期语义的译文".into(),
                Err(message) => message,
            };
            failures.push(format!("{api_mode:?} reasoning={reasoning}: {outcome}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
