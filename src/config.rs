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
    pub source: String,
    pub font_family: String,
    pub font_size: u8,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            target: "Simplified Chinese".into(),
            source: "English".into(),
            font_family: DEFAULT_FONT.into(),
            font_size: DEFAULT_SIZE,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    connection: Connection,
    #[serde(default)]
    active_provider: String,
    #[serde(default)]
    other_providers: std::collections::BTreeMap<String, Connection>,
    #[serde(default)]
    preferences: Preferences,
}

#[derive(Clone, Default)]
pub struct Profiles {
    pub active: String,
    pub others: std::collections::BTreeMap<String, Settings>,
}

#[derive(Serialize, Deserialize)]
struct Connection {
    endpoint: String,
    model: String,
    #[serde(default)]
    api_mode: ApiMode,
    #[serde(default)]
    reasoning: bool,
    protected_key: Vec<u8>,
}

impl Connection {
    fn protect(s: &Settings) -> Result<Self, String> {
        Ok(Self {
            endpoint: s.endpoint.clone(),
            model: s.model.clone(),
            api_mode: s.api_mode,
            reasoning: s.reasoning,
            protected_key: crypt(s.api_key.as_bytes(), true)?,
        })
    }
    fn open(self) -> Result<Settings, String> {
        Ok(Settings {
            endpoint: self.endpoint,
            model: self.model,
            api_mode: self.api_mode,
            reasoning: self.reasoning,
            api_key: String::from_utf8(crypt(&self.protected_key, false)?)
                .map_err(|_| "无法读取已保存的 API 密钥。")?,
        })
    }
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

fn read_at(path: &Path) -> Result<(Settings, Preferences, Profiles), String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Default::default()),
        Err(_) => return Err("无法读取设置文件。".into()),
    };
    let stored: Stored =
        serde_json::from_slice(&bytes).map_err(|_| "设置文件已损坏，请重新保存设置。")?;

    let mut preferences = stored.preferences;
    preferences.font_size = preferences
        .font_size
        .clamp(*FONT_SIZES.start(), *FONT_SIZES.end());
    if preferences.font_family.trim().is_empty() {
        preferences.font_family = DEFAULT_FONT.into();
    }
    Ok((
        stored.connection.open()?,
        preferences,
        Profiles {
            active: stored.active_provider,
            others: stored
                .other_providers
                .into_iter()
                .map(|(name, s)| Ok((name, s.open()?)))
                .collect::<Result<_, String>>()?,
        },
    ))
}

fn write_at(
    path: &Path,
    settings: &Settings,
    preferences: &Preferences,
    profiles: &Profiles,
) -> Result<(), String> {
    let dir = path.parent().ok_or("无效的设置文件位置。")?;
    fs::create_dir_all(dir).map_err(|_| "无法创建设置目录。")?;
    let stored = Stored {
        connection: Connection::protect(settings)?,
        active_provider: profiles.active.clone(),
        other_providers: profiles
            .others
            .iter()
            .map(|(name, s)| Ok((name.clone(), Connection::protect(s)?)))
            .collect::<Result<_, String>>()?,
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

pub fn load() -> Result<(Settings, Preferences, Profiles), String> {
    read_at(&config_file()?)
}

pub fn save(
    settings: &Settings,
    preferences: &Preferences,
    profiles: &Profiles,
) -> Result<(), String> {
    crate::engine::validate(settings)?;
    write_at(&config_file()?, settings, preferences, profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_under_current_windows_account_without_plaintext_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("子目录/settings.json");
        let (mut s, mut preferences, mut profiles) = read_at(&path).unwrap();
        preferences.appearance = Appearance::Dark;
        preferences.font_family = "Segoe UI".into();
        preferences.font_size = 22;
        s.reasoning = true;
        s.api_mode = ApiMode::Responses;
        s.api_key = "synthetic-key-123".to_owned();
        s.model = "local-model".into();
        profiles.active = "OpenAI".into();
        profiles.others.insert(
            "DeepSeek".into(),
            Settings {
                api_key: "other-secret".into(),
                ..s.clone()
            },
        );
        write_at(&path, &s, &preferences, &profiles).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("other-secret"));
        assert!(!fs::read_to_string(&path).unwrap().contains(&s.api_key));
        let (saved, actual, loaded) = read_at(&path).unwrap();
        assert_eq!(loaded.active, "OpenAI");
        assert!(loaded.others["DeepSeek"].api_key == "other-secret");
        assert_eq!(actual, preferences);
        assert_eq!(saved.api_mode, ApiMode::Responses);
        assert!(saved.reasoning);
        assert_eq!(
            (saved.model, saved.api_key),
            (s.model.clone(), s.api_key.clone())
        );
        s.api_key.clear();
        write_at(&path, &s, &preferences, &profiles).unwrap();
        assert!(read_at(&path).unwrap().0.api_key.is_empty());
    }

    #[test]
    fn old_preferences_keep_model_and_gain_new_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"endpoint":"https://example.org/v1/chat/completions","model":"my-model","protected_key":[],"preferences":{"appearance":"dark","target":"French"}}"#).unwrap();
        let (settings, prefs, _) = read_at(&path).unwrap();
        assert_eq!(settings.model, "my-model");
        assert_eq!(settings.api_mode, ApiMode::ChatCompletions);
        assert!(!settings.reasoning);
        assert_eq!(prefs.target, "French");
        assert_eq!(prefs.source, "English");
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
