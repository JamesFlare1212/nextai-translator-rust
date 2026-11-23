use crate::engine::{ApiMode, Settings, list_models, translate};
use std::{env, fs, sync::atomic::AtomicBool};

#[test]
#[ignore = "显式运行；使用真实 API 额度，密钥由 T3 临时文件提供"]
fn new_api_gpt_6_luna() {
    let path =
        env::var_os("NEXTAI_TEST_KEY_FILE").expect("请通过 T3 密钥卡提供 NEXTAI_TEST_KEY_FILE");
    let mut settings = Settings {
        endpoint: "https://new-api.jamesflare.com".into(),
        model: "gpt-6-luna".into(),
        api_key: fs::read_to_string(path)
            .expect("无法读取临时密钥文件")
            .trim()
            .into(),
        ..Settings::default()
    };
    assert!(!settings.api_key.is_empty());
    let models = list_models(&settings).expect("列出模型失败");
    assert!(
        models.contains(&settings.model),
        "端点的模型列表不含 gpt-6-luna"
    );
    for api_mode in [ApiMode::ChatCompletions, ApiMode::Responses] {
        for reasoning in [false, true] {
            settings.api_mode = api_mode;
            settings.reasoning = reasoning;
            let mut out = String::new();
            translate(
                &settings,
                "English",
                "Simplified Chinese",
                "The cat is sleeping.",
                &AtomicBool::new(false),
                |s| out.push_str(s),
            )
            .unwrap_or_else(|err| panic!("{api_mode:?} reasoning={reasoning}: {err}"));
            assert!(
                out.contains('猫') && out.contains('睡'),
                "{api_mode:?} reasoning={reasoning}: 未收到预期语义的译文"
            );
        }
    }
}
