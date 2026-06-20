// src-tauri/src/network_monitor.rs
// Emergency Mode — Network Monitor
//
// Background task yang memonitor status jaringan setiap 3 detik.
// Ketika router mati (misal mati lampu), semua interface non-loopback hilang.
// Monitor ini mendeteksi perubahan tersebut dan memberi tahu frontend.
//
// State transitions:
//   Normal → Lost       : saat interface hilang
//   Lost   → Normal     : saat interface kembali
//   Lost   → Emergency  : saat user aktifkan hotspot (di-set dari commands.rs)
//   Emergency → Normal  : saat hotspot dimatikan + interface kembali

use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::{info, warn, debug};

/// State jaringan saat ini
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkState {
    /// Router WiFi aktif — mode normal
    Normal,
    /// Jaringan hilang (kemungkinan mati lampu) — belum ada tindakan
    Lost,
    /// Emergency hotspot aktif — satu laptop jadi hotspot
    Emergency,
}

impl std::fmt::Display for NetworkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkState::Normal    => write!(f, "Normal"),
            NetworkState::Lost      => write!(f, "Jaringan Hilang"),
            NetworkState::Emergency => write!(f, "Mode Darurat"),
        }
    }
}

/// Cek apakah ada interface jaringan aktif (non-loopback, non-link-local).
///
/// Return false jika hanya ada loopback atau link-local (169.254.x.x) —
/// artinya router mati atau tidak ada koneksi.
pub fn has_active_network() -> bool {
    let ifaces = match get_if_addrs::get_if_addrs() {
        Ok(i) => i,
        Err(_) => return false,
    };

    ifaces.iter().any(|iface| {
        if iface.is_loopback() {
            return false;
        }
        if let get_if_addrs::IfAddr::V4(ref v4) = iface.addr {
            let octets = v4.ip.octets();
            // Bukan link-local (169.254.x.x)
            !(octets[0] == 169 && octets[1] == 254)
        } else {
            false
        }
    })
}

/// Cek apakah hotspot Windows (192.168.137.x) aktif sebagai interface.
pub fn has_hotspot_interface() -> bool {
    let ifaces = match get_if_addrs::get_if_addrs() {
        Ok(i) => i,
        Err(_) => return false,
    };

    ifaces.iter().any(|iface| {
        if let get_if_addrs::IfAddr::V4(ref v4) = iface.addr {
            let octets = v4.ip.octets();
            // Windows Mobile Hotspot default subnet: 192.168.137.x
            octets[0] == 192 && octets[1] == 168 && octets[2] == 137
        } else {
            false
        }
    })
}

/// Start network monitor background task.
///
/// Memonitor status jaringan setiap 3 detik dan emit event ke frontend:
///   - "network_lost"     : jaringan hilang (mati lampu)
///   - "network_restored" : jaringan kembali
///   - "network_status"   : update status periodik (setiap 10 detik)
pub async fn start_network_monitor(
    app_handle: tauri::AppHandle,
    network_state: Arc<Mutex<NetworkState>>,
) {
    info!("Network monitor dimulai (interval: 3 detik)");

    let mut status_tick = 0u32;

    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;

        let currently_has_network = has_active_network();
        let currently_has_hotspot = has_hotspot_interface();

        let mut state = network_state.lock().await;

        // Clone current state untuk match (hindari borrow checker error)
        let current_state = state.clone();

        match (current_state, currently_has_network, currently_has_hotspot) {
            // Jaringan baru saja hilang
            (NetworkState::Normal, false, false) => {
                warn!("⚠️  Jaringan hilang — kemungkinan mati lampu atau router mati");
                *state = NetworkState::Lost;

                app_handle.emit("network_lost", serde_json::json!({
                    "message": "Jaringan terputus. Router mati atau mati lampu?",
                    "timestamp": unix_now(),
                })).ok();
            }

            // Jaringan kembali normal (dari Lost atau Emergency)
            (NetworkState::Lost | NetworkState::Emergency, true, _) if !currently_has_hotspot => {
                info!("✅ Jaringan kembali normal");
                *state = NetworkState::Normal;

                app_handle.emit("network_restored", serde_json::json!({
                    "message": "Jaringan kembali normal",
                    "timestamp": unix_now(),
                })).ok();
            }

            // Hotspot aktif — update ke Emergency state jika belum
            (NetworkState::Lost, _, true) => {
                info!("🔥 Hotspot darurat terdeteksi — masuk Emergency Mode");
                *state = NetworkState::Emergency;

                app_handle.emit("emergency_mode_active", serde_json::json!({
                    "message": "Mode Darurat aktif via hotspot",
                    "hotspotSubnet": "192.168.137",
                    "timestamp": unix_now(),
                })).ok();
            }

            // Tidak ada perubahan state — emit status periodik setiap ~30 detik (10 * 3s)
            _ => {
                status_tick += 1;
                if status_tick >= 10 {
                    status_tick = 0;
                    debug!("Network status: {:?}, has_network={}, has_hotspot={}",
                        *state, currently_has_network, currently_has_hotspot);

                    app_handle.emit("network_status", serde_json::json!({
                        "state": format!("{:?}", *state),
                        "hasNetwork": currently_has_network,
                        "hasHotspot": currently_has_hotspot,
                        "timestamp": unix_now(),
                    })).ok();
                }
            }
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
