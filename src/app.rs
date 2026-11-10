//! The window. Three tabs (Setup, System Images, Devices) and one shared log panel.
//! Heavy work runs on background threads and reports back through a channel.
use crate::devices::{self, DeviceInfo, Preset};
use crate::images::{self, SystemImage};
use crate::launcher::{self, Gpu, LaunchOptions};
use crate::{java, sdk, virt, Res};
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

const GREEN: egui::Color32 = egui::Color32::from_rgb(26, 127, 55);
const RED: egui::Color32 = egui::Color32::from_rgb(198, 40, 40);

const SETUP_TEXT: &str = "Setting up the Android SDK means you accept Google's Android SDK License Agreement:\n\
https://developer.android.com/studio/terms\n\n\
If Java 17+ is missing, a small Java runtime (Eclipse Temurin 17, ~50 MB) is downloaded too.\n\n\
Continue?";

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Setup,
    Images,
    Devices,
}

/// What the user confirmed in a dialog.
enum Action {
    Setup,
    InstallImage(usize),
    DeleteImage(usize),
    DeleteDevice(String),
    StopAndQuit,
}

struct EnvStatus {
    virt: (bool, String),
    java: bool,
}

struct Snapshot {
    env: Option<EnvStatus>, // only filled on a "full" check (it is the slow part)
    tools: bool,
    adb: bool,
    emulator: bool,
    installed: Vec<bool>,
    devices: Vec<DeviceInfo>,
}

/// Messages from background threads to the window.
enum Msg {
    Log(String),
    Progress(Option<f32>),
    Refreshed(Snapshot),
    Finished { full: bool },
    EmuExited { code: i32 },
}

/// The emulator that is currently running (only one at a time).
struct Running {
    name: String,
    pid: Arc<AtomicU32>,
    alive: Arc<AtomicBool>,
    stopping: bool,
}

/// Handle given to background threads so they can talk to the window.
#[derive(Clone)]
struct Reporter {
    tx: mpsc::Sender<Msg>,
    ctx: egui::Context,
}

impl Reporter {
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.ctx.request_repaint(); // wake the UI (it sleeps when idle)
    }
    fn logger(&self) -> crate::runner::Log {
        let r = self.clone();
        Arc::new(move |s| r.send(Msg::Log(s)))
    }
    fn progress(&self, p: Option<f32>) {
        self.send(Msg::Progress(p));
    }
}

pub struct App {
    rep: Reporter,
    rx: mpsc::Receiver<Msg>,

    tab: Tab,
    busy: bool,
    progress: Option<f32>,
    log: Vec<String>,

    virt: (bool, String),
    java_ok: bool,
    tools_ok: bool,
    adb_ok: bool,
    emu_ok: bool,

    images: Vec<SystemImage>,
    installed: Vec<bool>,
    sel_image: Option<usize>,

    devices: Vec<DeviceInfo>,
    sel_device: Option<usize>,
    new_name: String,
    new_image: Option<usize>,
    new_preset: Preset,

    running: Option<Running>,
    opts: LaunchOptions,
    quit_when_stopped: bool,
    quitting: bool,

    confirm: Option<(String, Action)>,
    info: Option<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel();
        let images = images::catalog();
        let app = App {
            rep: Reporter { tx, ctx: cc.egui_ctx.clone() },
            rx,
            tab: Tab::Setup,
            busy: false,
            progress: None,
            log: Vec::new(),
            virt: (false, "Checking...".to_string()),
            java_ok: false,
            tools_ok: false,
            adb_ok: false,
            emu_ok: false,
            installed: vec![false; images.len()],
            images,
            sel_image: None,
            devices: Vec::new(),
            sel_device: None,
            new_name: String::new(),
            new_image: None,
            new_preset: Preset::Lite,
            running: None,
            opts: LaunchOptions::default(),
            quit_when_stopped: false,
            quitting: false,
            confirm: None,
            info: None,
        };
        app.refresh(true);
        app
    }

    // ---------- background work ----------

    /// Re-read what is installed. `full` also re-checks virtualization and Java (slower).
    fn refresh(&self, full: bool) {
        let rep = self.rep.clone();
        let imgs = self.images.clone();
        thread::spawn(move || {
            let env = if full {
                Some(EnvStatus { virt: virt::check(), java: java::java_home().is_some() })
            } else {
                None
            };
            let snap = Snapshot {
                env,
                tools: sdk::has_cmdline_tools(),
                adb: sdk::has_platform_tools(),
                emulator: sdk::has_emulator(),
                installed: imgs.iter().map(|i| i.is_installed()).collect(),
                devices: devices::list().unwrap_or_default(),
            };
            rep.send(Msg::Refreshed(snap));
        });
    }

    fn spawn_job<F>(&mut self, full: bool, job: F)
    where
        F: FnOnce(&Reporter) -> Res<()> + Send + 'static,
    {
        self.busy = true;
        self.progress = None;
        let rep = self.rep.clone();
        thread::spawn(move || {
            if let Err(e) = job(&rep) {
                rep.send(Msg::Log(format!("ERROR: {e}")));
            }
            rep.send(Msg::Finished { full });
        });
    }

    fn run_action(&mut self, action: Action) {
        match action {
            Action::Setup => self.spawn_job(true, |rep| {
                let log = rep.logger();
                if java::java_home().is_none() {
                    java::download_jre(&|p| rep.progress(Some(p)), &log)?;
                }
                if !sdk::has_cmdline_tools() {
                    sdk::download_cmdline_tools(&|p| rep.progress(Some(p)), &log)?;
                }
                rep.progress(None);
                sdk::install_packages(&log)?;
                log("Done!".to_string());
                Ok(())
            }),
            Action::InstallImage(i) => {
                let img = self.images[i].clone();
                self.spawn_job(false, move |rep| images::install(&img, &rep.logger()));
            }
            Action::DeleteImage(i) => {
                let img = self.images[i].clone();
                self.spawn_job(false, move |rep| images::uninstall(&img, &rep.logger()));
            }
            Action::DeleteDevice(name) => {
                self.spawn_job(false, move |rep| devices::delete(&name, &rep.logger()));
            }
            Action::StopAndQuit => {
                self.quit_when_stopped = true;
                self.on_stop_device();
            }
        }
    }

    // ---------- messages from threads ----------

    fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Log(s) => self.push_log(s),
                Msg::Progress(p) => self.progress = p,
                Msg::Refreshed(s) => self.apply(s),
                Msg::Finished { full } => {
                    self.busy = false;
                    self.progress = None;
                    self.refresh(full);
                }
                Msg::EmuExited { code } => {
                    self.running = None;
                    self.push_log(format!("Emulator closed (exit code {code})."));
                    if self.quit_when_stopped {
                        self.quitting = true;
                        self.rep.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
    }

    fn push_log(&mut self, s: String) {
        // sdkmanager prints a progress bar many times; keep only the latest one.
        let is_bar = |t: &str| t.starts_with('[') && t.contains('%');
        if is_bar(&s) {
            if let Some(last) = self.log.last_mut() {
                if is_bar(last) {
                    *last = s;
                    return;
                }
            }
        }
        self.log.push(s);
        if self.log.len() > 1000 {
            self.log.drain(..200);
        }
    }

    fn apply(&mut self, s: Snapshot) {
        if let Some(env) = s.env {
            self.virt = env.virt;
            self.java_ok = env.java;
        }
        self.tools_ok = s.tools;
        self.adb_ok = s.adb;
        self.emu_ok = s.emulator;
        self.installed = s.installed;
        self.devices = s.devices;
        if self.sel_device.map_or(false, |i| i >= self.devices.len()) {
            self.sel_device = None;
        }
        let still_ok = self.new_image.map_or(false, |i| self.installed[i]);
        if !still_ok {
            self.new_image = self.installed.iter().position(|&b| b);
        }
    }

    // ---------- button handlers ----------

    fn on_install_image(&mut self) {
        let Some(i) = self.sel_image else {
            self.info = Some("Select an image first.".to_string());
            return;
        };
        if self.installed[i] {
            self.info = Some("That image is already installed.".to_string());
            return;
        }
        self.confirm = Some((
            format!("Download {}?\nThis can be 1-2 GB.", self.images[i].display_name()),
            Action::InstallImage(i),
        ));
    }

    fn on_delete_image(&mut self) {
        let Some(i) = self.sel_image else {
            self.info = Some("Select an image first.".to_string());
            return;
        };
        if !self.installed[i] {
            self.info = Some("That image is not installed.".to_string());
            return;
        }
        match devices::devices_using(&self.images[i]) {
            Ok(users) if !users.is_empty() => {
                self.info = Some(format!(
                    "Can't delete this image. Used by: {}.\nDelete those devices first (Devices tab).",
                    users.join(", ")
                ));
            }
            Ok(_) => {
                self.confirm = Some((
                    format!("Delete {}?", self.images[i].display_name()),
                    Action::DeleteImage(i),
                ));
            }
            Err(e) => self.push_log(format!("ERROR: {e}")),
        }
    }

    fn on_create_device(&mut self) {
        let name = self.new_name.trim().to_string();
        if name.is_empty() {
            self.info = Some("Type a name for the device.".to_string());
            return;
        }
        let Some(i) = self.new_image else {
            self.info = Some("Pick an installed image.".to_string());
            return;
        };
        let img = self.images[i].clone();
        let preset = self.new_preset;
        self.new_name.clear();
        self.spawn_job(false, move |rep| devices::create(&name, &img, preset, &rep.logger()));
    }

    fn on_delete_device(&mut self) {
        let Some(i) = self.sel_device else {
            self.info = Some("Select a device first.".to_string());
            return;
        };
        let name = self.devices[i].name.clone();
        if self.running.as_ref().map_or(false, |r| r.name == name) {
            self.info = Some("Stop this device before deleting it.".to_string());
            return;
        }
        self.confirm = Some((
            format!("Delete device \"{name}\"? Its data will be erased."),
            Action::DeleteDevice(name),
        ));
    }

    fn on_start_device(&mut self) {
        let Some(i) = self.sel_device else {
            self.info = Some("Select a device first.".to_string());
            return;
        };
        if self.running.is_some() {
            self.info = Some("An emulator is already running. Stop it first.".to_string());
            return;
        }
        if !self.emu_ok {
            self.info = Some("The Android Emulator is not installed. Run Setup first.".to_string());
            return;
        }
        let name = self.devices[i].name.clone();
        let opts = self.opts.clone();
        let pid = Arc::new(AtomicU32::new(0));
        let alive = Arc::new(AtomicBool::new(true));
        self.running = Some(Running {
            name: name.clone(),
            pid: Arc::clone(&pid),
            alive: Arc::clone(&alive),
            stopping: false,
        });

        // A watcher thread runs the emulator and waits for it to exit.
        let rep = self.rep.clone();
        thread::spawn(move || {
            let log = rep.logger();
            let code = match launcher::start(&name, &opts, &log, &|p| pid.store(p, Ordering::SeqCst)) {
                Ok(c) => c,
                Err(e) => {
                    rep.send(Msg::Log(format!("ERROR: {e}")));
                    -1
                }
            };
            alive.store(false, Ordering::SeqCst);
            rep.send(Msg::EmuExited { code });
        });
    }

    fn on_stop_device(&mut self) {
        let Some(r) = self.running.as_mut() else { return };
        if r.stopping {
            return;
        }
        r.stopping = true;
        let pid = Arc::clone(&r.pid);
        let alive = Arc::clone(&r.alive);
        let rep = self.rep.clone();
        thread::spawn(move || {
            let log = rep.logger();
            log("Stopping emulator...".to_string());
            let _ = launcher::stop_graceful(&log); // polite: adb emu kill
            for _ in 0..40 {
                if !alive.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(Duration::from_millis(500));
            }
            log("The emulator did not close in time - forcing it.".to_string());
            launcher::kill_tree(pid.load(Ordering::SeqCst));
        });
    }

    // ---------- tabs ----------

    fn ui_setup(&mut self, ui: &mut egui::Ui) {
        ui.heading("Setup");
        ui.add_space(6.0);
        egui::Grid::new("status").num_columns(2).spacing([20.0, 8.0]).show(ui, |ui| {
            status_row(ui, "Virtualization", self.virt.0, &self.virt.1);
            let java_text = if self.java_ok {
                "Found"
            } else {
                "Not found - a small runtime (~50 MB) will be downloaded"
            };
            status_row(ui, "Java 17+ (for SDK tools)", self.java_ok, java_text);
            status_row(ui, "SDK command-line tools", self.tools_ok, yes_no(self.tools_ok));
            status_row(ui, "Platform-tools (adb)", self.adb_ok, yes_no(self.adb_ok));
            status_row(ui, "Android Emulator", self.emu_ok, yes_no(self.emu_ok));
        });
        ui.add_space(10.0);

        let all_ready = self.java_ok && self.tools_ok && self.adb_ok && self.emu_ok;
        let (mut setup, mut check) = (false, false);
        ui.horizontal(|ui| {
            setup = ui
                .add_enabled(!self.busy && !all_ready, egui::Button::new("Set up Android SDK"))
                .clicked();
            check = ui.add_enabled(!self.busy, egui::Button::new("Check again")).clicked();
        });
        if setup {
            self.confirm = Some((SETUP_TEXT.to_string(), Action::Setup));
        }
        if check {
            self.refresh(true);
        }
    }

    fn ui_images(&mut self, ui: &mut egui::Ui) {
        ui.heading("System Images");
        if !self.tools_ok {
            ui.colored_label(RED, "Set up the Android SDK first (Setup tab).");
        }
        ui.add_space(6.0);

        let mut sel = self.sel_image;
        egui::Grid::new("images").striped(true).num_columns(3).spacing([28.0, 6.0]).show(ui, |ui| {
            ui.strong("Android");
            ui.strong("Type");
            ui.strong("Status");
            ui.end_row();
            for (i, img) in self.images.iter().enumerate() {
                if ui.selectable_label(sel == Some(i), img.android_label()).clicked() {
                    sel = Some(i);
                }
                ui.label(img.variant);
                if self.installed[i] {
                    ui.colored_label(GREEN, "Installed");
                } else {
                    ui.label("Not installed");
                }
                ui.end_row();
            }
        });
        self.sel_image = sel;

        ui.add_space(6.0);
        ui.label(
            "Tip: \"Google APIs\" allows root. \"Google Play\" has the Play Store but can't be rooted. \
             \"AOSP\" has no Google services and is the lightest.",
        );
        ui.add_space(8.0);

        let (mut install, mut delete, mut refresh) = (false, false, false);
        ui.horizontal(|ui| {
            install = ui
                .add_enabled(!self.busy && self.tools_ok, egui::Button::new("Install selected"))
                .clicked();
            delete = ui
                .add_enabled(!self.busy && self.tools_ok, egui::Button::new("Delete selected"))
                .clicked();
            refresh = ui.add_enabled(!self.busy, egui::Button::new("Refresh")).clicked();
        });
        if install {
            self.on_install_image();
        }
        if delete {
            self.on_delete_image();
        }
        if refresh {
            self.refresh(false);
        }
    }

    fn ui_devices(&mut self, ui: &mut egui::Ui) {
        ui.heading("Devices");
        let any_installed = self.installed.iter().any(|&b| b);
        if !self.tools_ok {
            ui.colored_label(RED, "Set up the Android SDK first (Setup tab).");
        } else if !any_installed {
            ui.colored_label(RED, "Install a system image first (System Images tab).");
        }
        ui.add_space(6.0);

        if self.devices.is_empty() {
            ui.label("No devices yet. Create one below.");
        } else {
            let mut sel = self.sel_device;
            egui::Grid::new("devices").striped(true).num_columns(6).spacing([24.0, 6.0]).show(ui, |ui| {
                for h in ["Name", "Android", "RAM", "Cores", "Screen", "Status"] {
                    ui.strong(h);
                }
                ui.end_row();
                for (i, d) in self.devices.iter().enumerate() {
                    if ui.selectable_label(sel == Some(i), &d.name).clicked() {
                        sel = Some(i);
                    }
                    ui.label(friendly(&self.images, &d.sysdir));
                    ui.label(format!("{} MB", d.ram));
                    ui.label(&d.cores);
                    ui.label(&d.screen);
                    if self.running.as_ref().map_or(false, |r| r.name == d.name) {
                        ui.colored_label(GREEN, "Running");
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
            self.sel_device = sel;
        }

        ui.add_space(8.0);
        let running_now = self.running.is_some();
        let stopping = self.running.as_ref().map_or(false, |r| r.stopping);
        let (mut start, mut stop) = (false, false);
        ui.horizontal(|ui| {
            start = ui
                .add_enabled(!running_now && !self.busy && self.tools_ok, egui::Button::new("Start selected"))
                .clicked();
            stop = ui.add_enabled(running_now && !stopping, egui::Button::new("Stop")).clicked();
            if let Some(r) = &self.running {
                ui.colored_label(GREEN, format!("Running: {}", r.name));
            }
        });
        egui::CollapsingHeader::new("Launch options (lean by default)").show(ui, |ui| {
            ui.checkbox(&mut self.opts.no_audio, "Mute audio (saves CPU)");
            ui.checkbox(&mut self.opts.no_boot_anim, "Skip boot animation");
            ui.checkbox(&mut self.opts.quick_boot, "Quick boot (resume from saved state)");
            ui.checkbox(&mut self.opts.no_save, "Don't save state on exit (saves disk space)");
            ui.horizontal(|ui| {
                ui.label("Graphics:");
                egui::ComboBox::from_id_salt("gpu").selected_text(self.opts.gpu.label()).show_ui(ui, |ui| {
                    for g in Gpu::ALL {
                        ui.selectable_value(&mut self.opts.gpu, g, g.label());
                    }
                });
            });
        });
        if start {
            self.on_start_device();
        }
        if stop {
            self.on_stop_device();
        }

        ui.add_space(8.0);
        ui.separator();
        ui.strong("New device");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("Device name").desired_width(150.0));

            let img_text = self
                .new_image
                .map(|i| self.images[i].display_name())
                .unwrap_or_else(|| "Pick an image".to_string());
            egui::ComboBox::from_id_salt("new_image").selected_text(img_text).show_ui(ui, |ui| {
                for (i, img) in self.images.iter().enumerate() {
                    if self.installed[i] {
                        ui.selectable_value(&mut self.new_image, Some(i), img.display_name());
                    }
                }
            });

            egui::ComboBox::from_id_salt("new_preset")
                .selected_text(self.new_preset.label())
                .show_ui(ui, |ui| {
                    for p in Preset::ALL {
                        ui.selectable_value(&mut self.new_preset, p, p.label());
                    }
                });
        });
        ui.label(self.new_preset.description());
        ui.add_space(8.0);

        let (mut create, mut delete, mut refresh) = (false, false, false);
        ui.horizontal(|ui| {
            create = ui
                .add_enabled(
                    !self.busy && self.tools_ok && any_installed,
                    egui::Button::new("Create device"),
                )
                .clicked();
            delete = ui
                .add_enabled(!self.busy && self.tools_ok, egui::Button::new("Delete selected"))
                .clicked();
            refresh = ui.add_enabled(!self.busy, egui::Button::new("Refresh")).clicked();
        });
        if create {
            self.on_create_device();
        }
        if delete {
            self.on_delete_device();
        }
        if refresh {
            self.refresh(false);
        }
    }

    // ---------- dialogs ----------

    fn dialogs(&mut self, ctx: &egui::Context) {
        // confirm
        let mut decision: Option<bool> = None;
        if let Some((text, _)) = &self.confirm {
            egui::Window::new("Please confirm")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(text);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            decision = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(false);
                        }
                    });
                });
        }
        match decision {
            Some(true) => {
                if let Some((_, action)) = self.confirm.take() {
                    self.run_action(action);
                }
            }
            Some(false) => self.confirm = None,
            None => {}
        }

        // info
        let mut close = false;
        if let Some(text) = &self.info {
            egui::Window::new("Info")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(text);
                    ui.add_space(8.0);
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                });
        }
        if close {
            self.info = None;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();

        // Don't leave an emulator running (with a closed log pipe) when the manager closes.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.running.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some((
                "An emulator is running. Stop it and quit?".to_string(),
                Action::StopAndQuit,
            ));
        }

        let before = self.tab;
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Setup, "Setup");
                ui.selectable_value(&mut self.tab, Tab::Images, "System Images");
                ui.selectable_value(&mut self.tab, Tab::Devices, "Devices");
            });
        });
        if self.tab != before {
            self.refresh(false);
        }

        egui::TopBottomPanel::bottom("log").resizable(true).default_height(170.0).show(ctx, |ui| {
            if self.busy {
                ui.horizontal(|ui| match self.progress {
                    Some(p) => {
                        ui.add(egui::ProgressBar::new(p).show_percentage());
                    }
                    None => {
                        ui.spinner();
                        ui.label("Working...");
                    }
                });
            }
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for line in &self.log {
                    ui.label(egui::RichText::new(line).monospace());
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Setup => self.ui_setup(ui),
            Tab::Images => self.ui_images(ui),
            Tab::Devices => self.ui_devices(ui),
        });

        self.dialogs(ctx);
    }
}

// ---------- small helpers ----------

fn status_row(ui: &mut egui::Ui, name: &str, ok: bool, text: &str) {
    ui.strong(name);
    let (prefix, color) = if ok { ("OK - ", GREEN) } else { ("Missing - ", RED) };
    ui.colored_label(color, format!("{prefix}{text}"));
    ui.end_row();
}

fn yes_no(ok: bool) -> &'static str {
    if ok { "Installed" } else { "Not installed" }
}

/// Turn "system-images/android-34/google_apis/x86_64/" into a readable name when we know it.
fn friendly(images: &[SystemImage], sysdir: &str) -> String {
    let norm = devices::normalize_sysdir(sysdir);
    for img in images {
        if devices::normalize_sysdir(&img.package_id.replace(';', "/")) == norm {
            return img.display_name();
        }
    }
    sysdir.to_string()
}
