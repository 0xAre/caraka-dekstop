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

use crate::state::AppState;
use crate::packet::{ClampPacket, ClampHeader, HopAuth, PacketType, MAGIC, PROTOCOL_VERSION, TTL_MAX};
use crate::store::StoredMessage;

// ─── Response Types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeInfo {
    pub node_id: String,
    pub fingerprint: String,
    pub display_name: String,
    pub tcp_port: u16,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    state.display_name = name.trim().to_string();
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

    // Coba dekripsi dengan setiap peer yang dikenal
    let peers = {
        let db = state.db_conn.lock().await;
        crate::store::get_all_peers(&db).map_err(|e| e.to_string())?
    };

    for peer in peers {
        if let Ok(peer_bytes) = hex::decode(&peer.node_id) {
            if let Ok(arr) = peer_bytes.try_into() {
                let peer_pub = crate::keys::NodePublicKey(arr);
                let shared = crate::keys::ecdh(&state.my_private_key, &peer_pub);

                // Coba dengan session_id = [0;8] dulu (simple approach)
                let session_id = [0u8; 8];

                // Coba beberapa counter terakhir (0..10)
                for counter in 0u64..10 {
                    let aead_key = crate::keys::derive_dm_key(
                        &shared,
                        &peer_pub,       // sender = peer
                        &state.my_node_id, // receiver = kita
                        &session_id,
                        counter,
                    );

                    let nonce_wrapper = crate::crypto::Nonce(nonce);
                    // Buat AAD minimal (kita tidak punya header asli)
                    let aad = [0u8; 13]; // Simplified — full implementation perlu header

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

            let session_id = [0u8; 8];
            let mut decrypted_text = None;

            for counter in 0u64..100 {
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
                let aad = [0u8; 13];

                if let Ok(plain_bytes) = crate::crypto::decrypt(
                    &aead_key, &nonce_w, &msg.ciphertext, &tag_arr, &aad
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
    let ip_clone = ip.clone();
    tokio::spawn(async move {
        crate::transport::connect_to_peer(&ip_clone, port, app_handle, peer_senders).await;
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
