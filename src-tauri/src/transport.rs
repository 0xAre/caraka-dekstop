// src-tauri/src/transport.rs
// Fase 4 — Transport Layer (TCP LAN + Tor onion stream)
//
// Protokol framing:
//   [2 byte LE length] [data bytes]
//
// Setiap koneksi (TcpStream ataupun arti DataStream) dijaga sebagai
// background task lewat handle_connection yang generic atas
// AsyncRead + AsyncWrite. Ketika paket masuk, diproses melalui Router.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::{Manager, Emitter};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn, debug, error};

use crate::packet::{ClampPacket, PacketType};
use crate::routing::RoutingDecision;
use crate::keys::NodePublicKey;

/// Port TCP default — bisa di-override via env var CARAKA_TCP_PORT untuk multi-instance testing.
pub const DATA_PORT: u16 = 7771;

pub fn effective_data_port() -> u16 {
    std::env::var("CARAKA_TCP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DATA_PORT)
}
/// Maksimum ukuran paket yang diterima (64 KB)
const MAX_PACKET_SIZE: usize = 65535;

// ─── Connection Kind ───────────────────────────────────────────────────────

/// Asal koneksi — menentukan key sementara di peer_senders, isi event
/// frontend, dan alamat yang disimpan ke tabel peers setelah Hello.
#[derive(Clone, Debug)]
pub enum ConnKind {
    /// Koneksi TCP LAN biasa.
    Lan { addr: SocketAddr },
    /// Dial keluar ke onion peer — alamat onion tujuan diketahui.
    TorOutbound { onion: String },
    /// Stream masuk ke onion service kita — identitas peer anonim
    /// sampai Hello diterima.
    TorInbound { conn_id: String },
}

impl ConnKind {
    /// Key sementara di peer_senders sebelum node_id dari Hello diketahui.
    pub fn pre_hello_key(&self) -> String {
        match self {
            ConnKind::Lan { addr } => addr.to_string(),
            ConnKind::TorOutbound { onion } => format!("tor:{onion}"),
            ConnKind::TorInbound { conn_id } => format!("tor-in:{conn_id}"),
        }
    }

    /// Representasi host untuk event frontend (field "ip").
    pub fn display_host(&self) -> String {
        match self {
            ConnKind::Lan { addr } => addr.ip().to_string(),
            ConnKind::TorOutbound { onion } => onion.clone(),
            ConnKind::TorInbound { .. } => "tor".to_string(),
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            ConnKind::Lan { addr } => addr.port(),
            _ => crate::tor::TOR_VIRTUAL_PORT,
        }
    }
}

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
    let port = effective_data_port();
    let bind_addr = format!("0.0.0.0:{}", port);
    info!("TCP server dimulai di {}", bind_addr);

    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Gagal bind TCP port {}: {}", port, e);
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
                    handle_connection(stream, ConnKind::Lan { addr }, handle, senders).await;
                });
            }
            Err(e) => {
                warn!("Error pada TCP accept: {}", e);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

/// Handle satu koneksi peer — generic atas transport (TCP LAN / Tor stream).
pub async fn handle_connection<S>(
    mut stream: S,
    kind: ConnKind,
    app_handle: tauri::AppHandle,
    peer_senders: PeerSenders,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // Buat channel untuk mengirim data ke peer ini
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(64);
    let peer_key = kind.pre_hello_key();

    // Tunggu Hello packet untuk identifikasi node
    let peer_node_id = {
        match recv_packet_from_reader(&mut stream).await {
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
                warn!("Koneksi dari {} ditutup sebelum Hello", peer_key);
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
    if let Some(hello) = build_my_hello(&app_handle).await {
        let _ = send_raw_packet(&mut stream, &hello.encode()).await;
    }

    // Notify frontend peer connected
    app_handle.emit("peer_connected", serde_json::json!({
        "nodeId": peer_node_id,
        "ip": kind.display_host(),
        "port": kind.port()
    })).ok();

    // Split stream untuk read dan write secara concurrent
    let (reader, mut writer) = tokio::io::split(stream);
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
                process_incoming_packet(raw, &peer_node_id, &kind, &app_handle, &peer_senders).await;
            }
            Err(e) => {
                debug!("Koneksi dari {} terputus: {}", peer_key, e);
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

    info!("Koneksi dari {} ditutup", peer_key);
}

/// Bangun Hello packet dari state kita (node_id + display_name).
/// None jika vault belum unlock atau state belum siap.
async fn build_my_hello(app_handle: &tauri::AppHandle) -> Option<ClampPacket> {
    use crate::state::AppState;
    let state_arc = app_handle.try_state::<Arc<Mutex<Option<AppState>>>>()?;
    let state_opt = state_arc.lock().await;
    let st = state_opt.as_ref()?;
    let node_bytes = hex::decode(&st.node_id_hex).ok()?;
    let arr: [u8; 32] = node_bytes.try_into().ok()?;
    Some(ClampPacket::build_hello(&arr, &st.display_name))
}

// ─── TCP Client ────────────────────────────────────────────────────────────

/// Hubungi peer secara outbound (inisiator koneksi).
///
/// [FIX #6] Cek duplikat sebelumnya menggunakan format "ip:port" sebagai key,
/// padahal setelah handshake peer_senders menggunakan node_id_hex sebagai key.
/// Sekarang cek duplikat berdasarkan IP address saja (lebih robust).
///
/// Emit event feedback ke frontend:
///   - "peer_connecting" saat mulai
///   - "peer_connected" jika berhasil
///   - "peer_connect_failed" jika gagal atau timeout
pub async fn connect_to_peer(
    ip: &str,
    port: u16,
    node_id_hint: Option<&str>,
    app_handle: tauri::AppHandle,
    peer_senders: PeerSenders,
) {
    let addr = format!("{}:{}", ip, port);

    // [FIX #6] Cek duplikat berdasarkan node_id (jika diketahui dari beacon)
    // ATAU berdasarkan apakah ada sender dengan IP yang sama
    {
        let senders = peer_senders.lock().await;

        // Jika kita tahu node_id dari beacon, cek apakah sudah connected
        if let Some(nid) = node_id_hint {
            if senders.contains_key(nid) {
                debug!("Sudah terhubung ke node {} — skip connect ke {}", &nid[..8.min(nid.len())], addr);
                return;
            }
        }

        // Fallback: cek apakah ada koneksi dengan IP yang sama
        // (format key lama sebelum handshake adalah "ip:port")
        if senders.contains_key(&addr) {
            debug!("Sudah ada koneksi ke {} — skip", addr);
            return;
        }
    }

    info!("Mencoba connect ke peer: {}", addr);

    // Emit event "connecting" ke frontend sebelum mencoba
    app_handle.emit("peer_connecting", serde_json::json!({
        "ip": ip,
        "port": port,
        "nodeId": node_id_hint.unwrap_or(""),
    })).ok();

    // Connect dengan timeout 5 detik
    let connect_result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        TcpStream::connect(&addr),
    ).await;

    match connect_result {
        Ok(Ok(mut stream)) => {
            // Kirim Hello dulu
            if let Some(hello) = build_my_hello(&app_handle).await {
                let _ = send_raw_packet(&mut stream, &hello.encode()).await;
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
                handle_connection(stream, ConnKind::Lan { addr: peer_addr }, handle, senders).await;
            });
        }
        Ok(Err(e)) => {
            warn!("Gagal connect ke {}: {}", addr, e);
            app_handle.emit("peer_connect_failed", serde_json::json!({
                "ip": ip,
                "port": port,
                "reason": format!("Koneksi ditolak: {}", e),
            })).ok();
        }
        Err(_) => {
            warn!("Timeout connect ke {} (>5 detik)", addr);
            app_handle.emit("peer_connect_failed", serde_json::json!({
                "ip": ip,
                "port": port,
                "reason": "Timeout: peer tidak merespons dalam 5 detik",
            })).ok();
        }
    }
}

// ─── Tor Dial (F0) ─────────────────────────────────────────────────────────

/// Dial onion peer via Tor lalu jalankan pipeline handshake yang sama
/// dengan TCP. Retry dengan backoff — descriptor onion service baru
/// ter-publish beberapa puluh detik setelah status "ready", jadi dial
/// pertama sering gagal. Itu normal, bukan error fatal.
pub async fn connect_to_tor_peer(
    tor: Arc<crate::tor::TorContext>,
    onion: String,
    node_id_hint: Option<String>,
    app_handle: tauri::AppHandle,
    peer_senders: PeerSenders,
) {
    // Dedup: sudah terhubung ke node ini atau sedang dial onion yang sama
    {
        let senders = peer_senders.lock().await;
        if let Some(nid) = &node_id_hint {
            if senders.contains_key(nid) {
                debug!("Sudah terhubung ke node {} — skip dial Tor", &nid[..8.min(nid.len())]);
                return;
            }
        }
        if senders.contains_key(&format!("tor:{onion}")) {
            debug!("Sudah ada koneksi Tor ke {} — skip", onion);
            return;
        }
    }

    app_handle.emit("peer_connecting", serde_json::json!({
        "ip": onion,
        "port": crate::tor::TOR_VIRTUAL_PORT,
        "nodeId": node_id_hint.clone().unwrap_or_default(),
    })).ok();

    // Jeda sebelum tiap percobaan (detik). Total ~4 percobaan dalam ±50 detik.
    const RETRY_DELAYS: [u64; 4] = [0, 5, 15, 30];
    let mut last_err = String::new();

    for (attempt, delay) in RETRY_DELAYS.iter().enumerate() {
        if *delay > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(*delay)).await;
        }

        info!("Dial Tor ke {} (percobaan {}/{})", onion, attempt + 1, RETRY_DELAYS.len());

        let dial = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            tor.connect(&onion),
        ).await;

        match dial {
            Ok(Ok(mut stream)) => {
                info!("Terhubung ke {} via Tor", onion);

                if let Some(hello) = build_my_hello(&app_handle).await {
                    let _ = send_raw_packet(&mut stream, &hello.encode()).await;
                }

                let kind = ConnKind::TorOutbound { onion: onion.clone() };
                tokio::spawn(handle_connection(stream, kind, app_handle, peer_senders));
                return;
            }
            Ok(Err(e)) => last_err = e,
            Err(_) => last_err = "timeout 60 detik".to_string(),
        }

        warn!("Dial Tor ke {} gagal: {}", onion, last_err);
    }

    app_handle.emit("peer_connect_failed", serde_json::json!({
        "ip": onion,
        "port": crate::tor::TOR_VIRTUAL_PORT,
        "reason": format!(
            "Gagal dial via Tor setelah {} percobaan: {}. \
             Pastikan peer online dan Tor-nya sudah ready.",
            RETRY_DELAYS.len(), last_err
        ),
    })).ok();
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

/// Terima satu paket dari reader mana pun (TcpStream, DataStream, BufReader).
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
    kind: &ConnKind,
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

    // ── FITUR 4C: Epidemic Sync Packets ─────────────────────────────────────
    if pkt.header.packet_type == PacketType::SyncReq {
        handle_sync_req(&pkt, sender_key, app_handle, peer_senders).await;
        return;
    }
    if pkt.header.packet_type == PacketType::SyncResp {
        handle_sync_resp(&pkt, app_handle).await;
        return;
    }
    if pkt.header.packet_type == PacketType::SyncData {
        handle_sync_data(&pkt, app_handle).await;
        return;
    }

    // Hello packets — langsung proses (tidak melalui router)
    if pkt.header.packet_type == PacketType::Hello {
        let payload = serde_json::from_slice::<serde_json::Value>(&pkt.ciphertext)
            .unwrap_or_default();

        let node_id = payload["nodeId"].as_str().unwrap_or("");
        let display_name = payload["displayName"].as_str().unwrap_or("Unknown");

        debug!("Hello diterima dari node {}...", &node_id[..8.min(node_id.len())]);

        // Simpan peer ke database
        save_peer_from_hello(node_id, display_name, kind, app_handle).await;
        return;
    }

    // Broadcast packets — deliver ke app DAN relay, tanpa E2EE decrypt
    if pkt.header.packet_type == crate::packet::PacketType::Broadcast {
        // Cek duplikat via router packet cache, lalu register.
        // [FIX state-type] dulu pakai try_state::<Arc<Mutex<AppState>>> yang
        // selalu None sejak F1 — dedup broadcast tidak pernah jalan.
        let router = crate::state::with_state(app_handle, |st| st.router.clone()).await;

        if let Some(router) = &router {
            let mut r = router.lock().await;
            if r.is_duplicate(&pkt.header.packet_id) {
                debug!("Duplicate broadcast packet — di-drop");
                return;
            }
            r.register_broadcast(&pkt.header.packet_id);
        }

        // Parse payload (plaintext JSON)
        if let Ok(payload) = serde_json::from_slice::<crate::packet::BroadcastPayload>(&pkt.ciphertext) {
            // Kirim ke frontend
            app_handle.emit("broadcast_received", serde_json::json!({
                "senderId":   payload.sender_id,
                "senderName": payload.sender_name,
                "text":       payload.text,
                "timestamp":  payload.timestamp,
                "messageId":  payload.message_id,
                "hopCount":   pkt.hop_auth.hop_counter,
            })).ok();

            // Relay ke peer lain jika TTL masih tersisa
            if pkt.header.ttl > 0 {
                let mut relay_pkt = pkt.clone();
                relay_pkt.header.ttl -= 1;
                relay_pkt.hop_auth.hop_counter += 1;
                relay_packet(&relay_pkt, sender_key, peer_senders).await;
            }
        } else {
            debug!("Gagal parse BroadcastPayload dari packet");
        }
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
    // [FIX state-type] dulu try_state dengan tipe lama → selalu None → semua
    // paket DM masuk di-drop tanpa pernah sampai ke frontend.
    let decision = {
        if let Some(router) = crate::state::with_state(app_handle, |st| st.router.clone()).await {
            let mut r = router.lock().await;
            Some(r.handle_incoming(&mut pkt, &source))
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
    // Emit raw packet data ke frontend untuk didekripsi di commands layer.
    // TIDAK menyertakan ttl — pkt.header.ttl di titik ini sudah di-decrement oleh
    // Router::handle_incoming() (selalu, bahkan untuk pengiriman 1-hop langsung),
    // sementara AAD DM/File di commands.rs selalu direkonstruksi dengan TTL_MAX
    // konstan (sama seperti yang dipakai pengirim). Ttl dari wire TIDAK relevan
    // untuk dekripsi payload — ia murni metadata routing.
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

// ─── FITUR 4C: Epidemic Sync Handlers ─────────────────────────────────────

/// Proses SyncReq dari peer: kirim kembali fingerprint vector semua pesan yang kita punya.
async fn handle_sync_req(
    _pkt: &ClampPacket,
    sender_key: &str,
    app_handle: &tauri::AppHandle,
    peer_senders: &PeerSenders,
) {
    // Ambil db + node_id sekali dari state (helper aware Option<AppState>)
    let (db_arc, my_node_id_bytes) = match crate::state::with_state(
        app_handle,
        |st| (st.db_conn.clone(), st.my_node_id.0),
    ).await {
        Some(v) => v,
        None => return,
    };

    // Kumpulkan packet_id semua pesan yang tersimpan di DB
    let msg_fingerprints: Vec<String> = {
        let db = db_arc.lock().await;
        crate::store::get_all_message_ids(&db).unwrap_or_default()
    };

    let payload = serde_json::json!({ "fingerprints": msg_fingerprints }).to_string();
    let pkt_id = ClampPacket::generate_packet_id(&my_node_id_bytes);
    let resp_pkt = ClampPacket {
        header: crate::packet::ClampHeader {
            magic: crate::packet::MAGIC,
            version: crate::packet::PROTOCOL_VERSION,
            packet_type: PacketType::SyncResp,
            ttl: 1, // Sync tidak perlu di-relay
            packet_id: pkt_id,
        },
        hop_auth: crate::packet::HopAuth { hop_counter: 0, mac_tag: [0u8; 16] },
        nonce: [0u8; 16],
        ciphertext: payload.into_bytes(),
        aead_tag: [0u8; 16],
    };

    // Kirim hanya ke peer yang meminta
    let senders = peer_senders.lock().await;
    if let Some(tx) = senders.get(sender_key) {
        let _ = tx.try_send(resp_pkt.encode());
    }
}

/// Proses SyncResp: terima fingerprint list dari peer, minta data yang belum kita punya.
async fn handle_sync_resp(pkt: &ClampPacket, app_handle: &tauri::AppHandle) {
    let payload: serde_json::Value = match serde_json::from_slice(&pkt.ciphertext) {
        Ok(v) => v,
        Err(_) => return,
    };

    let peer_fingerprints: Vec<String> = payload["fingerprints"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    // Cari fingerprints yang tidak ada di DB kita
    let missing: Vec<String> = {
        if let Some(db_arc) = crate::state::with_state(app_handle, |st| st.db_conn.clone()).await {
            let db = db_arc.lock().await;
            let ours: std::collections::HashSet<String> =
                crate::store::get_all_message_ids(&db).unwrap_or_default().into_iter().collect();
            peer_fingerprints.into_iter().filter(|id| !ours.contains(id)).collect()
        } else { vec![] }
    };

    if missing.is_empty() {
        return;
    }

    // Emit event ke frontend agar bisa minta SyncData dari peer
    app_handle.emit("sync_missing_detected", serde_json::json!({
        "missingIds": missing,
    })).ok();
}

/// Proses SyncData: terima pesan yang diminta, simpan ke DB.
async fn handle_sync_data(pkt: &ClampPacket, app_handle: &tauri::AppHandle) {
    use crate::store::StoredMessage;

    let msg: StoredMessage = match bincode::deserialize(&pkt.ciphertext) {
        Ok(m) => m,
        Err(_) => return,
    };

    if let Some(db_arc) = crate::state::with_state(app_handle, |st| st.db_conn.clone()).await {
        let db = db_arc.lock().await;
        let _ = crate::store::save_message(&db, &msg);
    }

    debug!("SyncData: pesan {} disimpan dari epidemic sync", &msg.packet_id[..8.min(msg.packet_id.len())]);
}

/// Helper untuk simpan peer ke database setelah Hello.
///
/// Alamat yang disimpan tergantung jenis koneksi:
///   - Lan         → ip + port asli
///   - TorOutbound → onion address (ip dikosongkan, tidak menimpa data LAN lama)
///   - TorInbound  → identitas jaringan anonim; hanya update nama + last_seen
async fn save_peer_from_hello(
    node_id: &str,
    display_name: &str,
    kind: &ConnKind,
    app_handle: &tauri::AppHandle,
) {
    use crate::store::PeerRecord;

    let (ip, port, onion) = match kind {
        ConnKind::Lan { addr } => (addr.ip().to_string(), addr.port(), String::new()),
        ConnKind::TorOutbound { onion } => {
            (String::new(), crate::tor::TOR_VIRTUAL_PORT, onion.clone())
        }
        ConnKind::TorInbound { .. } => {
            (String::new(), crate::tor::TOR_VIRTUAL_PORT, String::new())
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let peer = PeerRecord {
        node_id: node_id.to_string(),
        display_name: display_name.to_string(),
        last_seen: now,
        ip_address: ip,
        tcp_port: port,
        onion_address: onion,
        trust_score: 2.0,
    };

    if let Some(db_arc) = crate::state::with_state(app_handle, |st| st.db_conn.clone()).await {
        let db = db_arc.lock().await;
        let _ = crate::store::upsert_peer(&db, &peer);
    }

    // Notify frontend
    app_handle.emit("peer_handshaked", serde_json::json!({
        "nodeId": node_id,
        "displayName": display_name,
        "ip": kind.display_host(),
        "port": kind.port(),
    })).ok();
}
