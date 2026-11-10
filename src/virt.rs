//! Checks whether this PC can run a hardware-accelerated Android emulator.
use crate::runner::no_window;
use std::process::Command;

/// Returns (ok, message).
pub fn check() -> (bool, String) {
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        "(Get-CimInstance Win32_ComputerSystem).HypervisorPresent; \
         (Get-CimInstance Win32_Processor | Select-Object -First 1).VirtualizationFirmwareEnabled",
    ]);
    no_window(&mut cmd);

    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => return (false, format!("Could not check: {e}")),
    };
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.len() < 2 {
        return (false, "Could not read virtualization info".into());
    }
    let hypervisor = lines[lines.len() - 2].eq_ignore_ascii_case("true");
    let firmware = lines[lines.len() - 1].eq_ignore_ascii_case("true");

    if hypervisor {
        (true, "Ready (Windows hypervisor is active)".into())
    } else if firmware {
        (false, "BIOS virtualization is on, but enable \"Windows Hypervisor Platform\" in Windows Features, then restart".into())
    } else {
        (false, "Turn on virtualization (Intel VT-x / AMD SVM) in your BIOS".into())
    }
}
