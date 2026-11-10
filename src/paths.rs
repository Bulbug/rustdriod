//! Every folder the app uses. All of it lives under %LOCALAPPDATA%\AndroidEmulatorManager.
use std::path::PathBuf;

pub fn home() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("AndroidEmulatorManager")
}

pub fn sdk_root() -> PathBuf { home().join("sdk") }
pub fn cmdline_tools() -> PathBuf { sdk_root().join("cmdline-tools").join("latest") }
pub fn sdkmanager() -> PathBuf { cmdline_tools().join("bin").join("sdkmanager.bat") }
pub fn avdmanager() -> PathBuf { cmdline_tools().join("bin").join("avdmanager.bat") }
pub fn adb() -> PathBuf { sdk_root().join("platform-tools").join("adb.exe") }
pub fn emulator() -> PathBuf { sdk_root().join("emulator").join("emulator.exe") }
pub fn avd_home() -> PathBuf { home().join("avd") }
pub fn jre_dir() -> PathBuf { home().join("jre") }
