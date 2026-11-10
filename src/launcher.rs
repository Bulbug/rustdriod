//! Starting and stopping the emulator. Lean flags are ON by default.
//! (The emulator is a native program: it does not need Java.)
use crate::runner::{self, no_window, Log};
use crate::{paths, Res};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gpu {
    Auto,
    Host,
    Angle,
    Software,
}

impl Gpu {
    pub const ALL: [Gpu; 4] = [Gpu::Auto, Gpu::Host, Gpu::Angle, Gpu::Software];

    pub fn label(self) -> &'static str {
        match self {
            Gpu::Auto => "Auto (recommended)",
            Gpu::Host => "Host GPU (fastest)",
            Gpu::Angle => "ANGLE / DirectX (fixes some graphics bugs)",
            Gpu::Software => "Software (slowest, most compatible)",
        }
    }

    fn flag(self) -> &'static str {
        match self {
            Gpu::Auto => "auto",
            Gpu::Host => "host",
            Gpu::Angle => "angle_indirect",
            Gpu::Software => "swiftshader_indirect",
        }
    }
}

#[derive(Clone)]
pub struct LaunchOptions {
    pub no_audio: bool,     // -no-audio
    pub no_boot_anim: bool, // -no-boot-anim
    pub quick_boot: bool,   // off = -no-snapshot-load (cold boot)
    pub no_save: bool,      // -no-snapshot-save (saves disk space)
    pub gpu: Gpu,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions { no_audio: true, no_boot_anim: true, quick_boot: true, no_save: false, gpu: Gpu::Auto }
    }
}

pub fn build_args(name: &str, o: &LaunchOptions) -> Vec<String> {
    let mut a: Vec<String> = vec!["-avd".into(), name.into(), "-gpu".into(), o.gpu.flag().into()];
    if o.no_audio {
        a.push("-no-audio".into());
    }
    if o.no_boot_anim {
        a.push("-no-boot-anim".into());
    }
    if !o.quick_boot {
        a.push("-no-snapshot-load".into());
    }
    if o.no_save {
        a.push("-no-snapshot-save".into());
    }
    a
}

/// Starts the emulator and BLOCKS until it exits (call it from a background thread).
/// `on_spawn` receives the process id as soon as it starts.
pub fn start(name: &str, opts: &LaunchOptions, log: &Log, on_spawn: &dyn Fn(u32)) -> Res<i32> {
    if !paths::emulator().exists() {
        return Err("The Android Emulator is not installed. Run Setup first.".into());
    }
    let args = build_args(name, opts);
    log(format!("Starting \"{name}\": emulator {}", args.join(" ")));
    let env = vec![
        ("ANDROID_SDK_ROOT".to_string(), paths::sdk_root().to_string_lossy().to_string()),
        ("ANDROID_AVD_HOME".to_string(), paths::avd_home().to_string_lossy().to_string()),
    ];
    Ok(runner::run_notify(paths::emulator(), &args, &env, None, log, on_spawn)?)
}

/// Polite stop: asks the emulator to shut down itself (it can save its state).
pub fn stop_graceful(log: &Log) -> Res<()> {
    let args: Vec<String> = ["emu", "kill"].iter().map(|s| s.to_string()).collect();
    runner::run(paths::adb(), &args, &[], None, log)?;
    Ok(())
}

/// Last resort: kill the emulator and everything it started (emulator.exe starts qemu).
pub fn kill_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    let mut cmd = Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
    no_window(&mut cmd);
    let _ = cmd.output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lean_defaults() {
        let a = build_args("Test", &LaunchOptions::default());
        assert_eq!(a, ["-avd", "Test", "-gpu", "auto", "-no-audio", "-no-boot-anim"]);
    }

    #[test]
    fn cold_boot_and_no_save_flags() {
        let o = LaunchOptions { quick_boot: false, no_save: true, no_audio: false, no_boot_anim: false, gpu: Gpu::Host };
        let a = build_args("X", &o);
        assert_eq!(a, ["-avd", "X", "-gpu", "host", "-no-snapshot-load", "-no-snapshot-save"]);
    }
}
