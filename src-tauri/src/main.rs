// src-tauri/src/main.rs
// Entry point CARAKA Desktop — Fase 6: Full Tauri Integration
//
// Semua modul diaktifkan dan di-register ke Tauri runtime.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// ─── Module Declarations ───────────────────────────────────────────────────

pub mod commands;
pub mod crypto;
pub mod discovery;
pub mod keys;
pub mod packet;
pub mod routing;
pub mod state;
pub mod store;
pub mod sync;
pub mod transport;

use tauri::Emitter;
use tracing_subscriber::EnvFilter;

// ─── Main Entry Point ──────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Setup logging. Set environment variable RUST_LOG=debug untuk verbose output.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("RUST_LOG")
                .unwrap_or_else(|_| EnvFilter::new("caraka_desktop=info,warn"))
        )
        .init();

    tracing::info!("CARAKA Desktop dimulai...");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Inisialisasi semua komponen secara async
            tauri::async_runtime::spawn(async move {
                if let Err(e) = state::initialize(app_handle.clone()).await {
                    tracing::error!("Gagal inisialisasi CARAKA node: {}", e);
                    // Notify frontend tentang error
                    app_handle.emit("node_error", serde_json::json!({
                        "error": e.to_string()
                    })).ok();
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::init_node,
            commands::set_display_name,
            commands::send_dm,
            commands::try_decrypt_packet,
            commands::get_messages,
            commands::get_peers,
            commands::add_peer_manual,
            commands::get_network_status,
        ])
        .run(tauri::generate_context!())
        .expect("error saat menjalankan CARAKA Desktop");
}
