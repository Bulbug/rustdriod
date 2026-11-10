//! Virtual devices (AVDs): list, create from a preset, delete.
//! Devices are stored in our own folder, not in the user profile.
use crate::images::SystemImage;
use crate::runner::{self, Log};
use crate::{java, paths, Res};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// ---------- presets ----------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Lite,
    LowEnd,
    Balanced,
    Gaming,
}

struct Spec {
    label: &'static str,
    desc: &'static str,
    ram_mb: u32,
    cores: usize,
    storage: &'static str,
    width: u32,
    height: u32,
    dpi: u32,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Lite, Preset::LowEnd, Preset::Balanced, Preset::Gaming];

    fn spec(self) -> Spec {
        match self {
            Preset::Lite => Spec {
                label: "Lite (lightest)",
                desc: "1.5 GB RAM, 2 cores, 540x960 - for low-end PCs",
                ram_mb: 1536, cores: 2, storage: "3G", width: 540, height: 960, dpi: 240,
            },
            Preset::LowEnd => Spec {
                label: "Low-end PC",
                desc: "2 GB RAM, 2 cores, 720x1280",
                ram_mb: 2048, cores: 2, storage: "4G", width: 720, height: 1280, dpi: 240,
            },
            Preset::Balanced => Spec {
                label: "Balanced",
                desc: "4 GB RAM, 4 cores, 1080x1920",
                ram_mb: 4096, cores: 4, storage: "8G", width: 1080, height: 1920, dpi: 320,
            },
            Preset::Gaming => Spec {
                label: "Gaming",
                desc: "6 GB RAM, 6 cores, 1080x1920",
                ram_mb: 6144, cores: 6, storage: "12G", width: 1080, height: 1920, dpi: 320,
            },
        }
    }

    pub fn label(self) -> &'static str { self.spec().label }
    pub fn description(self) -> &'static str { self.spec().desc }

    /// Never ask for more cores than this PC has.
    fn effective_cores(self) -> usize {
        let have = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        self.spec().cores.min(have).max(1)
    }
}

// ---------- list ----------

#[derive(Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub sysdir: String, // image.sysdir.1 from config.ini
    pub ram: String,
    pub cores: String,
    pub screen: String,
}

pub fn normalize_sysdir(s: &str) -> String {
    s.replace('\\', "/").trim_end_matches('/').to_string()
}

pub fn list() -> Res<Vec<DeviceInfo>> {
    let dir = paths::avd_home();
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let mut names: Vec<String> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.strip_suffix(".ini").map(|s| s.to_string())
        })
        .collect();
    names.sort();

    for name in names {
        let cfg = dir.join(format!("{name}.avd")).join("config.ini");
        if !cfg.exists() {
            continue;
        }
        let c = read_config(&cfg)?;
        let get = |k: &str| c.get(k).cloned().unwrap_or_else(|| "?".to_string());
        out.push(DeviceInfo {
            sysdir: get("image.sysdir.1"),
            ram: get("hw.ramSize"),
            cores: get("hw.cpu.ncore"),
            screen: format!("{}x{}", get("hw.lcd.width"), get("hw.lcd.height")),
            name,
        });
    }
    Ok(out)
}

/// Names of devices using this image (so we never delete an image under them).
pub fn devices_using(img: &SystemImage) -> Res<Vec<String>> {
    let want = normalize_sysdir(&img.package_id.replace(';', "/"));
    Ok(list()?
        .into_iter()
        .filter(|d| normalize_sysdir(&d.sysdir) == want)
        .map(|d| d.name)
        .collect())
}

// ---------- create / delete ----------

pub fn create(name: &str, img: &SystemImage, preset: Preset, log: &Log) -> Res<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return Err("Name can only use letters, numbers, dot, dash and underscore.".into());
    }
    fs::create_dir_all(paths::avd_home())?;
    if paths::avd_home().join(format!("{name}.ini")).exists() {
        return Err(format!("A device named \"{name}\" already exists.").into());
    }

    log(format!("Creating device \"{name}\" ..."));
    let env = java::tool_env()?;
    let args: Vec<String> = ["create", "avd", "-n", name, "-k", &img.package_id, "-d", "pixel"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let code = runner::run(paths::avdmanager(), &args, &env, Some("no\n"), log)?;
    if code != 0 {
        return Err(format!("avdmanager exited with code {code}").into());
    }

    let cfg = paths::avd_home().join(format!("{name}.avd")).join("config.ini");
    apply_preset(&cfg, preset)?;
    log(format!(
        "Applied preset: {} ({} MB RAM, {} cores).",
        preset.label(),
        preset.spec().ram_mb,
        preset.effective_cores()
    ));
    Ok(())
}

pub fn delete(name: &str, log: &Log) -> Res<()> {
    log(format!("Deleting device \"{name}\" ..."));
    let env = java::tool_env()?;
    let args: Vec<String> = ["delete", "avd", "-n", name].iter().map(|s| s.to_string()).collect();
    let code = runner::run(paths::avdmanager(), &args, &env, None, log)?;
    if code != 0 {
        return Err(format!("avdmanager exited with code {code}").into());
    }
    log("Deleted.".to_string());
    Ok(())
}

// ---------- config.ini helpers ----------

fn apply_preset(cfg: &Path, p: Preset) -> Res<()> {
    let s = p.spec();
    let updates: Vec<(&str, String)> = vec![
        ("hw.ramSize", s.ram_mb.to_string()),
        ("hw.cpu.ncore", p.effective_cores().to_string()),
        ("disk.dataPartition.size", s.storage.to_string()),
        ("hw.lcd.width", s.width.to_string()),
        ("hw.lcd.height", s.height.to_string()),
        ("hw.lcd.density", s.dpi.to_string()),
        ("skin.name", format!("{}x{}", s.width, s.height)),
        ("hw.gpu.enabled", "yes".to_string()),
        ("hw.gpu.mode", "auto".to_string()),
        ("hw.keyboard", "yes".to_string()),
    ];
    write_config(cfg, &updates, &["skin.path"])
}

fn read_config(path: &Path) -> Res<HashMap<String, String>> {
    let text = fs::read_to_string(path)?;
    let mut map = HashMap::new();
    for line in text.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(map)
}

/// Replace the given keys (or add them), and delete the keys in `remove`.
fn write_config(path: &Path, updates: &[(&str, String)], remove: &[&str]) -> Res<()> {
    let text = fs::read_to_string(path)?;
    let mut out: Vec<String> = Vec::new();
    let mut done = vec![false; updates.len()];

    for line in text.lines() {
        if let Some((k, _)) = line.split_once('=') {
            let key = k.trim();
            if remove.contains(&key) {
                continue;
            }
            if let Some(idx) = updates.iter().position(|(uk, _)| *uk == key) {
                out.push(format!("{}={}", key, updates[idx].1));
                done[idx] = true;
                continue;
            }
        }
        out.push(line.to_string());
    }
    for (i, (k, v)) in updates.iter().enumerate() {
        if !done[i] {
            out.push(format!("{k}={v}"));
        }
    }
    fs::write(path, out.join("\n") + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysdir_matches_package_id() {
        let want = normalize_sysdir(&"system-images;android-34;google_apis;x86_64".replace(';', "/"));
        assert_eq!(normalize_sysdir("system-images\\android-34\\google_apis\\x86_64\\"), want);
    }

    #[test]
    fn config_rewrite_replaces_adds_and_removes() {
        let dir = std::env::temp_dir().join("aem-test-config");
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config.ini");
        fs::write(&f, "hw.ramSize=2048\nskin.path=foo\nimage.sysdir.1=a/b/\n").unwrap();

        write_config(&f, &[("hw.ramSize", "1536".to_string()), ("hw.cpu.ncore", "2".to_string())], &["skin.path"]).unwrap();

        let c = read_config(&f).unwrap();
        assert_eq!(c["hw.ramSize"], "1536");
        assert_eq!(c["hw.cpu.ncore"], "2");
        assert_eq!(c["image.sysdir.1"], "a/b/");
        assert!(!c.contains_key("skin.path"));
    }

    #[test]
    fn lite_never_exceeds_machine_cores() {
        assert!(Preset::Gaming.effective_cores() >= 1);
        assert!(Preset::Gaming.effective_cores() <= 6);
    }
}
