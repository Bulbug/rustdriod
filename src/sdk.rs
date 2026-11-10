//! Downloads Google's command-line tools and installs platform-tools + the emulator.
use crate::runner::{self, Log};
use crate::{download, java, paths, Res};
use std::fs;

// Update this URL when Google releases a newer build
// (see developer.android.com/studio, "Command line tools only").
const CMDLINE_TOOLS_URL: &str =
    "https://dl.google.com/android/repository/commandlinetools-win-11076708_latest.zip";

pub fn has_cmdline_tools() -> bool { paths::sdkmanager().exists() }
pub fn has_platform_tools() -> bool { paths::adb().exists() }
pub fn has_emulator() -> bool { paths::emulator().exists() }

pub fn download_cmdline_tools(progress: &dyn Fn(f32), log: &Log) -> Res<()> {
    let zip = paths::home().join("cmdline-tools.zip");
    log("Downloading Android command-line tools...".to_string());
    download::fetch_to_file(CMDLINE_TOOLS_URL, &zip, progress)?;
    log("Extracting...".to_string());
    let target = paths::cmdline_tools();
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    download::extract_zip(&zip, &target, 1)?;
    fs::remove_file(&zip)?;
    log("Command-line tools ready.".to_string());
    Ok(())
}

/// Run sdkmanager with the given arguments. Licenses are auto-answered "y"
/// because the user already accepted them in the app.
pub fn run_sdkmanager(args: &[&str], log: &Log) -> Res<()> {
    let env = java::tool_env()?;
    let mut all = vec![format!("--sdk_root={}", paths::sdk_root().display())];
    all.extend(args.iter().map(|s| s.to_string()));
    let code = runner::run(paths::sdkmanager(), &all, &env, Some(&"y\n".repeat(30)), log)?;
    if code != 0 {
        return Err(format!("sdkmanager exited with code {code}").into());
    }
    Ok(())
}

pub fn install_packages(log: &Log) -> Res<()> {
    log("Installing platform-tools and emulator (can take a few minutes)...".to_string());
    run_sdkmanager(&["platform-tools", "emulator"], log)?;
    log("Install finished.".to_string());
    Ok(())
}
