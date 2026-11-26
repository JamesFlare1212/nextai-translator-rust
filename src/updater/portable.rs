use super::Update;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::fs::OpenOptionsExt,
    path::Path,
    process::Command,
};
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

pub(super) fn restart(payload: &Path, update: &Update) -> Result<(), String> {
    let mut staged = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(payload)
        .map_err(|e| format!("无法打开更新包：{e}"))?;
    let mut hash = Sha256::new();
    if staged.metadata().map_err(|e| e.to_string())?.len() != update.size
        || io::copy(&mut staged, &mut hash).map_err(|e| e.to_string())? != update.size
        || format!("{:x}", hash.finalize()) != update.hash
    {
        return Err("更新包校验失败，未安装".into());
    }

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = exe.parent().ok_or("无法定位程序目录")?;
    let mut backup = tempfile::Builder::new()
        .prefix("nextai-previous-")
        .suffix(".exe")
        .tempfile_in(parent)
        .map_err(|e| format!("程序目录不可写：{e}"))?;
    File::open(&exe)
        .and_then(|mut old| io::copy(&mut old, &mut backup))
        .and_then(|_| backup.as_file().sync_all())
        .map_err(|e| format!("无法备份当前版本：{e}"))?;
    let backup = backup.into_temp_path();
    let error = match self_replace::self_replace(payload) {
        Err(e) if exe.is_file() => return Err(format!("无法替换程序：{e}")),
        Err(e) => e,
        Ok(()) => match Command::new(&exe).current_dir(parent).spawn() {
            Ok(_) => return Ok(()),
            Err(e) => e,
        },
    };
    if let Err(failed) = backup.persist(&exe) {
        let backup = failed.path.keep().map_err(|e| e.to_string())?;
        return Err(format!(
            "更新失败（{error}），请从备份 {} 恢复程序",
            backup.display()
        ));
    }
    Err(format!("更新失败，已恢复原版本：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::{Kind, restart};
    use semver::Version;
    use std::fs;

    #[test]
    fn updates_only_a_disposable_copy_and_recovers_on_failure() {
        const NAME: &str = "disposable translator.exe";
        const CASE: &str = "NEXTAI_PORTABLE_TEST_CASE";
        let exe = std::env::current_exe().unwrap();
        let replacement = Path::new(&std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("hostname.exe");
        if exe.file_name().unwrap() == NAME {
            let case = std::env::var(CASE).unwrap();
            let stage = tempfile::tempdir_in(exe.parent().unwrap()).unwrap();
            let staged_path = stage.path().to_owned();
            let bytes = match case.as_str() {
                "success" | "checksum" => fs::read(replacement).unwrap(),
                "invalid-exe" => b"not an executable".to_vec(),
                _ => panic!("unknown test case"),
            };
            fs::write(stage.path().join("update.exe"), &bytes).unwrap();
            let mut update = Update {
                version: Version::new(1, 0, 0),
                size: bytes.len() as u64,
                hash: format!("{:x}", Sha256::digest(&bytes)),
                url: String::new(),
            };
            if case == "checksum" {
                update.hash = "0".repeat(64);
            }
            let result = restart(stage, Kind::Portable, &update);
            match case.as_str() {
                "success" => result.unwrap(),
                "checksum" => assert!(result.unwrap_err().contains("校验失败")),
                _ => assert!(result.unwrap_err().contains("已恢复原版本")),
            }
            assert!(!staged_path.exists());
            return;
        }
        for case in ["success", "invalid-exe", "checksum"] {
            let dir = tempfile::tempdir().unwrap();
            let copy = dir.path().join(NAME);
            fs::copy(&exe, &copy).unwrap();
            let output = Command::new(&copy)
                .args(["--exact", "updater::portable::tests::updates_only_a_disposable_copy_and_recovers_on_failure", "--nocapture"])
                .env(CASE, case)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            let expected = if case == "success" {
                &replacement
            } else {
                &exe
            };
            let digest = |path: &Path| {
                let mut hash = Sha256::new();
                io::copy(&mut File::open(path).unwrap(), &mut hash).unwrap();
                hash.finalize()
            };
            assert_eq!(digest(&copy), digest(expected), "{case}");
        }
    }
}
