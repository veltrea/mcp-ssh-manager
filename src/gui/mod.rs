use crate::db::{Account, DbHandler, Machine};
use anyhow::Result;
use eframe::egui;
use std::sync::Arc;

mod gui_tabs;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Tab {
    Connections,
    Accounts,
    Logs,
    Onboarding,
}

pub struct ManagerApp {
    pub db: Arc<DbHandler>,
    pub current_tab: Tab,
    pub machines: Vec<Machine>,
    pub accounts: Vec<Account>,
    pub search_query: String,

    pub adding_machine: Option<Machine>,
    pub updating_account: Option<(i64, String)>,
    pub new_credential: String,
    pub logs: Vec<crate::db::CommandLog>,

    // Onboarding State
    pub onboarding_step: usize,
    pub tpm_available: bool,
    pub secure_boot_enabled: bool,
    pub generated_pubkey: Option<String>,
    pub reg_host: String,
    pub reg_user: String,
    pub reg_pass: String,
}

impl ManagerApp {
    pub const FORM_LABEL_WIDTH: f32 = 132.0;
    pub const FORM_FIELD_WIDTH: f32 = 340.0;
    pub const SECTION_GAP: f32 = 12.0;

    pub fn new(_cc: &eframe::CreationContext<'_>, db: Arc<DbHandler>) -> Self {
        // Keep defaults first; add Japanese font as fallback to avoid oversized/imbalanced text.
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(font_data) = std::fs::read("/System/Library/Fonts/Hiragino Sans GB.ttc") {
            fonts
                .font_data
                .insert("Hiragino".to_owned(), egui::FontData::from_owned(font_data));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("Hiragino".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("Hiragino".to_owned());
        } else {
            eprintln!("Failed to load system font for Japanese characters.");
        }

        _cc.egui_ctx.set_fonts(fonts);
        Self::apply_global_ui_style(&_cc.egui_ctx);

        let mut app = Self {
            db,
            current_tab: Tab::Connections,
            machines: Vec::new(),
            accounts: Vec::new(),
            search_query: String::new(),

            adding_machine: None,
            updating_account: None,
            new_credential: String::new(),
            logs: Vec::new(),
            onboarding_step: 0,
            tpm_available: false,
            secure_boot_enabled: false,
            generated_pubkey: None,
            reg_host: String::new(),
            reg_user: String::new(),
            reg_pass: String::new(),
        };
        app.refresh();
        app.check_security_features();
        app
    }

    fn apply_global_ui_style(ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(13.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(16.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(12.0, egui::FontFamily::Proportional),
        );
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.window_margin = egui::Margin::same(12.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.text_edit_width = 280.0;
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(24, 24, 26);
        style.visuals.faint_bg_color = egui::Color32::from_rgb(30, 30, 33);
        ctx.set_style(style);
    }

    pub fn refresh(&mut self) {
        if let Ok(machines) = self.db.list_machines() {
            self.machines = machines;
        }
        if let Ok(accounts) = self.db.list_accounts() {
            self.accounts = accounts;
        }
        if let Ok(logs) = self.db.list_logs() {
            self.logs = logs;
        }
    }

    pub fn check_auto_backup(&self) {
        // Simple logic: check if backup exists for today, if not create one
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let proj_dirs =
            directories::ProjectDirs::from("com", "veltrea", "mcp-ssh-manager").unwrap();
        let backup_dir = proj_dirs.data_dir().join("backups");
        let _ = std::fs::create_dir_all(&backup_dir);

        // Cleanup old backups (keep last 5)
        if let Ok(entries) = std::fs::read_dir(&backup_dir) {
            let mut backups: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "db"))
                .collect();

            backups.sort_by_key(|b| b.metadata().ok().map(|m| m.modified().ok()).flatten());

            if backups.len() > 5 {
                for entry in backups.iter().take(backups.len() - 5) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }

        let backup_path = backup_dir.join(format!("auto_backup_{}.db", today));

        if !backup_path.exists() {
            let _ = self.db.backup_db(&backup_path);
        }
    }

    fn check_security_features(&mut self) {
        // Mock check (Task 7)
        self.tpm_available = rust_ssh::security::tpm::is_tpm_available();
        self.secure_boot_enabled = true; // Placeholder
    }
}

impl eframe::App for ManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("MCP-SSH Manager").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("v0.2.0").small());
                    if let Some(user_dir) = directories::UserDirs::new() {
                        ui.label(
                            egui::RichText::new(format!(
                                "User: {}",
                                user_dir.home_dir().to_string_lossy()
                            ))
                            .small(),
                        );
                    }
                });
            });
        });

        egui::SidePanel::left("side_panel")
            .resizable(false)
            .default_width(170.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    self.render_tab(ui, Tab::Connections, "接続先");
                    self.render_tab(ui, Tab::Accounts, "アカウント");
                    self.render_tab(ui, Tab::Logs, "ログ");
                    self.render_tab(ui, Tab::Onboarding, "セキュリティ設定");

                    ui.add_space(8.0);
                    if ui.button("データ再読込").clicked() {
                        self.refresh();
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.current_tab {
            Tab::Connections => gui_tabs::connections::show(self, ui, ctx),
            Tab::Accounts => gui_tabs::accounts::show(self, ui),
            Tab::Logs => gui_tabs::logs::show(self, ui),
            Tab::Onboarding => gui_tabs::onboarding::show(self, ui),
        });
    }
}

impl ManagerApp {
    fn render_tab(&mut self, ui: &mut egui::Ui, tab: Tab, label: &str) {
        let is_active = self.current_tab == tab;

        let (bg_color, text_color) = if is_active {
            (egui::Color32::from_rgb(45, 45, 50), egui::Color32::WHITE)
        } else {
            (
                egui::Color32::from_rgb(25, 25, 28),
                egui::Color32::from_gray(180),
            )
        };

        let response = ui.add(
            egui::Button::new(egui::RichText::new(label).color(text_color))
                .fill(bg_color)
                .rounding(egui::Rounding {
                    nw: 6.0,
                    ne: 6.0,
                    sw: 6.0,
                    se: 6.0,
                })
                .min_size(egui::vec2(140.0, 30.0))
                .frame(true),
        );

        if response.clicked() {
            self.current_tab = tab;
        }

        // Active tab indicator (bottom border)
        if is_active {
            let rect = response.rect;
            let bottom_left = egui::pos2(rect.left(), rect.bottom());
            let bottom_right = egui::pos2(rect.right(), rect.bottom());
            ui.painter().line_segment(
                [bottom_left, bottom_right],
                egui::Stroke::new(3.0, egui::Color32::from_rgb(10, 132, 255)),
            );
        }
    }
}

pub fn launch_ssh_terminal(machine: &Machine, account: &Account) -> Result<()> {
    let target = format!("{}@{}", account.username, machine.ip_address);
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Terminal\" to do script \"ssh {}\"",
            target
        );
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(&["/C", "start", "ssh", &target])
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        if std::process::Command::new("gnome-terminal")
            .arg("--")
            .arg("ssh")
            .arg(&target)
            .spawn()
            .is_err()
        {
            eprintln!("Linux terminal launch not fully implemented for all distros");
        }
    }
    Ok(())
}
