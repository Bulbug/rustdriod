//! The fixed "recommended" list of system images, plus install/remove.
use crate::runner::Log;
use crate::{paths, sdk, Res};
use std::path::PathBuf;

#[derive(Clone)]
pub struct SystemImage {
    pub package_id: String,       // e.g. system-images;android-34;google_apis;x86_64
    pub android: &'static str,    // "14"
    pub api: u32,                 // 34
    pub variant: &'static str,    // "Google APIs"
}

impl SystemImage {
    pub fn android_label(&self) -> String {
        format!("Android {} (API {})", self.android, self.api)
    }
    pub fn display_name(&self) -> String {
        format!("{} - {}", self.android_label(), self.variant)
    }
    /// Where sdkmanager puts this image on disk.
    pub fn install_dir(&self) -> PathBuf {
        let mut p = paths::sdk_root();
        for part in self.package_id.split(';') {
            p = p.join(part);
        }
        p
    }
    pub fn is_installed(&self) -> bool {
        self.install_dir().join("source.properties").exists()
    }
}

pub fn catalog() -> Vec<SystemImage> {
    // (Android version, API level, include the AOSP image?)
    let versions = [("15", 35, false), ("14", 34, true), ("13", 33, true), ("11", 30, true)];
    let mut list = Vec::new();
    for (android, api, aosp) in versions {
        let mut variants = vec![("google_apis", "Google APIs"), ("google_apis_playstore", "Google Play")];
        if aosp {
            // AOSP = no Google services. Smallest and lightest.
            variants.push(("default", "AOSP (lightest)"));
        }
        for (id, label) in variants {
            list.push(SystemImage {
                package_id: format!("system-images;android-{api};{id};x86_64"),
                android,
                api,
                variant: label,
            });
        }
    }
    list
}

pub fn install(img: &SystemImage, log: &Log) -> Res<()> {
    log(format!("Installing {} ...", img.display_name()));
    sdk::run_sdkmanager(&[&img.package_id], log)?;
    log("Installed.".to_string());
    Ok(())
}

pub fn uninstall(img: &SystemImage, log: &Log) -> Res<()> {
    log(format!("Removing {} ...", img.display_name()));
    sdk::run_sdkmanager(&["--uninstall", &img.package_id], log)?;
    log("Removed.".to_string());
    Ok(())
}
