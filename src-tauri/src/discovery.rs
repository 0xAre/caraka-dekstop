// src-tauri/src/discovery.rs
// Fase 4 — UDP Broadcast Peer Discovery (FIXED v2)
//
// Perbaikan dari versi sebelumnya:
//   [FIX #1] Ganti std::net::UdpSocket (blocking) → tokio::net::UdpSocket (async)
//            di start_listener agar tidak mem-block tokio thread pool.
//   [FIX #2] Ganti std::net::UdpSocket (blocking) → tokio::net::UdpSocket (async)
//            di start_broadcaster.
//   [FIX #3] build_beacon sekarang async, pakai .lock().await bukan try_lock()
//            agar beacon tidak di-skip saat state sedang dipegang thread lain.
//   [FIX #4] get_local_broadcast_addrs: perbaiki block kosong — interface dengan
//            broadcast 255.255.255.255 sekarang di-skip (bukan fallback global).
//   [FIX #5] Peer langsung di-upsert ke database saat beacon diterima,
//            tanpa menunggu TCP handshake selesai.
//   [FIX interval] Beacon interval diperkecil: 30s → 5s untuk responsivitas lebih baik.
//
// Protokol:
//   - Setiap node broadcast JSON beacon via UDP <subnet>.255:7770 setiap 5 detik
//   - Node lain yang mendengar beacon akan mencoba connect TCP ke port 7771
//   - Beacon berisi: node_id_hex, display_name, tcp_port

use std::net::{SocketAddr, Ipv4Addr};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{Manager, Emitter};
use tokio::net::UdpSocket;  // [FIX #1 & #2] tokio async UdpSocket
use tokio::sync::Mutex;
use tracing::{info, warn, debug};

use crate::state::AppState;  // Import di atas agar tersedia di seluruh modul

pub const DISCOVERY_PORT: u16 = 7770;
pub const BEACON_INTERVAL_SEC: u64 = 5; // [FIX interval] was 30

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

// ─── Subnet Broadcast Helper ───────────────────────────────────────────────

/// Dapatkan daftar alamat broadcast spesifik subnet dari semua interface aktif.
///
/// Menggantikan pengiriman ke 255.255.255.255 yang sering diblokir oleh
/// Windows network stack, terutama pada koneksi WiFi.
///
/// [FIX #4]: Interface yang menghasilkan broadcast 255.255.255.255 (subnet /0)
/// di-skip karena itu artinya netmask-nya tidak valid atau interface tidak benar-benar aktif.
/// 255.255.255.255 ditambahkan sebagai fallback TERPISAH di akhir.
///
/// Contoh output: ["192.168.1.255:7770", "10.0.0.255:7770"]
fn get_local_broadcast_addrs() -> Vec<SocketAddr> {
    let ifaces = match get_if_addrs::get_if_addrs() {
        Ok(i) => i,
        Err(e) => {
            warn!("Gagal mendapatkan interface jaringan: {}", e);
            return vec![SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT))];
        }
    };

    let mut addrs: Vec<SocketAddr> = ifaces
        .iter()
        .filter(|iface| !iface.is_loopback())
        .filter_map(|iface| {
            if let get_if_addrs::IfAddr::V4(ref v4) = iface.addr {
                let ip = u32::from(v4.ip);
                let mask = u32::from(v4.netmask);

                // [FIX #4] Skip interface dengan netmask 0 (tidak valid)
                if mask == 0 {
                    debug!("Interface {} di-skip: netmask 0", iface.name);
                    return None;
                }

                // Broadcast = ip | (~mask) — semua bit host diset ke 1
                let broadcast_u32 = ip | (!mask);
                let broadcast_ip = Ipv4Addr::from(broadcast_u32);

                // [FIX #4] Skip jika hasil broadcast adalah 255.255.255.255
                // (artinya mask = 0.0.0.0 — interface tidak dikonfigurasi benar)
                if broadcast_u32 == 0xFFFF_FFFF {
                    debug!("Interface {} menghasilkan 255.255.255.255 — di-skip, pakai fallback", iface.name);
                    return None;
                }

                // Skip loopback
                if broadcast_ip.is_loopback() {
                    return None;
                }

                // Skip link-local (169.254.x.x) — biasanya interface tidak aktif
                let octets = v4.ip.octets();
                if octets[0] == 169 && octets[1] == 254 {
                    debug!("Interface {} adalah link-local, di-skip", iface.name);
                    return None;
                }

                let addr = SocketAddr::from((broadcast_ip, DISCOVERY_PORT));
                debug!("Interface {}: IP={}, Mask={}, Broadcast={}",
                    iface.name, v4.ip, v4.netmask, broadcast_ip);
                Some(addr)
            } else {
                None // Skip IPv6
            }
        })
        .collect();

    if addrs.is_empty() {
        warn!("Tidak ada interface subnet aktif — fallback ke 255.255.255.255");
        addrs.push(SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT)));
    } else {
        // Tambahkan 255.255.255.255 sebagai fallback tambahan
        addrs.push(SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT)));
        info!("Interface aktif untuk broadcast: {:?}", addrs);
    }

    addrs
}

// ─── Broadcaster (kirim beacon) ────────────────────────────────────────────

/// Start beacon broadcaster — kirim UDP broadcast ke semua subnet aktif
/// setiap BEACON_INTERVAL_SEC detik.
///
/// [FIX #2] Menggunakan tokio::net::UdpSocket (async) — tidak memblokir runtime.
///
/// Berjalan sebagai background tokio task.
pub async fn start_broadcaster(app_handle: tauri::AppHandle) {
    info!("Discovery broadcaster dimulai (port {})", DISCOVERY_PORT);

    // [FIX #2] tokio UdpSocket — async, non-blocking
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
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
        // [FIX #3] build_beacon sekarang async
        if let Some(beacon_bytes) = build_beacon(&app_handle).await {
            let broadcast_targets = get_local_broadcast_addrs();

            if broadcast_targets.is_empty() {
                warn!("Tidak ada target broadcast — skip beacon cycle");
            } else {
                let mut success_count = 0usize;
                for target in &broadcast_targets {
                    // [FIX #2] .send_to(...).await — non-blocking
                    match socket.send_to(&beacon_bytes, target).await {
                        Ok(bytes) => {
                            debug!("Beacon terkirim ke {}: {} bytes", target, bytes);
                            success_count += 1;
                        }
                        Err(e) => warn!("Gagal kirim beacon ke {}: {}", target, e),
                    }
                }
                info!("Beacon dikirim ke {}/{} target", success_count, broadcast_targets.len());
            }
        }

        tokio::time::sleep(Duration::from_secs(BEACON_INTERVAL_SEC)).await;
    }
}

/// Build beacon bytes dari AppState. [FIX #3] Sekarang async, pakai .lock().await.
async fn build_beacon(app_handle: &tauri::AppHandle) -> Option<Vec<u8>> {
    use crate::state::AppState;

    let state_arc = app_handle.try_state::<Arc<Mutex<AppState>>>()?;

    // [FIX #3] Gunakan .lock().await — dijamin dapat lock, tidak di-skip
    let state = state_arc.lock().await;

    let beacon = hex::decode(&state.node_id_hex).ok().and_then(|bytes| {
        bytes.try_into().ok().map(|arr: [u8; 32]| {
            PeerBeacon::new(&arr, &state.display_name, crate::transport::DATA_PORT)
        })
    })?;

    Some(beacon.to_bytes())
}

// ─── Listener (terima beacon) ──────────────────────────────────────────────

/// Start UDP listener — terima beacon dari peer lain di LAN.
///
/// [FIX #1] Menggunakan tokio::net::UdpSocket (async) — tidak memblokir runtime.
///
/// Ketika beacon diterima:
///   1. Parse JSON beacon
///   2. Validasi protokol CARAKA
///   3. [FIX #5] Simpan peer langsung ke database (tidak menunggu TCP handshake)
///   4. Emit event "peer_discovered" ke frontend
///   5. Emit event ke transport untuk initiate TCP connection
pub async fn start_listener(app_handle: tauri::AppHandle) {
    let bind_addr = format!("0.0.0.0:{}", DISCOVERY_PORT);
    info!("Discovery listener dimulai di {}", bind_addr);

    // [FIX #1] tokio::net::UdpSocket — async, non-blocking
    let socket = match UdpSocket::bind(&bind_addr).await {
        Ok(s) => s,
        Err(e) => {
            warn!("Gagal bind UDP port {}: {} — Discovery tidak aktif", DISCOVERY_PORT, e);
            return;
        }
    };

    let mut buf = [0u8; 1024];

    loop {
        // [FIX #1] .recv_from().await — async, tidak memblokir tokio scheduler
        match socket.recv_from(&mut buf).await {
            Ok((len, src_addr)) => {
                let data = &buf[..len];

                if let Some(beacon) = PeerBeacon::from_bytes(data) {
                    if !beacon.is_valid() {
                        debug!("Beacon tidak valid dari {}", src_addr);
                        continue;
                    }

                    // Skip beacon dari diri sendiri
                    let is_self = {
                        if let Some(state_arc) = app_handle.try_state::<Arc<Mutex<AppState>>>() {
                            let state = state_arc.lock().await;
                            state.node_id_hex == beacon.node_id
                        } else {
                            false
                        }
                    };

                    if is_self {
                        debug!("Skip beacon dari diri sendiri ({})", src_addr);
                        continue;
                    }

                    info!(
                        "Peer ditemukan: {} ({}) di {}:{}",
                        beacon.display_name,
                        &beacon.node_id[..8],
                        src_addr.ip(),
                        beacon.tcp_port
                    );

                    // [FIX #5] Simpan peer langsung ke database dari discovery
                    // Tidak perlu menunggu TCP handshake — peer sudah bisa terlihat di UI
                    save_peer_from_beacon(&beacon, &src_addr, &app_handle).await;

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
                } else {
                    debug!("Terima {} bytes dari {} — bukan beacon CARAKA valid", len, src_addr);
                }
            }
            Err(e) => {
                warn!("Error pada UDP listener: {}", e);
                // Jangan exit — coba recover setelah jeda singkat
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

// ─── Helper: Simpan Peer dari Beacon ──────────────────────────────────────

/// [FIX #5] Simpan peer ke database segera saat beacon UDP diterima.
///
/// Trust score = 1.0 (rendah) sampai TCP handshake berhasil konfirmasi identitas.
/// Peer dengan trust 1.0 akan diupdate ke trust 2.0 setelah handshake di transport.rs.
async fn save_peer_from_beacon(
    beacon: &PeerBeacon,
    src_addr: &SocketAddr,
    app_handle: &tauri::AppHandle,
) {
    use crate::state::AppState;
    use crate::store::PeerRecord;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let peer = PeerRecord {
        node_id: beacon.node_id.clone(),
        display_name: beacon.display_name.clone(),
        last_seen: now,
        ip_address: src_addr.ip().to_string(),
        tcp_port: beacon.tcp_port,
        trust_score: 1.0, // Trust rendah sampai handshake konfirmasi
    };

    if let Some(state_arc) = app_handle.try_state::<Arc<Mutex<AppState>>>() {
        let state = state_arc.lock().await;
        // Ambil Arc<Mutex<Connection>> dulu, lalu drop state agar tidak borrow overlap
        let db_arc = state.db_conn.clone();
        drop(state);
        let db = db_arc.lock().await;
        match crate::store::upsert_peer(&db, &peer) {
            Ok(_) => debug!("Peer {} disimpan dari beacon discovery", &beacon.node_id[..8]),
            Err(e) => warn!("Gagal simpan peer dari beacon: {}", e),
        }
    }
}
