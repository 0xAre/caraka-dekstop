// src-tauri/src/transport.rs
// Fase 4 — TCP Transport Layer
//
// Protokol framing:
//   [2 byte LE length] [data bytes]
//
// Setiap TCP connection dijaga sebagai background task.
// Ketika paket masuk, diproses melalui Router.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::{Manager, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn, debug, error};

use crate::packet::{ClampPacket, PacketType};
use crate::routing::RoutingDecision;
use crate::keys::NodePublicKey;

pub const DATA_PORT: u16 = 7771;
/// Maksimum ukuran paket yang diterima (64 KB)
const MAX_PACKET_SIZE: usize = 65535;

// ─── Connection Manager ────────────────────────────────────────────────────

/// Sender channel ke setiap peer yang terhubung.
/// Key = node_id_hex atau ip:port (sebelum handshake selesai)
pub type PeerSenders = Arc<Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>>;

// ─── TCP Server ────────────────────────────────────────────────────────────

/// Start TCP server — listen untuk koneksi masuk dari peer.
pub async fn start_tcp_server(
    app_handle: tauri::AppHandle,
    peer_senders: PeerSenders,
) {
    let bind_addr = format!("0.0.0.0:{}", DATA_PORT);
    info!("TCP server dimulai di {}", bind_addr);

    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Gagal bind TCP port {}: {}", DATA_PORT, e);
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                info!("Koneksi masuk dari: {}", addr);
                let handle = app_handle.clone();
                let senders = peer_senders.clone();
                tokio::spawn(async move {
                    handle_inbound_connection(stream, addr, handle, senders).await;
                });
            }
            Err(e) => {
                warn!("Error pada TCP accept: {}", e);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

/// Handle koneksi masuk dari peer.
async fn handle_inbound_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    app_handle: tauri::AppHandle,
    peer_senders: PeerSenders,
) {
    // Buat channel untuk mengirim data ke peer ini
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(64);
    let peer_key = addr.to_string();

    // Tunggu Hello packet untuk identifikasi node
    let peer_node_id = {
        match recv_packet(&mut stream).await {
            Ok(raw) => {
                match ClampPacket::decode(&raw) {
                    Ok(pkt) if pkt.header.packet_type == PacketType::Hello => {
                        // Parse node ID dari payload Hello (JSON)
                        let node_id_hex = String::from_utf8_lossy(&pkt.ciphertext)
                            .lines()
                            .next()
                            .and_then(|s| {
                                serde_json::from_str::<serde_json::Value>(s).ok()
                            })
                            .and_then(|v| v["nodeId"].as_str().map(|s| s.to_string()));

                        if let Some(nid) = node_id_hex {
                            info!("Handshake dari node: {}...", &nid[..8.min(nid.len())]);
                            nid
                        } else {
                            peer_key.clone()
                        }
                    }
                    _ => peer_key.clone(),
                }
            }
            Err(_) => {
                warn!("Koneksi dari {} ditutup sebelum Hello", addr);
                return;
            }
        }
    };

    // Simpan sender ke PeerSenders map
    {
        let mut senders = peer_senders.lock().await;
        senders.insert(peer_node_id.clone(), tx);
    }

    // Kirim Hello kita sebagai balasan
    {
        use crate::state::AppState;
        if let Some(state_arc) = app_handle.try_state::<Arc<Mutex<AppState>>>() {
            if let Ok(state) = state_arc.try_lock() {
                if let Ok(node_bytes) = hex::decode(&state.node_id_hex) {
                    if let Ok(arr) = node_bytes.try_into() {
                        let hello = ClampPacket::build_hello(&arr, &state.display_name);
                        let _ = send_raw_packet(&mut stream, &hello.encode()).await;
                    }
                }
            }
        }
    }

    // Notify frontend peer connected
    app_handle.emit("peer_connected", serde_json::json!({
        "nodeId": peer_node_id,
        "ip": addr.ip().to_string(),
        "port": addr.port()
    })).ok();

    // Split stream untuk read dan write secara concurrent
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader);

    // Spawn writer task
    tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            if send_raw_packet(&mut writer, &data).await.is_err() {
                break;
            }
        }
    });

    // Reader loop — proses paket masuk
    let peer_id_for_cleanup = peer_node_id.clone();
    let senders_for_cleanup = peer_senders.clone();

    loop {
        match recv_packet_from_reader(&mut reader).await {
            Ok(raw) => {
                process_incoming_packet(raw, &peer_node_id, &app_handle, &peer_senders).await;
            }
            Err(e) => {
                debug!("Koneksi dari {} terputus: {}", addr, e);
                break;
            }
        }
    }

    // Cleanup
    {
        let mut senders = senders_for_cleanup.lock().await;
        senders.remove(&peer_id_for_cleanup);
    }

    app_handle.emit("peer_disconnected", serde_json::json!({
        "nodeId": peer_id_for_cleanup
    })).ok();

    info!("Koneksi dari {} ditutup", addr);
}

// ─── TCP Client ────────────────────────────────────────────────────────────

/// Hubungi peer secara outbound (inisiator koneksi).
pub async fn connect_to_peer(
    ip: &str,
    port: u16,
    app_handle: tauri::AppHandle,
    peer_senders: PeerSenders,
) {
    let addr = format!("{}:{}", ip, port);

    // Cek apakah sudah terhubung
    {
        let senders = peer_senders.lock().await;
        if senders.contains_key(&addr) {
            debug!("Sudah terhubung ke {}", addr);
            return;
        }
    }

    info!("Mencoba connect ke peer: {}", addr);

    match TcpStream::connect(&addr).await {
        Ok(mut stream) => {
            // Kirim Hello dulu
            {
                use crate::state::AppState;
                if let Some(state_arc) = app_handle.try_state::<Arc<Mutex<AppState>>>() {
                    if let Ok(state) = state_arc.try_lock() {
                        if let Ok(node_bytes) = hex::decode(&state.node_id_hex) {
                            if let Ok(arr) = node_bytes.try_into() {
                                let hello = ClampPacket::build_hello(&arr, &state.display_name);
                                let _ = send_raw_packet(&mut stream, &hello.encode()).await;
                            }
                        }
                    }
                }
            }

            // Spawn handler untuk koneksi ini
            let handle = app_handle.clone();
            let senders = peer_senders.clone();
            let addr_clone = addr.clone();

            tokio::spawn(async move {
                let peer_addr: SocketAddr = match addr_clone.parse() {
                    Ok(a) => a,
                    Err(_) => return,
                };
                handle_inbound_connection(stream, peer_addr, handle, senders).await;
            });
        }
        Err(e) => {
            warn!("Gagal connect ke {}: {}", addr, e);
        }
    }
}

// ─── Packet I/O ────────────────────────────────────────────────────────────

/// Kirim bytes dengan 2-byte length prefix framing.
pub async fn send_raw_packet<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    data: &[u8],
) -> anyhow::Result<()> {
    if data.len() > MAX_PACKET_SIZE {
        return Err(anyhow::anyhow!("Packet terlalu besar: {} bytes", data.len()));
    }
    let len = data.len() as u16;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}

/// Terima satu paket dari TcpStream (blocking-style untuk non-split stream).
pub async fn recv_packet(stream: &mut TcpStream) -> anyhow::Result<Vec<u8>> {
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_le_bytes(len_buf) as usize;

    if len == 0 || len > MAX_PACKET_SIZE {
        return Err(anyhow::anyhow!("Panjang paket tidak valid: {}", len));
    }

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Terima satu paket dari BufReader.
async fn recv_packet_from_reader<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> anyhow::Result<Vec<u8>> {
    let mut len_buf = [0u8; 2];
    reader.read_exact(&mut len_buf).await?;
    let len = u16::from_le_bytes(len_buf) as usize;

    if len == 0 || len > MAX_PACKET_SIZE {
        return Err(anyhow::anyhow!("Panjang paket tidak valid: {}", len));
    }

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

// ─── Packet Processing ─────────────────────────────────────────────────────

/// Proses paket yang masuk dari peer.
///
/// Pipeline:
///   1. Decode CLAMP packet
///   2. Cari NodePublicKey dari peer (default jika belum dikenal)
///   3. Panggil router.handle_incoming()
///   4. Berdasarkan RoutingDecision:
///      - DeliverToApp → emit ke frontend
///      - Relay → broadcast ke semua peer lain
///      - DeliverAndRelay → keduanya
///      - Drop → abaikan
async fn process_incoming_packet(
    raw: Vec<u8>,
    sender_key: &str,
    app_handle: &tauri::AppHandle,
    peer_senders: &PeerSenders,
) {
    let mut pkt = match ClampPacket::decode(&raw) {
        Ok(p) => p,
        Err(e) => {
            debug!("Gagal decode paket dari {}: {}", sender_key, e);
            return;
        }
    };

    // Hello packets — langsung proses (tidak melalui router)
    if pkt.header.packet_type == PacketType::Hello {
        let payload = serde_json::from_slice::<serde_json::Value>(&pkt.ciphertext)
            .unwrap_or_default();

        let node_id = payload["nodeId"].as_str().unwrap_or("");
        let display_name = payload["displayName"].as_str().unwrap_or("Unknown");

        debug!("Hello diterima dari node {}...", &node_id[..8.min(node_id.len())]);

        // Simpan peer ke database
        save_peer_from_hello(node_id, display_name, sender_key, app_handle).await;
        return;
    }

    // Dapatkan source NodePublicKey
    let source_node_id = {
        if let Ok(bytes) = hex::decode(sender_key) {
            bytes.try_into().ok().map(NodePublicKey)
        } else {
            // Gunakan packet_id prefix sebagai proxy identitas (sebelum handshake)
            let mut proxy = [0u8; 32];
            proxy[0..8].copy_from_slice(&pkt.header.packet_id);
            Some(NodePublicKey(proxy))
        }
    };

    let source = match source_node_id {
        Some(id) => id,
        None => return,
    };

    // Proses melalui router
    use crate::state::AppState;
    let decision = {
        if let Some(state_arc) = app_handle.try_state::<Arc<Mutex<AppState>>>() {
            if let Ok(state) = state_arc.try_lock() {
                let mut router = state.router.try_lock().ok();
                if let Some(ref mut r) = router {
                    Some(r.handle_incoming(&mut pkt, &source))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };

    match decision {
        Some(RoutingDecision::DeliverToApp) => {
            deliver_to_app(&pkt, app_handle).await;
        }
        Some(RoutingDecision::Relay) => {
            relay_packet(&pkt, sender_key, peer_senders).await;
        }
        Some(RoutingDecision::DeliverAndRelay) => {
            deliver_to_app(&pkt, app_handle).await;
            relay_packet(&pkt, sender_key, peer_senders).await;
        }
        Some(RoutingDecision::Drop(reason)) => {
            debug!("Paket di-drop: {:?}", reason);
        }
        None => {}
    }
}

/// Kirim event ke frontend (Tauri IPC) untuk menampilkan pesan.
async fn deliver_to_app(pkt: &ClampPacket, app_handle: &tauri::AppHandle) {
    // Emit raw packet data ke frontend untuk didekripsi di commands layer
    let payload = serde_json::json!({
        "packetId": hex::encode(pkt.header.packet_id),
        "packetType": pkt.header.packet_type as u8,
        "nonce": hex::encode(pkt.nonce),
        "ciphertext": hex::encode(&pkt.ciphertext),
        "aeadTag": hex::encode(pkt.aead_tag),
        "hopCounter": pkt.hop_auth.hop_counter,
    });

    app_handle.emit("clamp_packet_received", payload).ok();
}

/// Forward paket ke semua peer kecuali yang mengirim.
async fn relay_packet(
    pkt: &ClampPacket,
    sender_key: &str,
    peer_senders: &PeerSenders,
) {
    let encoded = pkt.encode();
    let senders = peer_senders.lock().await;

    for (peer_id, sender) in senders.iter() {
        if peer_id != sender_key {
            if let Err(e) = sender.try_send(encoded.clone()) {
                debug!("Gagal relay ke {}: {}", peer_id, e);
            }
        }
    }
}

/// Broadcast paket ke SEMUA peer (untuk mengirim pesan kita sendiri).
pub async fn broadcast_packet(
    pkt: &ClampPacket,
    peer_senders: &PeerSenders,
) {
    let encoded = pkt.encode();
    let senders = peer_senders.lock().await;

    for (peer_id, sender) in senders.iter() {
        if let Err(e) = sender.try_send(encoded.clone()) {
            debug!("Gagal broadcast ke {}: {}", peer_id, e);
        }
    }

    info!("Paket di-broadcast ke {} peer", senders.len());
}

/// Helper untuk simpan peer ke database setelah Hello.
async fn save_peer_from_hello(
    node_id: &str,
    display_name: &str,
    addr_key: &str,
    app_handle: &tauri::AppHandle,
) {
    use crate::state::AppState;
    use crate::store::PeerRecord;

    let ip = addr_key.split(':').next().unwrap_or("").to_string();
    let port: u16 = addr_key
        .split(':')
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(DATA_PORT);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let peer = PeerRecord {
        node_id: node_id.to_string(),
        display_name: display_name.to_string(),
        last_seen: now,
        ip_address: ip,
        tcp_port: port,
        trust_score: 2.0,
    };

    if let Some(state_arc) = app_handle.try_state::<Arc<Mutex<AppState>>>() {
        if let Ok(state) = state_arc.try_lock() {
            if let Ok(db) = state.db_conn.try_lock() {
                let _ = crate::store::upsert_peer(&db, &peer);
            }
        }
    }

    // Notify frontend
    app_handle.emit("peer_handshaked", serde_json::json!({
        "nodeId": node_id,
        "displayName": display_name,
    })).ok();
}
