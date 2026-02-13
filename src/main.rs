#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod db;
mod gui;
mod knowledge;

use crate::db::{Account, DbHandler, Machine};
use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "mcp-ssh-manager")]
#[command(about = "SSH connection manager with GUI, MCP, and CLI interfaces", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all registered machines
    List,
    /// Add a new machine
    Add {
        /// Alias name for the machine
        name: String,
        /// Hostname or IP address
        ip: String,
        /// Purpose of the machine
        #[arg(long)]
        purpose: String,
        /// Ownership (e.g., personal, company)
        #[arg(long, default_value = "personal")]
        owner: String,
        /// OS Type (linux, windows, macos)
        #[arg(long, default_value = "windows")]
        os: String,
    },
    /// Create a database backup immediately
    Backup {
        /// Optional path to save the backup
        path: Option<String>,
    },
    /// Run as a headless MCP server (no GUI)
    Mcp,
}

#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<Value>,
    id: Option<Value>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let db = Arc::new(DbHandler::new()?);

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::List => {
                let machines = db.list_machines()?;
                println!(
                    "{:<20} {:<20} {:<10} {:<10}",
                    "Name", "IP Address", "Status", "OS"
                );
                println!("{}", "-".repeat(65));
                for m in machines {
                    println!(
                        "{:<20} {:<20} {:<10} {:<10}",
                        m.name, m.ip_address, m.status, m.os_type
                    );
                }
                return Ok(());
            }
            Commands::Add {
                name,
                ip,
                purpose,
                owner,
                os,
            } => {
                let machine = Machine {
                    id: None,
                    name: name.clone(),
                    ip_address: ip,
                    purpose,
                    ownership: owner,
                    os_type: os,
                    status: "active".to_string(),
                };
                let id = db.add_machine(machine)?;
                println!("Machine '{}' added successfully with ID: {}", name, id);
                return Ok(());
            }
            Commands::Backup { path } => {
                let backup_path = if let Some(p) = path {
                    std::path::PathBuf::from(p)
                } else {
                    let today = chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string();
                    let proj_dirs =
                        directories::ProjectDirs::from("com", "veltrea", "mcp-ssh-manager")
                            .unwrap();
                    let backup_dir = proj_dirs.data_dir().join("backups");
                    let _ = std::fs::create_dir_all(&backup_dir);
                    backup_dir.join(format!("manual_backup_{}.db", today))
                };
                db.backup_db(&backup_path)?;
                println!("Backup created at: {:?}", backup_path);
                return Ok(());
            }
            Commands::Mcp => {
                println!("Running in headless MCP mode...");
                run_mcp_loop(db)?;
                return Ok(());
            }
        }
    } else {
        // Default: Launch GUI + Spawn MCP thread
        let db_for_mcp = Arc::clone(&db);
        std::thread::spawn(move || {
            if let Err(e) = run_mcp_loop(db_for_mcp) {
                eprintln!("MCP Loop Error: {}", e);
            }
        });

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 400.0]),
            ..Default::default()
        };

        eframe::run_native(
            "MCP-SSH Manager",
            options,
            Box::new(|cc| {
                let app = gui::ManagerApp::new(cc, db);
                app.check_auto_backup();
                Box::new(app)
            }),
        )
        .map_err(|e| anyhow!("GUI error: {}", e))?;
    }

    Ok(())
}

fn run_mcp_loop(db: Arc<DbHandler>) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    while reader.read_line(&mut line)? > 0 {
        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => {
                line.clear();
                continue;
            }
        };

        // We use a simple blocking handle in this thread
        let res = handle_request_sync(req, &db);
        let res_json = serde_json::to_string(&res)?;
        println!("{}", res_json);
        io::stdout().flush()?;

        line.clear();
    }
    Ok(())
}

fn handle_request_sync(req: JsonRpcRequest, db: &DbHandler) -> JsonRpcResponse {
    let id = req.id.clone();
    let result = match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "mcp-ssh-manager", "version": "0.2.0" }
        })),
        "notifications/initialized" => Ok(Value::Null),
        "tools/list" => Ok(json!({
            "tools": [
                {
                    "name": "register_machine",
                    "description": "Register a new machine",
                    "inputSchema": { "type": "object", "properties": { "name": { "type": "string" }, "ip_address": { "type": "string" }, "purpose": { "type": "string" }, "ownership": { "type": "string" }, "os_type": { "type": "string" }, "username": { "type": "string" }, "auth_type": { "type": "string" }, "credential": { "type": "string" } }, "required": ["name", "ip_address", "purpose", "ownership", "os_type", "username", "auth_type", "credential"] }
                },
                {
                    "name": "list_machines",
                    "description": "List all registered machines",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "diagnose_connection",
                    "description": "Diagnose SSH connection issues and provide agentic hints",
                    "inputSchema": { "type": "object", "properties": { "machine_id": { "type": "integer" } }, "required": ["machine_id"] }
                },
                {
                    "name": "rotate_keys",
                    "description": "Rotate the master encryption key and re-encrypt all stored credentials",
                    "inputSchema": { "type": "object", "properties": {} }
                }
            ]
        })),
        "tools/call" => {
            if let Some(params) = req.params {
                let name = params.get("name").and_then(|v| v.as_str());
                let arguments = params.get("arguments");
                match (name, arguments) {
                    (Some("register_machine"), Some(args)) => {
                        handle_register_machine_sync(args, db)
                    }
                    (Some("list_machines"), _) => handle_list_machines_sync(db),
                    (Some("diagnose_connection"), Some(args)) => {
                        handle_diagnose_connection(args, db)
                    }
                    (Some("rotate_keys"), _) => handle_rotate_keys(db),
                    _ => Err(anyhow!("Unknown tool")),
                }
            } else {
                Err(anyhow!("Missing params"))
            }
        }
        _ => Err(anyhow!("Method not found")),
    };

    match result {
        Ok(res) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(res),
            error: None,
            id,
        },
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(json!({ "code": -32603, "message": e.to_string() })),
            id,
        },
    }
}

fn handle_register_machine_sync(args: &Value, db: &DbHandler) -> Result<Value> {
    let get_str = |key: &str| -> Result<String> {
        args.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Missing or invalid argument: {}", key))
    };

    let machine = Machine {
        id: None,
        name: get_str("name")?,
        ip_address: get_str("ip_address")?,
        purpose: args
            .get("purpose")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        ownership: get_str("ownership")?,
        os_type: get_str("os_type")?,
        status: "active".to_string(),
    };

    let machine_id = db
        .add_machine(machine)
        .map_err(|e| anyhow!("Failed to add machine: {}", e))?;

    let account = Account {
        id: None,
        machine_id,
        username: get_str("username")?,
        auth_type: get_str("auth_type")?,
        credential: get_str("credential")?,
    };

    db.add_account(account)
        .map_err(|e| anyhow!("Failed to add account: {}", e))?;

    Ok(
        json!({ "content": [{ "type": "text", "text": format!("Machine registered with ID {}", machine_id) }] }),
    )
}

fn handle_list_machines_sync(db: &DbHandler) -> Result<Value> {
    let machines = db.list_machines()?;
    Ok(json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&machines)? }] }))
}

fn handle_diagnose_connection(args: &Value, db: &DbHandler) -> Result<Value> {
    let machine_id = args
        .get("machine_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("machine_id is required"))?;

    // 1. Fetch machine and account info
    let machines = db.list_machines()?;
    let machine = machines
        .into_iter()
        .find(|m| m.id == Some(machine_id))
        .ok_or_else(|| anyhow!("Machine not found"))?;

    let accounts = db.list_accounts()?;
    let account = accounts
        .into_iter()
        .find(|a| a.machine_id == machine_id)
        .ok_or_else(|| anyhow!("No account found for machine"))?;

    let target = format!("{}@{}", account.username, machine.ip_address);
    println!("Diagnosing connection to {}...", target);

    // 2. Run SSH command (capturing stderr)
    // Use BatchMode=yes to avoid interactivity, ConnectTimeout=5 to avoid hanging
    let output = std::process::Command::new("ssh")
        .args(&[
            "-v",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            &target,
            "echo",
            "connection_success",
        ])
        .output()
        .map_err(|e| anyhow!("Failed to execute ssh command: {}", e))?;

    if output.status.success() {
        return Ok(json!({
            "content": [{ "type": "text", "text": "Connection successful. No issues detected." }]
        }));
    }

    // 3. Analyze failure
    let stderr = String::from_utf8_lossy(&output.stderr);
    let patterns = knowledge::load_troubleshooting_patterns();

    let mut response_text = format!("SSH Connection Failed.\n\nSTDERR:\n{}\n\n", stderr);
    let mut hint_data = serde_json::Map::new();

    if let Some(suggestion) = knowledge::match_error_pattern(&stderr, &patterns) {
        response_text.push_str(&format!("--- AGENT HINT ---\n{}\n", suggestion.message));
        if let Some(cmd) = &suggestion.command_hint {
            response_text.push_str(&format!("Suggested Command: `{}`\n", cmd));
        }
        if let Some(script) = &suggestion.script_path {
            response_text.push_str(&format!("Suggested Script: `{}`\n", script));
        }

        // Structure for agent
        hint_data.insert("agent_hint".to_string(), json!(suggestion));
    } else {
        response_text.push_str("No specific troubleshooting hint found.");
    }

    Ok(json!({
        "content": [{ "type": "text", "text": response_text }],
        "data": hint_data
    }))
}

fn handle_rotate_keys(db: &DbHandler) -> Result<Value> {
    db.rotate_keys()?;
    Ok(json!({
        "content": [{ "type": "text", "text": "Master key rotated and all credentials re-encrypted successfully." }]
    }))
}
