// src-tauri/src/discovery.rs
// Fase 4 — UDP Broadcast Peer Discovery
//
// Protokol:
//   - Setiap node broadcast JSON beacon via UDP 255.255.255.255:7770 setiap 30 detik
//   - Node lain yang mendengar beacon akan mencoba connect TCP ke port 7771
//   - Beacon berisi: node_id_hex, display_name, tcp_port

use std::net::{UdpSocket, SocketAddr};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{Manager, Emitter};
use tracing::{info, warn, debug};

pub const DISCOVERY_PORT: u16 = 7770;
pub const BEACON_INTERVAL_SEC: u64 = 30;

// ─── Beacon Format ─────────────────────────────────────────────────────────

/// Pesan beacon yang dikirim via UDP broadcast
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PeerBeacon {
    /// Magic string untuk identifikasi protokol CARAKA
    pub protocol: String,
    /// Node ID pengirim (32 byte hex = 64 karakter)
    pub node_id: String,
    /// Nama tampilan yang dikonfigurasi user
    pub display_name: String,
    /// Port TCP yang digunakan untuk terima koneksi data
    pub tcp_port: u16,
    /// Version protokol
    pub version: u8,
}

impl PeerBeacon {
    pub fn new(node_id: &[u8; 32], display_name: &str, tcp_port: u16) -> Self {
        PeerBeacon {
            protocol: "CARAKA-v1".to_string(),
            node_id: hex::encode(node_id),
            display_name: display_name.to_string(),
            tcp_port,
            version: 1,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }

    pub fn is_valid(&self) -> bool {
        self.protocol == "CARAKA-v1"
            && self.node_id.len() == 64  // 32 bytes hex = 64 chars
            && self.version == 1
    }
}

// ─── Broadcaster (kirim beacon) ────────────────────────────────────────────

/// Start beacon broadcaster — kirim UDP broadcast setiap 30 detik.
///
/// Berjalan sebagai background tokio task.
pub async fn start_broadcaster(app_handle: tauri::AppHandle) {
    info!("Discovery broadcaster dimulai di port {}", DISCOVERY_PORT);

    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            warn!("Gagal buat UDP socket untuk broadcaster: {}", e);
            return;
        }
    };

    if let Err(e) = socket.set_broadcast(true) {
        warn!("Gagal enable broadcast: {}", e);
        return;
    }

    loop {
        if let Some(beacon) = build_beacon(&app_handle) {
            let target: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT)
                .parse()
                .unwrap();

            match socket.send_to(&beacon, target) {
                Ok(bytes) => debug!("Beacon terkirim: {} bytes", bytes),
                Err(e) => warn!("Gagal kirim beacon: {}", e),
            }
        }

        tokio::time::sleep(Duration::from_secs(BEACON_INTERVAL_SEC)).await;
    }
}

/// Build beacon bytes dari AppState.
fn build_beacon(app_handle: &tauri::AppHandle) -> Option<Vec<u8>> {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use crate::state::AppState;

    let state_arc = app_handle.try_state::<Arc<Mutex<AppState>>>()?;
    // Buat beacon sementara tanpa block — kita pakai data yang di-cache di AppState
    // untuk menghindari deadlock saat tokio::block_in_place
    // Beacon data disimpan sebagai plain fields di AppState untuk akses non-async
    let node_id_hex = {
        match state_arc.try_lock() {
            Ok(state) => hex::decode(&state.node_id_hex).ok().and_then(|bytes| {
                bytes.try_into().ok().map(|arr: [u8; 32]| {
                    PeerBeacon::new(&arr, &state.display_name, crate::transport::DATA_PORT)
                })
            }),
            Err(_) => None,
        }
    };

    node_id_hex.map(|beacon| beacon.to_bytes())
}

// ─── Listener (terima beacon) ──────────────────────────────────────────────

/// Start UDP listener — terima beacon dari peer lain di LAN.
///
/// Ketika beacon diterima:
///   1. Parse JSON beacon
///   2. Validasi protokol CARAKA
///   3. Emit event "peer_discovered" ke frontend
///   4. Emit event ke transport untuk initiate TCP connection
pub async fn start_listener(app_handle: tauri::AppHandle) {
    let bind_addr = format!("0.0.0.0:{}", DISCOVERY_PORT);
    info!("Discovery listener dimulai di {}", bind_addr);

    let socket = match UdpSocket::bind(&bind_addr) {
        Ok(s) => s,
        Err(e) => {
            warn!("Gagal bind UDP port {}: {} — Discovery tidak aktif", DISCOVERY_PORT, e);
            // Jika port sudah dipakai (mungkin instance lain), lanjut tanpa listener
            return;
        }
    };

    // Set timeout agar loop bisa dicek secara periodik
    socket.set_read_timeout(Some(Duration::from_secs(5))).ok();

    let mut buf = [0u8; 1024];

    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, src_addr)) => {
                let data = &buf[..len];

                if let Some(beacon) = PeerBeacon::from_bytes(data) {
                    if !beacon.is_valid() {
                        debug!("Beacon tidak valid dari {}", src_addr);
                        continue;
                    }

                    // Skip beacon dari diri sendiri
                    let is_self = {
                        use std::sync::Arc;
                        use tokio::sync::Mutex;
                        use crate::state::AppState;

                        if let Some(state_arc) = app_handle.try_state::<Arc<Mutex<AppState>>>() {
                            if let Ok(state) = state_arc.try_lock() {
                                state.node_id_hex == beacon.node_id
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };

                    if is_self {
                        debug!("Skip beacon dari diri sendiri");
                        continue;
                    }

                    info!(
                        "Peer ditemukan: {} ({}) di {}:{}",
                        beacon.display_name,
                        &beacon.node_id[..8],
                        src_addr.ip(),
                        beacon.tcp_port
                    );

                    // Emit event ke frontend
                    let peer_info = serde_json::json!({
                        "nodeId": beacon.node_id,
                        "displayName": beacon.display_name,
                        "ip": src_addr.ip().to_string(),
                        "port": beacon.tcp_port,
                        "source": "udp_discovery"
                    });

                    app_handle.emit("peer_discovered", peer_info).ok();

                    // Trigger TCP connection attempt via event
                    app_handle.emit("connect_to_peer", serde_json::json!({
                        "ip": src_addr.ip().to_string(),
                        "port": beacon.tcp_port,
                        "nodeId": beacon.node_id
                    })).ok();
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                   || e.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout normal — lanjutkan loop
                continue;
            }
            Err(e) => {
                warn!("Error pada UDP listener: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
