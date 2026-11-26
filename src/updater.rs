mod portable;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{future::Future, io::Write, time::Duration};
use tempfile::TempDir;

const REPO: &str = "JamesFlare1212/nextai-translator-rust";
const LIMIT: u64 = 256 << 20;
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy)]
pub enum Kind {
    Portable,
    Setup,
}
impl Kind {
    pub fn current() -> Self {
        if std::env::current_exe().is_ok_and(|p| p.with_file_name("nextai-setup.marker").is_file())
        {
            Self::Setup
        } else {
            Self::Portable
        }
    }
    fn slug(self) -> &'static str {
        match self {
            Self::Portable => "portable",
            Self::Setup => "setup",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Portable => "便携版",
            Self::Setup => "安装版",
        }
    }
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    digest: Option<String>,
}
#[derive(Clone)]
pub struct Update {
    pub version: Version,
    pub size: u64,
    url: String,
    hash: String,
}

fn select(r: Release, kind: Kind, current: &Version) -> Result<Option<Update>, String> {
    if r.draft || r.prerelease {
        return Ok(None);
    }
    let v = r
        .tag_name
        .strip_prefix('v')
        .and_then(|s| Version::parse(s).ok())
        .ok_or("发布版本号无效")?;
    if !v.pre.is_empty() || &v <= current {
        return Ok(None);
    }
    let name = format!("nextai-translator-{v}-windows-x64-{}.exe", kind.slug());
    let a = r
        .assets
        .into_iter()
        .find(|a| a.name == name)
        .ok_or("发布包尚不完整，请稍后重试")?;
    let hash = a
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("发布包缺少 SHA-256 校验值")?;
    if a.size == 0 || a.size > LIMIT {
        return Err("发布包大小异常".into());
    }
    Ok(Some(Update {
        url: format!("https://github.com/{REPO}/releases/download/v{v}/{name}"),
        version: v,
        size: a.size,
        hash: hash.to_ascii_lowercase(),
    }))
}
pub fn run<T>(job: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(job)
}
fn client(seconds: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(true)
        .user_agent("NextAI-Translator-Updater")
        .timeout(Duration::from_secs(seconds))
        .build()
        .map_err(|e| e.to_string())
}
pub async fn check(kind: Kind) -> Result<Option<Update>, String> {
    let res = client(15)?
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .send()
        .await
        .map_err(|_| "无法连接 GitHub，请检查网络")?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let r = res
        .error_for_status()
        .map_err(|e| format!("GitHub 请求失败 ({})", e.status().unwrap()))?
        .json()
        .await
        .map_err(|_| "无法读取发布信息")?;
    select(
        r,
        kind,
        &Version::parse(CURRENT).map_err(|_| "当前版本号无效")?,
    )
}
pub async fn download(u: &Update) -> Result<TempDir, String> {
    download_with(client(300)?, u).await
}

async fn download_with(client: reqwest::Client, u: &Update) -> Result<TempDir, String> {
    let dir = tempfile::tempdir().map_err(|_| "无法创建临时目录")?;
    let mut file =
        std::fs::File::create(dir.path().join("update.exe")).map_err(|_| "无法创建临时文件")?;
    let mut res = client
        .get(&u.url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| "下载失败，请检查网络")?;
    let mut count = 0;
    let mut hash = Sha256::new();
    while let Some(chunk) = res.chunk().await.map_err(|_| "下载中断")? {
        count += chunk.len() as u64;
        if count > u.size {
            return Err("下载大小异常".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk)
            .map_err(|_| "写入失败，请检查磁盘空间")?;
    }
    if count != u.size || format!("{:x}", hash.finalize()) != u.hash {
        return Err("下载校验失败，未安装".into());
    }
    file.sync_all().map_err(|_| "写入失败")?;
    Ok(dir)
}
#[cfg(windows)]
pub fn restart(dir: TempDir, kind: Kind, u: &Update) -> Result<(), String> {
    use std::{os::windows::process::CommandExt, process::Command};
    if matches!(kind, Kind::Portable) {
        return portable::restart(&dir.path().join("update.exe"), u);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let shell = std::path::Path::new(&std::env::var_os("SystemRoot").ok_or("无法定位 Windows")?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script = dir.path().join("apply.ps1");
    std::fs::write(&script, include_bytes!("../packaging/apply.ps1")).map_err(|e| e.to_string())?;
    Command::new(shell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .env("NEXTAI_PARENT", std::process::id().to_string())
        .env("NEXTAI_TARGET", exe)
        .env("NEXTAI_STAGE", dir.path())
        .env("NEXTAI_HASH", &u.hash)
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    let _ = dir.keep();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(kind: &str, hash: &str) -> Release {
        serde_json::from_value(serde_json::json!({"tag_name":"v1.10.0","draft":false,"prerelease":false,"assets":[{"name":format!("nextai-translator-1.10.0-windows-x64-{kind}.exe"),"size":100,"digest":hash}]})).unwrap()
    }
    #[test]
    fn verify_download_before_staging() {
        let mut server = mockito::Server::new();
        let asset = server
            .mock("GET", "/update.exe")
            .with_body("abc")
            .expect(4)
            .create();
        for (size, digest, valid) in [
            (3, Sha256::digest(b"abc"), true),
            (3, Sha256::digest(b"bad"), false),
            (2, Sha256::digest(b"abc"), false),
            (4, Sha256::digest(b"abc"), false),
        ] {
            let update = Update {
                version: Version::new(1, 0, 0),
                size,
                url: format!("{}/update.exe", server.url()),
                hash: format!("{digest:x}"),
            };
            let result = run(download_with(
                reqwest::Client::builder().no_proxy().build().unwrap(),
                &update,
            ));
            assert_eq!(result.is_ok(), valid);
            if let Ok(dir) = result {
                assert_eq!(
                    std::fs::read(dir.path().join("update.exe")).unwrap(),
                    b"abc"
                );
            }
        }
        asset.assert();
    }
    #[test]
    fn release_selection() {
        let v = Version::parse("1.9.0").unwrap();
        let hash = format!("sha256:{}", "a".repeat(64));
        for k in [Kind::Setup, Kind::Portable] {
            assert!(
                select(release(k.slug(), &hash), k, &v)
                    .unwrap()
                    .unwrap()
                    .url
                    .ends_with(&format!("{}.exe", k.slug()))
            )
        }
        assert!(select(release("portable", &hash), Kind::Setup, &v).is_err());
        assert!(select(release("setup", "bad"), Kind::Setup, &v).is_err());
        assert!(
            select(
                release("setup", &hash),
                Kind::Setup,
                &Version::parse("2.0.0").unwrap()
            )
            .unwrap()
            .is_none()
        );
        let mut r = release("setup", &hash);
        r.prerelease = true;
        assert!(select(r, Kind::Setup, &v).unwrap().is_none());
    }
}
