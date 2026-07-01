// src-tauri/src/main.rs
// Entry point CARAKA Desktop — Fase 6: Full Tauri Integration
//
// Flow startup (F1 — Argon2id Vault):
//   1. setup() → manage empty Arc<Mutex<Option<AppState>>>
//   2. pre_initialize() → emit vault_check ke frontend
//   3. User input passphrase → create_vault / unlock_vault command
//   4. complete_initialize() → isi AppState, start services, emit node_ready

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod commands;
pub mod crypto;
pub mod discovery;
pub mod hotspot;
pub mod keys;
pub mod network_monitor;
pub mod packet;
pub mod routing;
pub mod state;
pub mod store;
pub mod sync;
pub mod transport;
pub mod vault;

use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("RUST_LOG")
                .unwrap_or_else(|_| EnvFilter::new("caraka_desktop=info,warn")),
        )
        .init();

    tracing::info!("CARAKA Desktop dimulai...");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            #[cfg(debug_assertions)]
            {
                if let Some(win) = app.get_webview_window("main") {
                    win.open_devtools();
                }
            }

            // Pre-register empty state SEBELUM async task apa pun.
            // Semua command yang butuh AppState akan return error sampai vault di-unlock.
            app_handle.manage(Arc::new(Mutex::new(Option::<state::AppState>::None)));

            // Phase 1: cek vault, emit hasil ke frontend
            tauri::async_runtime::spawn(async move {
                state::pre_initialize(app_handle.clone()).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // ─── Vault (F1) ───────────────────────────────────────────
            commands::check_vault_exists,
            commands::create_vault,
            commands::unlock_vault,
            // ─── Core ────────────────────────────────────────────────
            commands::init_node,
            commands::set_display_name,
            commands::send_dm,
            commands::try_decrypt_packet,
            commands::get_messages,
            commands::get_peers,
            commands::add_peer_manual,
            commands::get_network_status,
            commands::get_local_ip,
            commands::send_broadcast,
            // ─── Emergency Mode ───────────────────────────────────────
            commands::activate_emergency_hotspot,
            commands::deactivate_emergency_hotspot,
            commands::get_emergency_status,
            commands::reconnect_known_peers,
            commands::scan_emergency_network,
            // ─── QR & Safety ──────────────────────────────────────────
            commands::generate_peer_qr,
            commands::compute_safety_number,
        ])
        .run(tauri::generate_context!())
        .expect("error saat menjalankan CARAKA Desktop");
}
