// src-tauri/src/commands.rs
// Fase 6 — Tauri IPC Command Handlers
//
// Semua fungsi di sini dipanggil dari frontend via invoke('command_name', args).
// ATURAN KEAMANAN:
//   - Tidak boleh return private key atau plaintext ke frontend dalam bentuk raw bytes
//   - Semua error harus di-handle, tidak boleh panic!

use std::sync::Arc;
use tauri::{State, Emitter};
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use base64::Engine as _;

use crate::state::AppState;
use crate::packet::{ClampPacket, ClampHeader, HopAuth, PacketType, MAGIC, PROTOCOL_VERSION, TTL_MAX};
use crate::store::StoredMessage;


// ─── Response Types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub node_id: String,
    pub fingerprint: String,
    pub display_name: String,
    pub tcp_port: u16,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageInfo {
    pub id: String,
    pub sender_id: String,
    pub sender_fingerprint: String,
    pub recipient_id: String,
    pub plaintext: String,
    pub timestamp: i64,
    pub is_outgoing: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub node_id: String,
    pub display_name: String,
    pub last_seen: i64,
    pub ip_address: String,
    pub tcp_port: u16,
    pub trust_score: f64,
    pub is_online: bool,
    pub fingerprint: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    pub connected_peers: usize,
    pub known_peers: usize,
    pub my_node_id: String,
    pub my_fingerprint: String,
}

// ─── IPC Commands ──────────────────────────────────────────────────────────

/// Inisialisasi node — dipanggil frontend setelah node_ready event.
/// Return: NodeInfo berisi node_id dan fingerprint.
#[tauri::command]
pub async fn init_node(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<NodeInfo, String> {
    let state = state.lock().await;
    let fingerprint = crate::keys::fingerprint(&state.my_node_id);

    Ok(NodeInfo {
        node_id: state.node_id_hex.clone(),
        fingerprint,
        display_name: state.display_name.clone(),
        tcp_port: crate::transport::DATA_PORT,
    })
}

/// Update display name node ini.
#[tauri::command]
pub async fn set_display_name(
    name: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut state = state.lock().await;
    if name.trim().is_empty() || name.len() > 50 {
        return Err("Display name harus 1–50 karakter".to_string());
    }
    let trimmed = name.trim().to_string();

    // Persist ke database
    {
        let db = state.db_conn.lock().await;
        crate::store::save_setting(&db, "display_name", &trimmed)
            .map_err(|e| format!("Gagal simpan setting: {}", e))?;
    }

    state.display_name = trimmed;
    Ok(())
}

/// Kirim Direct Message ke peer tertentu.
///
/// Pipeline:
///   1. Parse recipient Node ID dari hex string
///   2. ECDH untuk shared secret
///   3. Derive DM-Key menggunakan session_id + msg_counter
///   4. Enkripsi plaintext dengan Ascon-AEAD128
///   5. Build CLAMP packet
///   6. Simpan ke database (ciphertext only!)
///   7. Broadcast ke semua peer yang terhubung
#[tauri::command]
pub async fn send_dm(
    recipient_id: String,
    plaintext: String,
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    if plaintext.trim().is_empty() {
        return Err("Pesan tidak boleh kosong".to_string());
    }
    if plaintext.len() > 4096 {
        return Err("Pesan terlalu panjang (max 4096 karakter)".to_string());
    }

    let state = state.lock().await;

    // 1. Parse recipient Node ID
    let recipient_bytes = hex::decode(&recipient_id)
        .map_err(|_| "Recipient ID tidak valid (bukan hex string)".to_string())?;
    let peer_public: [u8; 32] = recipient_bytes
        .try_into()
        .map_err(|_| "Recipient ID harus 32 byte (64 hex chars)".to_string())?;
    let peer_id = crate::keys::NodePublicKey(peer_public);

    // 2. Ambil atau buat session info
    let (session_id, msg_counter) = {
        let db = state.db_conn.lock().await;
        crate::store::get_or_create_session(&db, &recipient_id)
            .map_err(|e| e.to_string())?
    };

    // 3. ECDH → shared secret
    let shared_secret = crate::keys::ecdh(&state.my_private_key, &peer_id);

    // 4. Derive DM-Key
    let aead_key = crate::keys::derive_dm_key(
        &shared_secret,
        &state.my_node_id,  // sender = kita
        &peer_id,           // receiver = mereka
        &session_id,
        msg_counter,
    );

    // 5. Generate Packet ID
    let packet_id = ClampPacket::generate_packet_id(&state.my_node_id.0);

    // 6. Build inner payload (JSON yang akan dienkripsi)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let inner = crate::packet::DmInnerPayload {
        sender_id: state.node_id_hex.clone(),
        recipient_id: recipient_id.clone(),
        text: plaintext.clone(),
        timestamp: now,
        session_id: hex::encode(session_id),
        msg_counter,
    };

    let inner_json = serde_json::to_vec(&inner)
        .map_err(|e| e.to_string())?;

    // 7. Build header untuk AAD
    let header = ClampHeader {
        magic: MAGIC,
        version: PROTOCOL_VERSION,
        packet_type: PacketType::Dm,
        ttl: TTL_MAX,
        packet_id,
    };

    let temp_packet = ClampPacket {
        header: header.clone(),
        hop_auth: HopAuth { hop_counter: 0, mac_tag: [0u8; 16] },
        nonce: [0u8; 16],
        ciphertext: vec![],
        aead_tag: [0u8; 16],
    };
    let aad = temp_packet.header_bytes();

    // 8. Enkripsi
    let nonce = crate::crypto::generate_nonce();
    let (ciphertext, aead_tag) = crate::crypto::encrypt(
        &aead_key,
        &nonce,
        &inner_json,
        &aad,
    ).map_err(|e| e.to_string())?;

    // 9. Hitung Hop-MAC (origin hop)
    let hop_mac = {
        let router = state.router.lock().await;
        router.compute_origin_mac(&ClampPacket {
            header: header.clone(),
            hop_auth: HopAuth { hop_counter: 0, mac_tag: [0u8; 16] },
            nonce: nonce.0,
            ciphertext: ciphertext.clone(),
            aead_tag,
        })
    };

    // 10. Build paket lengkap
    let full_packet = ClampPacket {
        header,
        hop_auth: HopAuth { hop_counter: 0, mac_tag: hop_mac },
        nonce: nonce.0,
        ciphertext: ciphertext.clone(),
        aead_tag,
    };

    // 11. Simpan ke database (ciphertext, BUKAN plaintext!)
    let msg_uuid = format!("{}", now * 1000 + msg_counter);
    let stored_msg = StoredMessage {
        id: msg_uuid.clone(),
        packet_id: hex::encode(packet_id),
        sender_id: state.node_id_hex.clone(),
        recipient_id: recipient_id.clone(),
        nonce: nonce.0.to_vec(),
        ciphertext: ciphertext.clone(),
        aead_tag: aead_tag.to_vec(),
        received_at: now as i64,
        delivered: false,
    };

    {
        let db = state.db_conn.lock().await;
        crate::store::save_message(&db, &stored_msg)
            .map_err(|e| e.to_string())?;
    }

    // 12. Increment message counter
    {
        let db = state.db_conn.lock().await;
        crate::store::increment_msg_counter(&db, &recipient_id)
            .map_err(|e| e.to_string())?;
    }

    // 13. Register di router (untuk replay protection)
    {
        let mut router = state.router.lock().await;
        router.register_outgoing(&full_packet);
    }

    // 14. Broadcast ke semua peer
    crate::transport::broadcast_packet(&full_packet, &state.peer_senders).await;

    // 15. Emit event ke UI agar pesan ditampilkan sebagai sent
    app_handle.emit("message_sent", serde_json::json!({
        "id": msg_uuid,
        "recipientId": recipient_id,
        "text": plaintext,
        "timestamp": now,
        "isOutgoing": true
    })).ok();

    Ok(hex::encode(packet_id))
}

/// Terima dan dekripsi pesan yang masuk (dipanggil dari event handler frontend).
///
/// Frontend memanggil ini setelah menerima event "clamp_packet_received".
#[tauri::command]
pub async fn try_decrypt_packet(
    packet_id: String,
    nonce_hex: String,
    ciphertext_hex: String,
    aead_tag_hex: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<MessageInfo>, String> {
    let state = state.lock().await;

    // Parse packet_id ke bytes untuk rekonstruksi AAD
    let packet_id_bytes = hex::decode(&packet_id)
        .map_err(|_| "Packet ID hex invalid".to_string())?;
    let packet_id_arr: [u8; 8] = packet_id_bytes
        .try_into()
        .map_err(|_| "Packet ID harus 8 byte".to_string())?;

    let nonce_bytes = hex::decode(&nonce_hex)
        .map_err(|_| "Nonce hex invalid".to_string())?;
    let ciphertext = hex::decode(&ciphertext_hex)
        .map_err(|_| "Ciphertext hex invalid".to_string())?;
    let aead_tag_bytes = hex::decode(&aead_tag_hex)
        .map_err(|_| "AEAD tag hex invalid".to_string())?;

    let nonce: [u8; 16] = nonce_bytes
        .try_into()
        .map_err(|_| "Nonce harus 16 byte".to_string())?;
    let aead_tag: [u8; 16] = aead_tag_bytes
        .try_into()
        .map_err(|_| "AEAD tag harus 16 byte".to_string())?;

    // BUG #1 FIX: Rekonstruksi AAD 13 byte yang sama seperti saat enkripsi.
    // Pengirim selalu gunakan TTL_MAX saat membuat header untuk enkripsi.
    // Format: magic(2B) + version(1B) + packet_type(1B) + ttl(1B) + packet_id(8B)
    let mut aad = [0u8; crate::packet::HEADER_SIZE];
    aad[0..2].copy_from_slice(&MAGIC);
    aad[2] = PROTOCOL_VERSION;
    aad[3] = PacketType::Dm as u8;
    aad[4] = TTL_MAX;
    aad[5..13].copy_from_slice(&packet_id_arr);

    // Coba dekripsi dengan setiap peer yang dikenal
    let peers = {
        let db = state.db_conn.lock().await;
        crate::store::get_all_peers(&db).map_err(|e| e.to_string())?
    };

    for peer in &peers {
        if let Ok(peer_bytes) = hex::decode(&peer.node_id) {
            if let Ok(arr) = peer_bytes.try_into() {
                let peer_pub = crate::keys::NodePublicKey(arr);
                let shared = crate::keys::ecdh(&state.my_private_key, &peer_pub);

                // BUG #3 FIX: Load session_id dari DB — BUKAN hardcode [0u8;8]
                let (session_id, stored_counter) = {
                    let db = state.db_conn.lock().await;
                    crate::store::get_or_create_session(&db, &peer.node_id)
                        .unwrap_or(([0u8; 8], 0))
                };

                // Coba beberapa counter di sekitar stored_counter untuk toleransi out-of-order
                let start = stored_counter.saturating_sub(5);
                let end = stored_counter + 50;

                for counter in start..end {
                    let aead_key = crate::keys::derive_dm_key(
                        &shared,
                        &peer_pub,           // sender = peer
                        &state.my_node_id,   // receiver = kita
                        &session_id,
                        counter,
                    );

                    let nonce_wrapper = crate::crypto::Nonce(nonce);

                    if let Ok(plaintext_bytes) = crate::crypto::decrypt(
                        &aead_key,
                        &nonce_wrapper,
                        &ciphertext,
                        &aead_tag,
                        &aad,
                    ) {
                        if let Ok(inner) = serde_json::from_slice::<crate::packet::DmInnerPayload>(&plaintext_bytes) {
                            let sender_bytes = hex::decode(&inner.sender_id).ok()
                                .and_then(|b| b.try_into().ok())
                                .map(|arr| crate::keys::NodePublicKey(arr));

                            let sender_fp = sender_bytes
                                .as_ref()
                                .map(crate::keys::fingerprint)
                                .unwrap_or_default();

                            // BUG #6 FIX: Simpan incoming message ke database
                            let now_ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64;

                            let stored_msg = StoredMessage {
                                id: format!("{}_{}", &inner.sender_id[..8.min(inner.sender_id.len())], inner.timestamp),
                                packet_id: packet_id.clone(),
                                sender_id: inner.sender_id.clone(),
                                recipient_id: inner.recipient_id.clone(),
                                nonce: nonce.to_vec(),
                                ciphertext: ciphertext.clone(),
                                aead_tag: aead_tag.to_vec(),
                                received_at: now_ts,
                                delivered: true,
                            };
                            {
                                let db = state.db_conn.lock().await;
                                let _ = crate::store::save_message(&db, &stored_msg);
                            }

                            return Ok(Some(MessageInfo {
                                id: packet_id,
                                sender_id: inner.sender_id,
                                sender_fingerprint: sender_fp,
                                recipient_id: inner.recipient_id,
                                plaintext: inner.text,
                                timestamp: inner.timestamp as i64,
                                is_outgoing: false,
                            }));
                        }
                    }
                }
            }
        }
    }

    // Tidak bisa dekripsi — mungkin bukan untuk node ini (relay saja)
    Ok(None)
}

/// Ambil daftar pesan dengan peer tertentu (dari database — ciphertext akan didekripsi).
#[tauri::command]
pub async fn get_messages(
    peer_id: String,
    limit: Option<usize>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<serde_json::Value>, String> {
    let state = state.lock().await;
    let limit = limit.unwrap_or(50).min(200);

    let messages = {
        let db = state.db_conn.lock().await;
        crate::store::get_messages_between(&db, &state.node_id_hex, &peer_id, limit)
            .map_err(|e| e.to_string())?
    };

    // Coba dekripsi setiap pesan
    let peer_bytes = hex::decode(&peer_id)
        .map_err(|_| "Peer ID tidak valid".to_string())?;
    let peer_pub_arr: [u8; 32] = peer_bytes
        .try_into()
        .map_err(|_| "Peer ID harus 32 byte".to_string())?;
    let peer_pub = crate::keys::NodePublicKey(peer_pub_arr);
    let shared = crate::keys::ecdh(&state.my_private_key, &peer_pub);

    let mut result = Vec::new();
    for msg in messages {
        let is_outgoing = msg.sender_id == state.node_id_hex;

        // Coba dekripsi
        if let (Ok(nonce_arr), Ok(tag_arr)) = (
            msg.nonce.clone().try_into().map_err(|_| ()),
            msg.aead_tag.clone().try_into().map_err(|_| ()),
        ) {
            let nonce_arr: [u8; 16] = nonce_arr;
            let tag_arr: [u8; 16] = tag_arr;

            // BUG #3 FIX: Load session_id dari DB
            let (session_id, stored_counter) = {
                let db = state.db_conn.lock().await;
                crate::store::get_or_create_session(&db, &peer_id)
                    .unwrap_or(([0u8; 8], 0))
            };

            let mut decrypted_text = None;

            // BUG #1 FIX: Rekonstruksi AAD yang benar
            // Outgoing: sender=my_node_id, receiver=peer_pub → packet_id from stored msg
            let msg_packet_id_arr: [u8; 8] = msg.packet_id
                .as_bytes()
                .chunks(2)
                .take(8)
                .filter_map(|ch| {
                    std::str::from_utf8(ch).ok()
                        .and_then(|s| u8::from_str_radix(s, 16).ok())
                })
                .collect::<Vec<u8>>()
                .try_into()
                .unwrap_or([0u8; 8]);

            let mut aad = [0u8; crate::packet::HEADER_SIZE];
            aad[0..2].copy_from_slice(&MAGIC);
            aad[2] = PROTOCOL_VERSION;
            aad[3] = PacketType::Dm as u8;
            aad[4] = TTL_MAX;
            aad[5..13].copy_from_slice(&msg_packet_id_arr);

            let start_counter = stored_counter.saturating_sub(5);
            let end_counter = stored_counter + 100;

            for counter in start_counter..end_counter {
                let aead_key = if is_outgoing {
                    crate::keys::derive_dm_key(
                        &shared, &state.my_node_id, &peer_pub,
                        &session_id, counter,
                    )
                } else {
                    crate::keys::derive_dm_key(
                        &shared, &peer_pub, &state.my_node_id,
                        &session_id, counter,
                    )
                };

                let nonce_w = crate::crypto::Nonce(nonce_arr);

                if let Ok(plain_bytes) = crate::crypto::decrypt(
                    &aead_key, &nonce_w, &msg.ciphertext, &tag_arr, &aad,
                ) {
                    if let Ok(inner) = serde_json::from_slice::<crate::packet::DmInnerPayload>(&plain_bytes) {
                        decrypted_text = Some((inner.text, inner.timestamp as i64));
                        break;
                    }
                }
            }

            if let Some((text, ts)) = decrypted_text {
                result.push(serde_json::json!({
                    "id": msg.id,
                    "senderId": msg.sender_id,
                    "recipientId": msg.recipient_id,
                    "text": text,
                    "timestamp": ts,
                    "isOutgoing": is_outgoing,
                    "decrypted": true
                }));
            } else {
                // Tampilkan pesan yang tidak bisa didekripsi sebagai [encrypted]
                result.push(serde_json::json!({
                    "id": msg.id,
                    "senderId": msg.sender_id,
                    "recipientId": msg.recipient_id,
                    "text": "[pesan terenkripsi]",
                    "timestamp": msg.received_at,
                    "isOutgoing": is_outgoing,
                    "decrypted": false
                }));
            }
        }
    }

    Ok(result)
}

/// Ambil daftar semua peer yang dikenal.
#[tauri::command]
pub async fn get_peers(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<PeerInfo>, String> {
    let state = state.lock().await;

    let db = state.db_conn.lock().await;
    let peers = crate::store::get_all_peers(&db)
        .map_err(|e| e.to_string())?;

    // Cek peers yang sedang online (ada di peer_senders)
    let online_ids: Vec<String> = {
        let senders = state.peer_senders.lock().await;
        senders.keys().cloned().collect()
    };

    let _now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let result: Vec<PeerInfo> = peers.iter().map(|p| {
        let is_online = online_ids.iter().any(|id| id == &p.node_id);
        let fingerprint = if let Ok(bytes) = hex::decode(&p.node_id) {
            if let Ok(arr) = bytes.try_into() {
                crate::keys::fingerprint(&crate::keys::NodePublicKey(arr))
            } else {
                "?".to_string()
            }
        } else {
            "?".to_string()
        };

        PeerInfo {
            node_id: p.node_id.clone(),
            display_name: p.display_name.clone(),
            last_seen: p.last_seen,
            ip_address: p.ip_address.clone(),
            tcp_port: p.tcp_port,
            trust_score: p.trust_score,
            is_online,
            fingerprint,
        }
    }).collect();

    Ok(result)
}

/// Tambah peer secara manual via IP:Port.
#[tauri::command]
pub async fn add_peer_manual(
    ip: String,
    port: Option<u16>,
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let port = port.unwrap_or(crate::transport::DATA_PORT);

    // Validasi IP sederhana
    if ip.trim().is_empty() {
        return Err("IP address tidak boleh kosong".to_string());
    }

    let state_guard = state.lock().await;
    let peer_senders = state_guard.peer_senders.clone();
    drop(state_guard);

    // Trigger koneksi TCP
    // node_id_hint = None karena manual connect tidak diketahui node_id-nya dulu
    let ip_clone = ip.clone();
    tokio::spawn(async move {
        crate::transport::connect_to_peer(&ip_clone, port, None, app_handle, peer_senders).await;
    });

    Ok(format!("Mencoba connect ke {}:{}", ip, port))
}

/// Get status jaringan (untuk status bar di UI).
#[tauri::command]
pub async fn get_network_status(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<NetworkStatus, String> {
    let state = state.lock().await;

    let connected = {
        let senders = state.peer_senders.lock().await;
        senders.len()
    };

    let known = {
        let db = state.db_conn.lock().await;
        crate::store::get_all_peers(&db)
            .map(|p| p.len())
            .unwrap_or(0)
    };

    let fingerprint = crate::keys::fingerprint(&state.my_node_id);

    Ok(NetworkStatus {
        connected_peers: connected,
        known_peers: known,
        my_node_id: state.node_id_hex[..8].to_string() + "...",
        my_fingerprint: fingerprint,
    })
}

/// Dapatkan IP lokal utama node ini.
///
/// Digunakan oleh frontend untuk tampilan di halaman Home dan generate QR Code.
/// Return: IP lokal sebagai string (contoh: "192.168.1.15") atau "Tidak diketahui".
#[tauri::command]
pub async fn get_local_ip() -> Result<String, String> {
    let ifaces = get_if_addrs::get_if_addrs()
        .map_err(|e| format!("Gagal baca interface: {}", e))?;

    // Cari IP lokal non-loopback pertama
    for iface in &ifaces {
        if iface.is_loopback() { continue; }
        if let get_if_addrs::IfAddr::V4(ref v4) = iface.addr {
            let ip = v4.ip.to_string();
            // Filter link-local (169.254.x.x)
            if !ip.starts_with("169.254") {
                return Ok(ip);
            }
        }
    }

    Ok("Tidak diketahui".to_string())
}

/// Kirim pesan Broadcast darurat ke semua peer di jaringan mesh.
///
/// Pesan ini TIDAK dienkripsi — ditujukan untuk keadaan darurat publik.
/// Akan diteruskan ke seluruh mesh hingga TTL = 0 (maks 5 lompatan).
#[tauri::command]
pub async fn send_broadcast(
    text: String,
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("Pesan tidak boleh kosong".to_string());
    }
    if text.len() > 500 {
        return Err("Pesan broadcast terlalu panjang (max 500 karakter)".to_string());
    }

    let state = state.lock().await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Generate UUID untuk deduplikasi
    let message_id = uuid::Uuid::new_v4().to_string();

    let payload = crate::packet::BroadcastPayload {
        sender_id: state.node_id_hex.clone(),
        sender_name: state.display_name.clone(),
        text: text.clone(),
        timestamp: now,
        message_id: message_id.clone(),
    };

    // Build paket Broadcast
    let pkt = crate::packet::ClampPacket::build_broadcast(&state.my_node_id.0, &payload);

    // Register di router agar tidak diproses ulang jika kembali ke kita
    {
        let mut router = state.router.lock().await;
        router.register_outgoing(&pkt);
    }

    // Kirim ke semua peer yang terhubung
    crate::transport::broadcast_packet(&pkt, &state.peer_senders).await;

    // Notify frontend bahwa pesan sudah terkirim
    app_handle.emit("broadcast_sent", serde_json::json!({
        "senderId":   state.node_id_hex,
        "senderName": state.display_name,
        "text":       text,
        "timestamp":  now,
        "messageId":  message_id,
    })).ok();

    Ok("Pesan broadcast terkirim".to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// EMERGENCY MODE COMMANDS — Komunikasi Saat Mati Lampu
// ═══════════════════════════════════════════════════════════════════════════

/// Response struct untuk status Emergency Mode.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmergencyStatus {
    /// State jaringan: "Normal" | "Lost" | "Emergency"
    pub network_state: String,
    /// Apakah hotspot darurat sedang aktif
    pub hotspot_active: bool,
    /// SSID hotspot (jika aktif)
    pub hotspot_ssid: String,
    /// Subnet hotspot (misal "192.168.137")
    pub hotspot_subnet: String,
    /// Jumlah peer yang terhubung saat ini
    pub connected_peers: usize,
    /// Jumlah peer yang pernah dikenal (dari database)
    pub known_peers: usize,
}

/// Aktifkan hotspot darurat (Mode Host — jadi access point).
///
/// Membuat WiFi hotspot "CARAKA-Emergency" tanpa password
/// agar rekan lain bisa terhubung saat mati lampu.
///
/// Membutuhkan hak Administrator Windows.
#[tauri::command]
pub async fn activate_emergency_hotspot(
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    use crate::hotspot;
    use crate::network_monitor::NetworkState;

    // Aktifkan hotspot via Windows netsh/PowerShell
    match hotspot::start_emergency_hotspot().await {
        Ok(status) => {
            // Update network state ke Emergency
            {
                let state_guard = state.lock().await;
                let mut ns = state_guard.network_state.lock().await;
                *ns = NetworkState::Emergency;
            }

            // Notify frontend
            app_handle.emit("emergency_hotspot_started", serde_json::json!({
                "ssid": status.ssid,
                "subnet": status.subnet,
                "message": format!(
                    "Hotspot '{}' aktif. Rekan lain bisa konek ke WiFi '{}' (tanpa password).",
                    hotspot::EMERGENCY_SSID, hotspot::EMERGENCY_SSID
                ),
            })).ok();

            Ok(format!(
                "Hotspot '{}' berhasil diaktifkan. Rekan bisa konek tanpa password.",
                hotspot::EMERGENCY_SSID
            ))
        }
        Err(e) => {
            // Jika gagal otomatis, beri instruksi manual
            app_handle.emit("emergency_hotspot_manual_needed", serde_json::json!({
                "ssid": hotspot::EMERGENCY_SSID,
                "error": e,
                "instruction": format!(
                    "Aktifkan Mobile Hotspot di Windows Settings.\n\
                     Atur SSID: '{}'\n\
                     Matikan password (open network).\n\
                     Rekan lain akan konek otomatis.", hotspot::EMERGENCY_SSID
                ),
            })).ok();

            Err(e)
        }
    }
}

/// Matikan hotspot darurat dan kembali ke mode normal.
#[tauri::command]
pub async fn deactivate_emergency_hotspot(
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    use crate::hotspot;
    use crate::network_monitor::NetworkState;

    hotspot::stop_emergency_hotspot().await?;

    // Reset ke Normal state
    {
        let state_guard = state.lock().await;
        let mut ns = state_guard.network_state.lock().await;
        *ns = NetworkState::Normal;
    }

    app_handle.emit("emergency_hotspot_stopped", serde_json::json!({
        "message": "Hotspot darurat dimatikan",
    })).ok();

    Ok(())
}

/// Dapatkan status Emergency Mode saat ini.
#[tauri::command]
pub async fn get_emergency_status(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<EmergencyStatus, String> {
    use crate::hotspot;

    let state_guard = state.lock().await;

    let network_state_str = {
        let ns = state_guard.network_state.lock().await;
        format!("{}", *ns)
    };

    let connected = {
        let senders = state_guard.peer_senders.lock().await;
        senders.len()
    };

    let known = {
        let db = state_guard.db_conn.lock().await;
        crate::store::get_all_peers(&db).map(|p| p.len()).unwrap_or(0)
    };

    // Cek status hotspot Windows
    let hotspot_status = hotspot::get_hotspot_status().await;

    Ok(EmergencyStatus {
        network_state: network_state_str,
        hotspot_active: hotspot_status.is_active,
        hotspot_ssid: hotspot_status.ssid,
        hotspot_subnet: hotspot_status.subnet,
        connected_peers: connected,
        known_peers: known,
    })
}

/// Coba reconnect ke semua peer yang pernah dikenal (dari database).
///
/// Digunakan saat masuk Mode Darurat — mencoba IP terakhir yang diketahui
/// untuk setiap peer, yang mungkin sudah dapat IP baru dari hotspot.
#[tauri::command]
pub async fn reconnect_known_peers(
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<usize, String> {
    let state_guard = state.lock().await;
    let peer_senders = state_guard.peer_senders.clone();

    let peers = {
        let db = state_guard.db_conn.lock().await;
        crate::store::get_all_peers(&db).map_err(|e| e.to_string())?
    };

    drop(state_guard);

    let mut attempted = 0usize;

    for peer in peers {
        if peer.ip_address.is_empty() {
            continue;
        }

        let ip = peer.ip_address.clone();
        let port = peer.tcp_port;
        let node_id = peer.node_id.clone();
        let handle = app_handle.clone();
        let senders = peer_senders.clone();

        tokio::spawn(async move {
            tracing::info!("Emergency reconnect ke {} ({})", peer.display_name, ip);
            crate::transport::connect_to_peer(
                &ip, port, Some(&node_id), handle, senders
            ).await;
        });

        attempted += 1;
    }

    Ok(attempted)
}

/// Scan jaringan hotspot (192.168.137.x) untuk menemukan peer CARAKA baru.
///
/// Digunakan oleh device yang konek sebagai CLIENT ke hotspot darurat.
/// Scan dilakukan port-by-port untuk menemukan peer yang listen di port 7771.
#[tauri::command]
pub async fn scan_emergency_network(
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<String>, String> {
    use crate::hotspot;

    let peer_senders = {
        let s = state.lock().await;
        s.peer_senders.clone()
    };

    // Scan subnet hotspot
    let found_ips = hotspot::scan_hotspot_subnet(
        crate::transport::DATA_PORT,
        300, // 300ms timeout per IP
    ).await;

    // Coba connect ke setiap IP yang ditemukan
    for ip in &found_ips {
        let ip_clone = ip.clone();
        let handle = app_handle.clone();
        let senders = peer_senders.clone();

        tokio::spawn(async move {
            crate::transport::connect_to_peer(
                &ip_clone,
                crate::transport::DATA_PORT,
                None,
                handle,
                senders,
            ).await;
        });
    }

    Ok(found_ips)
}

// ═══════════════════════════════════════════════════════════════════════════
// FITUR 4A — QR Code Peer Discovery
// ═══════════════════════════════════════════════════════════════════════════

/// Generate QR code PNG berisi Node ID + IP + Port untuk peer discovery.
///
/// Return: base64-encoded PNG string untuk ditampilkan di UI.
/// Data di-encode sebagai JSON: {"nodeId":"...","ip":"...","port":7771}
#[tauri::command]
pub async fn generate_peer_qr(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    use qrcode::QrCode;

    let (node_id, local_ip) = {
        let s = state.lock().await;
        (s.node_id_hex.clone(), get_local_ip().await.unwrap_or_else(|_| "unknown".to_string()))
    };

    let qr_data = serde_json::json!({
        "nodeId": node_id,
        "ip": local_ip,
        "port": crate::transport::DATA_PORT,
        "app": "CARAKA"
    }).to_string();

    let code = QrCode::new(qr_data.as_bytes())
        .map_err(|e| format!("Gagal buat QR code: {}", e))?;

    // Render QR matrix ke PNG menggunakan image crate 0.25 secara langsung
    let colors = code.to_colors();
    let modules = code.width();
    let scale: u32 = 8;
    let quiet: u32 = scale * 2; // quiet zone 2 modules
    let img_size = modules as u32 * scale + quiet * 2;

    let mut img_buf = image::GrayImage::new(img_size, img_size);
    // Fill putih
    for pixel in img_buf.pixels_mut() {
        *pixel = image::Luma([255u8]);
    }
    // Gambar modul hitam
    for (idx, &color) in colors.iter().enumerate() {
        if color == qrcode::Color::Dark {
            let mx = (idx % modules) as u32;
            let my = (idx / modules) as u32;
            let px = mx * scale + quiet;
            let py = my * scale + quiet;
            for dy in 0..scale {
                for dx in 0..scale {
                    img_buf.put_pixel(px + dx, py + dy, image::Luma([0u8]));
                }
            }
        }
    }

    // Encode ke PNG bytes
    let mut png_bytes: Vec<u8> = Vec::new();
    {
        use std::io::Cursor;
        let mut cursor = Cursor::new(&mut png_bytes);
        image::DynamicImage::ImageLuma8(img_buf)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| format!("Gagal encode PNG: {}", e))?;
    }

    // Kembalikan sebagai base64 data URI
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Ok(format!("data:image/png;base64,{}", b64))
}

// ═══════════════════════════════════════════════════════════════════════════
// FITUR 4B — Safety Number Verification
// ═══════════════════════════════════════════════════════════════════════════

/// Hitung Safety Number untuk verifikasi peer secara out-of-band.
///
/// Safety Number = SHA-256(my_pub_key || peer_pub_key) ditampilkan sebagai
/// 12 grup angka 5-digit (mirip Signal). User membandingkan angka ini via
/// telepon/SMS/tatap muka untuk memverifikasi tidak ada MITM.
#[tauri::command]
pub async fn compute_safety_number(
    peer_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    use sha2::{Sha256, Digest};

    let peer_bytes = hex::decode(&peer_id)
        .map_err(|_| "Peer ID tidak valid (bukan hex)".to_string())?;
    let peer_arr: [u8; 32] = peer_bytes
        .try_into()
        .map_err(|_| "Peer ID harus 32 byte".to_string())?;

    let my_pub = {
        let s = state.lock().await;
        s.my_node_id.0
    };

    // Canonical ordering: lexicographic supaya hasilnya sama di kedua sisi
    let (first, second) = if my_pub < peer_arr {
        (&my_pub, &peer_arr)
    } else {
        (&peer_arr, &my_pub)
    };

    let mut hasher = Sha256::new();
    hasher.update(first);
    hasher.update(second);
    let hash = hasher.finalize();

    // Konversi 32 byte ke 12 grup angka 5-digit (60 digit total)
    // Ambil 30 byte pertama (sudah cukup untuk 12 x 5 digit = 60 digit)
    let mut groups: Vec<String> = Vec::with_capacity(12);
    for i in 0..12 {
        // Ambil 2.5 byte per grup → pakai u16 dari 2 byte, mod 100000
        let byte_idx = i * 2;
        let val = u32::from_be_bytes([
            if byte_idx < 32 { hash[byte_idx] } else { 0 },
            if byte_idx + 1 < 32 { hash[byte_idx + 1] } else { 0 },
            if byte_idx + 2 < 32 { hash[byte_idx + 2] } else { 0 },
            if byte_idx + 3 < 32 { hash[byte_idx + 3] } else { 0 },
        ]) % 100_000;
        groups.push(format!("{:05}", val));
    }

    // Format: "12345 67890 12345 67890 ..."
    let formatted = groups.chunks(4)
        .map(|chunk| chunk.join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(formatted)
}
