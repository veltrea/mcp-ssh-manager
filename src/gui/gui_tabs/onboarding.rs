use crate::db::{Account, Machine};
use crate::gui::ManagerApp;
use anyhow::{Result, anyhow};
use eframe::egui;

fn register_key_to_remote(app: &mut ManagerApp) -> Result<()> {
    let pubkey = app
        .generated_pubkey
        .as_ref()
        .ok_or_else(|| anyhow!("No key generated"))?;

    let command = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
        pubkey
    );

    println!(
        "🚀 Using one-time password to register hardware identity on {}...",
        app.reg_host
    );

    let (_stdout, stderr, exit_code) = rust_ssh::run_command(
        &app.reg_host,
        22,
        &app.reg_user,
        None,
        Some(&app.reg_pass),
        &command,
    )?;

    app.reg_pass.clear();

    if exit_code == 0 {
        let machine = Machine {
            id: None,
            name: app.reg_host.clone(),
            ip_address: app.reg_host.clone(),
            purpose: "Hardware-bound secure node".to_string(),
            ownership: "personal".to_string(),
            os_type: "linux".to_string(),
            status: "active".to_string(),
        };

        let machine_id = app.db.add_machine(machine)?;

        let account = Account {
            id: None,
            machine_id,
            username: app.reg_user.clone(),
            auth_type: "tpm".to_string(),
            credential: "TPM_HARDWARE_BOUND".to_string(),
        };
        app.db.add_account(account)?;

        app.refresh();
        Ok(())
    } else {
        Err(anyhow!(
            "Registration failed (exit {}): {}",
            exit_code,
            stderr
        ))
    }
}

pub fn show(app: &mut ManagerApp, ui: &mut egui::Ui) {
    ui.add_space(ManagerApp::SECTION_GAP);
    ui.heading("セキュリティ設定ウィザード");
    ui.label(format!("ステップ {}/5", app.onboarding_step + 1));
    ui.add_space(ManagerApp::SECTION_GAP);

    match app.onboarding_step {
        0 => {
            ui.label("このデバイスのセキュリティ状態を診断します。");
            egui::Grid::new("onboarding_security_check")
                .num_columns(2)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    ui.add_sized(
                        [ManagerApp::FORM_LABEL_WIDTH, 28.0],
                        egui::Label::new("TPM"),
                    );
                    if app.tpm_available {
                        ui.colored_label(egui::Color32::GREEN, "✅ 利用可能");
                    } else {
                        ui.colored_label(
                            egui::Color32::RED,
                            "❌ 未検出 (ソフトウェア鍵を使用します)",
                        );
                    }
                    ui.end_row();

                    ui.add_sized(
                        [ManagerApp::FORM_LABEL_WIDTH, 28.0],
                        egui::Label::new("Secure Boot"),
                    );
                    if app.secure_boot_enabled {
                        ui.colored_label(egui::Color32::GREEN, "✅ 有効");
                    } else {
                        ui.colored_label(egui::Color32::YELLOW, "⚠ 無効 (または診断不可)");
                    }
                });

            ui.add_space(12.0);
            if ui.button("次へ進む ➔").clicked() {
                app.onboarding_step = 1;
            }
        }
        1 => {
            ui.label("ステップ 2: ハードウェア識別鍵の生成");
            ui.label("ハードウェア(TPM/Secure Enclave)内で秘密鍵を生成します。");
            ui.label("生成された秘密鍵はデバイス外に持ち出すことはできません。");

            ui.add_space(10.0);
            if ui.button("鍵を生成する").clicked() {
                match rust_ssh::security::tpm::generate_tpm_key() {
                    Ok(key) => {
                        app.generated_pubkey = Some(key);
                        app.onboarding_step = 2;
                    }
                    Err(e) => {
                        eprintln!("鍵生成に失敗しました: {}", e);
                    }
                }
            }

            if ui.button("⬅ 戻る").clicked() {
                app.onboarding_step = 0;
            }
        }
        2 => {
            ui.label("ステップ 3: 公開鍵の登録");
            ui.label("以下の公開鍵を、接続先のサーバーに登録してください。");

            if let Some(key) = &app.generated_pubkey {
                ui.group(|ui| {
                    ui.label(egui::RichText::new(key).monospace());
                    if ui.button("クリップボードにコピー").clicked() {
                        ui.output_mut(|o| o.copied_text = key.clone());
                    }
                });
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("自動でサーバーに登録する").clicked() {
                    app.onboarding_step = 3;
                }
                if ui.button("完了").clicked() {
                    app.current_tab = crate::gui::Tab::Connections;
                }
            });
        }
        3 => {
            ui.label("ステップ 4: リモートサーバーへの自動登録 (One-Time Password Flow)");
            ui.label("このデバイスの公開鍵をリモートサーバーの authorized_keys に追加します。");
            ui.label(
                "パスワードは本プロセスでの一回限りの使用となり、データベースには保存されません。",
            );
            ui.add_space(8.0);
            ui.colored_label(
                egui::Color32::YELLOW,
                "⚠ 以降、このPCのハードウェアチップ(TPM)による認証に切り替わります。",
            );

            egui::Grid::new("onboarding_register_grid")
                .num_columns(2)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    ui.add_sized(
                        [ManagerApp::FORM_LABEL_WIDTH, 28.0],
                        egui::Label::new("ホスト"),
                    );
                    ui.add_sized(
                        [ManagerApp::FORM_FIELD_WIDTH, 28.0],
                        egui::TextEdit::singleline(&mut app.reg_host),
                    );
                    ui.end_row();

                    ui.add_sized(
                        [ManagerApp::FORM_LABEL_WIDTH, 28.0],
                        egui::Label::new("ユーザー"),
                    );
                    ui.add_sized(
                        [ManagerApp::FORM_FIELD_WIDTH, 28.0],
                        egui::TextEdit::singleline(&mut app.reg_user),
                    );
                    ui.end_row();

                    ui.add_sized(
                        [ManagerApp::FORM_LABEL_WIDTH, 28.0],
                        egui::Label::new("パスワード"),
                    );
                    ui.add_sized(
                        [ManagerApp::FORM_FIELD_WIDTH, 28.0],
                        egui::TextEdit::singleline(&mut app.reg_pass).password(true),
                    );
                    ui.end_row();
                });

            ui.add_space(8.0);
            if ui.button("登録を実行（一回限りのパスワード）").clicked() {
                match register_key_to_remote(app) {
                    Ok(_) => {
                        app.onboarding_step = 4;
                    }
                    Err(e) => {
                        eprintln!("登録に失敗しました: {}", e);
                    }
                }
            }

            if ui.button("⬅ 戻る").clicked() {
                app.onboarding_step = 2;
            }
        }
        4 => {
            ui.label("ワンタイム登録が完了しました。");
            ui.label("パスワードはメモリから即座に破棄されました。");
            ui.label("データベースには「ハードウェア（TPM）認証」として登録されています。");
            ui.add_space(12.0);
            if ui.button("終了してダッシュボードへ").clicked() {
                app.current_tab = crate::gui::Tab::Connections;
            }
        }
        _ => {
            app.onboarding_step = 0;
        }
    }
}
