//! Google's sdkmanager/avdmanager are Java programs, so we need Java 17+.
//! (The emulator itself is native and does NOT need Java.)
//! If the PC has no suitable Java, we download a small runtime (Eclipse Temurin 17).
use crate::runner::{no_window, Log};
use crate::{download, paths, Res};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const JRE_URL: &str =
    "https://api.adoptium.net/v3/binary/latest/17/ga/windows/x64/jre/hotspot/normal/eclipse";

fn java_exe(home: &Path) -> PathBuf {
    home.join("bin").join("java.exe")
}

/// Ask `java -version` for the major version (8, 11, 17, 21 ...).
fn major_version(exe: &Path) -> Option<u32> {
    let mut cmd = Command::new(exe);
    cmd.arg("-version");
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    parse_major(&String::from_utf8_lossy(&out.stderr))
}

/// Reads the version out of text like: openjdk version "17.0.9" 2023-10-17
fn parse_major(text: &str) -> Option<u32> {
    let start = text.find('"')? + 1;
    let rest = &text[start..];
    let end = rest.find('"')?;
    let mut parts = rest[..end]
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty());
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok() // old style "1.8.0" means Java 8
    } else {
        Some(first)
    }
}

/// Find a usable Java 17+ home folder: ours first, then JAVA_HOME, then PATH.
pub fn java_home() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![paths::jre_dir()];
    if let Some(h) = std::env::var_os("JAVA_HOME") {
        candidates.push(PathBuf::from(h));
    }
    if let Some(p) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&p) {
            if let Some(parent) = dir.parent() {
                candidates.push(parent.to_path_buf());
            }
        }
    }
    candidates.into_iter().find(|home| {
        let exe = java_exe(home);
        exe.exists() && major_version(&exe).map_or(false, |v| v >= 17)
    })
}

/// Environment variables the SDK tools need.
pub fn tool_env() -> Res<Vec<(String, String)>> {
    let home = java_home().ok_or("Java 17+ was not found. Run Setup first.")?;
    Ok(vec![
        ("JAVA_HOME".into(), home.to_string_lossy().to_string()),
        ("ANDROID_SDK_ROOT".into(), paths::sdk_root().to_string_lossy().to_string()),
        ("ANDROID_AVD_HOME".into(), paths::avd_home().to_string_lossy().to_string()),
    ])
}

pub fn download_jre(progress: &dyn Fn(f32), log: &Log) -> Res<()> {
    let zip = paths::home().join("jre.zip");
    log("Downloading a small Java runtime (Eclipse Temurin 17)...".to_string());
    download::fetch_to_file(JRE_URL, &zip, progress)?;
    log("Extracting Java...".to_string());
    let target = paths::jre_dir();
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    download::extract_zip(&zip, &target, 1)?;
    fs::remove_file(&zip)?;
    log("Java ready.".to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_major;

    #[test]
    fn reads_java_versions() {
        assert_eq!(parse_major("openjdk version \"17.0.9\" 2023-10-17"), Some(17));
        assert_eq!(parse_major("java version \"1.8.0_352\""), Some(8));
        assert_eq!(parse_major("openjdk version \"21\" 2023-09-19"), Some(21));
        assert_eq!(parse_major("garbage"), None);
    }
}
