use crate::config::{config_file, crypt};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

const LIMIT: usize = 100;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Entry {
    pub original: String,
    pub translated: String,
    pub source: String,
    pub target: String,
}

fn path() -> Result<PathBuf, String> {
    Ok(config_file()?.with_file_name("history.bin"))
}

fn read_at(path: &Path) -> Result<Vec<Entry>, String> {
    match fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&crypt(&bytes, false)?).map_err(|_| "无法读取历史记录。".into())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err("无法打开历史记录。".into()),
    }
}

fn record_at(path: &Path, entry: Entry) -> Result<(), String> {
    let mut entries = read_at(path)?;
    entries.retain(|e| {
        e.original != entry.original || e.target != entry.target || e.source != entry.source
    });
    entries.insert(0, entry);
    entries.truncate(LIMIT);
    let bytes = serde_json::to_vec(&entries).map_err(|_| "无法保存历史记录。")?;
    let protected = crypt(&bytes, true)?;
    let parent = path.parent().ok_or("无效的历史记录位置。")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建历史记录目录。")?;
    let mut tmp = NamedTempFile::new_in(parent).map_err(|_| "历史记录目录不可写。")?;
    tmp.write_all(&protected)
        .and_then(|_| tmp.as_file().sync_all())
        .map_err(|_| "无法写入历史记录。")?;
    tmp.persist(path).map_err(|_| "无法更新历史记录。")?;
    Ok(())
}

pub fn load() -> Result<Vec<Entry>, String> {
    read_at(&path()?)
}

pub fn record(entry: Entry) -> Result<(), String> {
    record_at(&path()?, entry)
}

pub fn clear() -> Result<(), String> {
    match fs::remove_file(path()?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法清除历史记录。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_private_and_retry_replaces_the_saved_result() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("history.bin");
        let mut e = Entry {
            original: "A private sentence.".into(),
            translated: "一句话。".into(),
            source: "English".into(),
            target: "Simplified Chinese".into(),
        };
        record_at(&path, e.clone()).unwrap();
        e.translated = "一个私密的句子。".into();
        record_at(&path, e.clone()).unwrap();
        assert!(!String::from_utf8_lossy(&fs::read(&path).unwrap()).contains(&e.original));
        assert!(read_at(&path).unwrap() == vec![e]);
    }
}
