use crate::gui::ManagerApp;
use eframe::egui;

pub fn show(app: &mut ManagerApp, ui: &mut egui::Ui) {
    ui.add_space(ManagerApp::SECTION_GAP);
    ui.horizontal(|ui| {
        ui.heading("アクセスログ");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("更新").clicked() {
                app.refresh();
            }
        });
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for log in &app.logs {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(&log.timestamp);
                    ui.separator();
                    ui.strong(&log.machine_name);
                    ui.label(format!("({})", log.username));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (text, color) = if log.exit_code == Some(0) {
                            ("成功", egui::Color32::GREEN)
                        } else {
                            ("失敗", egui::Color32::RED)
                        };
                        ui.label(egui::RichText::new(text).color(color));
                    });
                });
                ui.label(egui::RichText::new(&log.command).monospace());
                if let Some(err) = &log.stderr {
                    if !err.is_empty() {
                        ui.collapsing("エラー出力", |ui| {
                            ui.label(
                                egui::RichText::new(err)
                                    .color(egui::Color32::LIGHT_RED)
                                    .monospace(),
                            );
                        });
                    }
                }
            });
        }
    });
}
