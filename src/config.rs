use crate::engine::{ApiMode, Settings};

pub const DEFAULT_FONT: &str = "MiSans";
pub const DEFAULT_SIZE: u8 = 15;
pub const FONT_SIZES: std::ops::RangeInclusive<u8> = 10..=36;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    ptr, slice,
};
use tempfile::NamedTempFile;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};

#[derive(Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Preferences {
    pub appearance: Appearance,
    pub target: String,
    pub font_family: String,
    pub font_size: u8,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            target: "Simplified Chinese".into(),
            font_family: DEFAULT_FONT.into(),
            font_size: DEFAULT_SIZE,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Stored {
    endpoint: String,
    model: String,
    #[serde(default)]
    api_mode: ApiMode,
    #[serde(default)]
    reasoning: bool,
    protected_key: Vec<u8>,
    #[serde(default)]
    preferences: Preferences,
}

pub(crate) fn config_file() -> Result<PathBuf, String> {
    env::var_os("APPDATA")
        .map(|dir| {
            PathBuf::from(dir)
                .join("NextAI Translator Rust")
                .join("settings.json")
        })
        .ok_or_else(|| "找不到 Windows 用户配置目录。".into())
}

pub(crate) fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().map_err(|_| "密钥过长。")?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
    };
    if ok == 0 {
        return Err("无法使用当前 Windows 帐户保护或读取 API 密钥。".into());
    }
    unsafe {
        let data = slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData.cast());
        Ok(data)
    }
}

fn read_at(path: &Path) -> Result<(Settings, Preferences), String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Default::default()),
        Err(_) => return Err("无法读取设置文件。".into()),
    };
    let stored: Stored =
        serde_json::from_slice(&bytes).map_err(|_| "设置文件已损坏，请重新保存设置。")?;
    let api_key = String::from_utf8(crypt(&stored.protected_key, false)?)
        .map_err(|_| "无法读取已保存的 API 密钥。")?;
    let mut preferences = stored.preferences;
    preferences.font_size = preferences
        .font_size
        .clamp(*FONT_SIZES.start(), *FONT_SIZES.end());
    if preferences.font_family.trim().is_empty() {
        preferences.font_family = DEFAULT_FONT.into();
    }
    Ok((
        Settings {
            endpoint: stored.endpoint,
            model: stored.model,
            api_key,
            api_mode: stored.api_mode,
            reasoning: stored.reasoning,
        },
        preferences,
    ))
}

fn write_at(path: &Path, settings: &Settings, preferences: &Preferences) -> Result<(), String> {
    let dir = path.parent().ok_or("无效的设置文件位置。")?;
    fs::create_dir_all(dir).map_err(|_| "无法创建设置目录。")?;
    let stored = Stored {
        endpoint: settings.endpoint.clone(),
        model: settings.model.clone(),
        api_mode: settings.api_mode,
        reasoning: settings.reasoning,
        protected_key: crypt(settings.api_key.as_bytes(), true)?,
        preferences: preferences.clone(),
    };
    let mut file = NamedTempFile::new_in(dir).map_err(|_| "设置目录不可写。")?;
    serde_json::to_writer(file.as_file_mut(), &stored).map_err(|_| "无法写入设置文件。")?;
    file.as_file_mut()
        .sync_all()
        .map_err(|_| "无法保存设置文件。")?;
    file.persist(path).map_err(|_| "无法替换设置文件。")?;
    Ok(())
}

pub fn load() -> Result<(Settings, Preferences), String> {
    read_at(&config_file()?)
}

pub fn save(settings: &Settings, preferences: &Preferences) -> Result<(), String> {
    crate::engine::validate(settings)?;
    write_at(&config_file()?, settings, preferences)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_under_current_windows_account_without_plaintext_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("子目录/settings.json");
        let (mut s, mut preferences) = read_at(&path).unwrap();
        preferences.appearance = Appearance::Dark;
        preferences.font_family = "Segoe UI".into();
        preferences.font_size = 22;
        s.reasoning = true;
        s.api_mode = ApiMode::Responses;
        s.api_key = "synthetic-key-123".to_owned();
        s.model = "local-model".into();
        write_at(&path, &s, &preferences).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains(&s.api_key));
        let (saved, actual) = read_at(&path).unwrap();
        assert_eq!(actual, preferences);
        assert_eq!(saved.api_mode, ApiMode::Responses);
        assert!(saved.reasoning);
        assert_eq!(
            (saved.model, saved.api_key),
            (s.model.clone(), s.api_key.clone())
        );
        s.api_key.clear();
        write_at(&path, &s, &preferences).unwrap();
        assert!(read_at(&path).unwrap().0.api_key.is_empty());
    }

    #[test]
    fn old_preferences_keep_model_and_gain_new_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"endpoint":"https://example.org/v1/chat/completions","model":"my-model","protected_key":[],"preferences":{"appearance":"dark","target":"French"}}"#).unwrap();
        let (settings, prefs) = read_at(&path).unwrap();
        assert_eq!(settings.model, "my-model");
        assert_eq!(settings.api_mode, ApiMode::ChatCompletions);
        assert!(!settings.reasoning);
        assert_eq!(prefs.target, "French");
        assert_eq!(
            (prefs.font_family.as_str(), prefs.font_size),
            ("MiSans", 15)
        );
    }

    #[test]
    fn invalid_file_is_not_silently_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "invalid").unwrap();
        assert!(read_at(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "invalid");
        assert!(crypt(b"not a DPAPI blob", false).is_err());
    }
}
