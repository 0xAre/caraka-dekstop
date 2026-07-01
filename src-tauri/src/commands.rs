// src-tauri/src/commands.rs
// Fase 6 — Tauri IPC Command Handlers
//
// Semua fungsi di sini dipanggil dari frontend via invoke('command_name', args).
// ATURAN KEAMANAN:
//   - Tidak boleh return private key atau plaintext ke frontend dalam bentuk raw bytes
//   - Semua error harus di-handle, tidak boleh panic!
//
// State type: Arc<Mutex<Option<AppState>>>
//   - None = vault belum di-unlock
//   - Some = vault terbuka, node berjalan

use std::sync::Arc;
use tauri::{State, Emitter, Manager};
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use base64::Engine as _;

use crate::state::AppState;
use crate::packet::{ClampPacket, ClampHeader, HopAuth, PacketType, MAGIC, PROTOCOL_VERSION, TTL_MAX};
use crate::store::StoredMessage;

// Type alias untuk keterbacaan
type SharedState<'r> = State<'r, Arc<Mutex<Option<AppState>>>>;

/// Helper: unwrap Option<AppState> atau return error "vault terkunci".
fn require_state(opt: &Option<AppState>) -> Result<&AppState, String> {
    opt.as_ref()
        .ok_or_else(|| "Vault masih terkunci. Masukkan password dulu.".to_string())
}

fn require_state_mut(opt: &mut Option<AppState>) -> Result<&mut AppState, String> {
    opt.as_mut()
        .ok_or_else(|| "Vault masih terkunci. Masukkan password dulu.".to_string())
}

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
    pub reply_to_id: Option<String>,
    pub reply_to_text: Option<String>,
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

// ─── Vault Commands ────────────────────────────────────────────────────────

/// Cek apakah vault sudah ada (first run atau returning user).
#[tauri::command]
pub async fn check_vault_exists(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    Ok(crate::vault::vault_exists(&app_data_dir))
}

/// Buat vault baru dengan passphrase (first run).
///
/// Jika ada key lama di Windows Credential Manager, migrasi ke vault.
/// Jika tidak, generate key baru.
#[tauri::command]
pub async fn create_vault(
    passphrase: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    if passphrase.len() < 8 {
        return Err("Password minimal 8 karakter.".to_string());
    }

    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    // Coba load key lama dari keyring (migrasi dari versi sebelumnya)
    let private_key_bytes: [u8; 32] = {
        let from_keyring: Option<Vec<u8>> = keyring::Entry::new("caraka-desktop", "node-private-key")
            .ok()
            .and_then(|e| e.get_password().ok())
            .and_then(|hex_key| hex::decode(&hex_key).ok());

        if let Some(key_bytes) = from_keyring {
            if let Ok(arr) = key_bytes.try_into() {
                tracing::info!("Migrasi key dari Windows Credential Manager ke vault");
                arr
            } else {
                generate_new_private_key()
            }
        } else {
            generate_new_private_key()
        }
    };

    // Buat vault file dengan key + passphrase
    crate::vault::create_vault(&app_data_dir, &passphrase, &private_key_bytes)?;

    // Selesaikan inisialisasi node
    crate::state::complete_initialize(app_handle, private_key_bytes)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn generate_new_private_key() -> [u8; 32] {
    let (priv_key, _) = crate::keys::generate_keypair();
    priv_key.0
}

/// Buka vault dengan passphrase (returning user).
#[tauri::command]
pub async fn unlock_vault(
    passphrase: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    // Buka vault — return Err("Password salah") jika gagal
    let private_key_bytes = crate::vault::unlock_vault(&app_data_dir, &passphrase)?;

    // Selesaikan inisialisasi node
    crate::state::complete_initialize(app_handle, private_key_bytes)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ─── IPC Commands ──────────────────────────────────────────────────────────

/// Inisialisasi node — dipanggil frontend setelah node_ready event.
#[tauri::command]
pub async fn init_node(
    state: SharedState<'_>,
) -> Result<NodeInfo, String> {
    let state_opt = state.lock().await;
    let state = require_state(&state_opt)?;
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
    state: SharedState<'_>,
) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > 50 {
        return Err("Display name harus 1–50 karakter".to_string());
    }
    let trimmed = name.trim().to_string();

    let mut state_opt = state.lock().await;
    let state = require_state_mut(&mut state_opt)?;

    {
        let db = state.db_conn.lock().await;
        crate::store::save_setting(&db, "display_name", &trimmed)
            .map_err(|e| format!("Gagal simpan setting: {}", e))?;
    }

    state.display_name = trimmed;
    Ok(())
}

/// Kirim Direct Message ke peer tertentu.
#[tauri::command]
pub async fn send_dm(
    recipient_id: String,
    plaintext: String,
    reply_to_id: Option<String>,
    reply_to_text: Option<String>,
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<String, String> {
    if plaintext.trim().is_empty() {
        return Err("Pesan tidak boleh kosong".to_string());
    }
    if plaintext.len() > 4096 {
        return Err("Pesan terlalu panjang (max 4096 karakter)".to_string());
    }

    // SECURITY 3B: lepas outer lock setelah ekstrak Arc-Arc, sebelum komputasi kriptografi
    let (db_conn, peer_senders, my_priv_bytes, my_node_id_arr, node_id_hex, router) = {
        let state_opt = state.lock().await;
        let st = require_state(&state_opt)?;
        (
            st.db_conn.clone(),
            st.peer_senders.clone(),
            st.my_private_key.0,
            st.my_node_id.0,
            st.node_id_hex.clone(),
            st.router.clone(),
        )
    };

    // 1. Parse recipient Node ID
    let recipient_bytes = hex::decode(&recipient_id)
        .map_err(|_| "Recipient ID tidak valid (bukan hex string)".to_string())?;
    let peer_public: [u8; 32] = recipient_bytes
        .try_into()
        .map_err(|_| "Recipient ID harus 32 byte (64 hex chars)".to_string())?;
    let peer_id   = crate::keys::NodePublicKey(peer_public);
    let my_priv   = crate::keys::NodePrivateKey(my_priv_bytes);
    let my_pub    = crate::keys::NodePublicKey(my_node_id_arr);

    // 2. Ambil atau buat session info
    let (session_id, msg_counter) = {
        let db = db_conn.lock().await;
        crate::store::get_or_create_session(&db, &recipient_id)
            .map_err(|e| e.to_string())?
    };

    // 3. ECDH → shared secret
    let shared_secret = crate::keys::ecdh(&my_priv, &peer_id);

    // 4. Derive DM-Key
    let aead_key = crate::keys::derive_dm_key(
        &shared_secret,
        &my_pub,
        &peer_id,
        &session_id,
        msg_counter,
    );

    // 5. Generate Packet ID
    let packet_id = ClampPacket::generate_packet_id(&my_node_id_arr);

    // 6. Build inner payload
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let inner = crate::packet::DmInnerPayload {
        sender_id: node_id_hex.clone(),
        recipient_id: recipient_id.clone(),
        text: plaintext.clone(),
        timestamp: now,
        session_id: hex::encode(session_id),
        msg_counter,
        reply_to_id: reply_to_id.clone(),
        reply_to_text: reply_to_text.clone(),
    };

    let inner_json = serde_json::to_vec(&inner).map_err(|e| e.to_string())?;

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
    let (ciphertext, aead_tag) = crate::crypto::encrypt(&aead_key, &nonce, &inner_json, &aad)
        .map_err(|e| e.to_string())?;

    // 9. Hitung Hop-MAC
    let hop_mac = {
        let router_guard = router.lock().await;
        router_guard.compute_origin_mac(&ClampPacket {
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

    // 11. Simpan ke database
    let msg_uuid = format!("{}", now * 1000 + msg_counter);
    let stored_msg = StoredMessage {
        id: msg_uuid.clone(),
        packet_id: hex::encode(packet_id),
        sender_id: node_id_hex.clone(),
        recipient_id: recipient_id.clone(),
        nonce: nonce.0.to_vec(),
        ciphertext: ciphertext.clone(),
        aead_tag: aead_tag.to_vec(),
        received_at: now as i64,
        delivered: false,
    };

    {
        let db = db_conn.lock().await;
        crate::store::save_message(&db, &stored_msg).map_err(|e| e.to_string())?;
    }

    // 12. Increment message counter
    {
        let db = db_conn.lock().await;
        crate::store::increment_msg_counter(&db, &recipient_id).map_err(|e| e.to_string())?;
    }

    // 13. Register di router
    {
        let mut router_guard = router.lock().await;
        router_guard.register_outgoing(&full_packet);
    }

    // 14. Broadcast ke semua peer
    crate::transport::broadcast_packet(&full_packet, &peer_senders).await;

    // 15. Emit event ke UI
    app_handle
        .emit(
            "message_sent",
            serde_json::json!({
                "id": msg_uuid,
                "recipientId": recipient_id,
                "text": plaintext,
                "timestamp": now,
                "isOutgoing": true,
                "replyToId": reply_to_id,
                "replyToText": reply_to_text,
            }),
        )
        .ok();

    Ok(hex::encode(packet_id))
}

/// Dekripsi paket DM yang masuk (dipanggil dari event handler frontend).
///
/// SECURITY 3B: lock AppState hanya untuk ekstrak Arc, lalu lepas sebelum loop dekripsi.
#[tauri::command]
pub async fn try_decrypt_packet(
    packet_id: String,
    nonce_hex: String,
    ciphertext_hex: String,
    aead_tag_hex: String,
    state: SharedState<'_>,
) -> Result<Option<MessageInfo>, String> {
    let (db_conn, my_priv_bytes, my_node_id_arr) = {
        let state_opt = state.lock().await;
        let st = require_state(&state_opt)?;
        (st.db_conn.clone(), st.my_private_key.0, st.my_node_id.0)
    };

    let my_priv    = crate::keys::NodePrivateKey(my_priv_bytes);
    let my_node_id = crate::keys::NodePublicKey(my_node_id_arr);

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

    let mut aad = [0u8; crate::packet::HEADER_SIZE];
    aad[0..2].copy_from_slice(&MAGIC);
    aad[2] = PROTOCOL_VERSION;
    aad[3] = PacketType::Dm as u8;
    aad[4] = TTL_MAX;
    aad[5..13].copy_from_slice(&packet_id_arr);

    let peers = {
        let db = db_conn.lock().await;
        crate::store::get_all_peers(&db).map_err(|e| e.to_string())?
    };

    for peer in &peers {
        if let Ok(peer_bytes) = hex::decode(&peer.node_id) {
            if let Ok(arr) = peer_bytes.try_into() {
                let peer_pub = crate::keys::NodePublicKey(arr);
                let shared = crate::keys::ecdh(&my_priv, &peer_pub);

                let (session_id, stored_counter) = {
                    let db = db_conn.lock().await;
                    crate::store::get_or_create_session(&db, &peer.node_id)
                        .unwrap_or(([0u8; 8], 0))
                };

                let start = stored_counter.saturating_sub(5);
                let end = stored_counter + 50;

                for counter in start..end {
                    let aead_key = crate::keys::derive_dm_key(
                        &shared,
                        &peer_pub,
                        &my_node_id,
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
                        if let Ok(inner) =
                            serde_json::from_slice::<crate::packet::DmInnerPayload>(&plaintext_bytes)
                        {
                            let sender_bytes = hex::decode(&inner.sender_id)
                                .ok()
                                .and_then(|b| b.try_into().ok())
                                .map(crate::keys::NodePublicKey);

                            let sender_fp = sender_bytes
                                .as_ref()
                                .map(crate::keys::fingerprint)
                                .unwrap_or_default();

                            let now_ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64;

                            let stored_msg = StoredMessage {
                                id: format!(
                                    "{}_{}",
                                    &inner.sender_id[..8.min(inner.sender_id.len())],
                                    inner.timestamp
                                ),
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
                                let db = db_conn.lock().await;
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
                                reply_to_id: inner.reply_to_id,
                                reply_to_text: inner.reply_to_text,
                            }));
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Ambil daftar pesan dengan peer tertentu.
///
/// SECURITY 3B: lock AppState hanya untuk ekstrak Arc-Arc, lalu lepas sebelum loop dekripsi.
#[tauri::command]
pub async fn get_messages(
    peer_id: String,
    limit: Option<usize>,
    state: SharedState<'_>,
) -> Result<Vec<serde_json::Value>, String> {
    let limit = limit.unwrap_or(50).min(200);

    // Ekstrak field yang dibutuhkan, lepas outer lock segera
    let (db_conn, my_priv_bytes, node_id_hex) = {
        let state_opt = state.lock().await;
        let st = require_state(&state_opt)?;
        (
            st.db_conn.clone(),
            st.my_private_key.0,
            st.node_id_hex.clone(),
        )
    };

    let messages = {
        let db = db_conn.lock().await;
        crate::store::get_messages_between(&db, &node_id_hex, &peer_id, limit)
            .map_err(|e| e.to_string())?
    };

    let peer_bytes = hex::decode(&peer_id)
        .map_err(|_| "Peer ID tidak valid".to_string())?;
    let peer_pub_arr: [u8; 32] = peer_bytes
        .try_into()
        .map_err(|_| "Peer ID harus 32 byte".to_string())?;
    let peer_pub = crate::keys::NodePublicKey(peer_pub_arr);
    let my_priv  = crate::keys::NodePrivateKey(my_priv_bytes);
    let shared   = crate::keys::ecdh(&my_priv, &peer_pub);

    let my_pub_for_derive = crate::keys::public_key_from_private(&crate::keys::NodePrivateKey(my_priv_bytes));

    let mut result = Vec::new();
    for msg in messages {
        let is_outgoing = msg.sender_id == node_id_hex;

        if let (Ok(nonce_arr), Ok(tag_arr)) = (
            msg.nonce.clone().try_into().map_err(|_: Vec<u8>| ()),
            msg.aead_tag.clone().try_into().map_err(|_: Vec<u8>| ()),
        ) {
            let nonce_arr: [u8; 16] = nonce_arr;
            let tag_arr: [u8; 16] = tag_arr;

            let (session_id, stored_counter) = {
                let db = db_conn.lock().await;
                crate::store::get_or_create_session(&db, &peer_id).unwrap_or(([0u8; 8], 0))
            };

            let mut decrypted_text = None;

            let msg_packet_id_arr: [u8; 8] = msg
                .packet_id
                .as_bytes()
                .chunks(2)
                .take(8)
                .filter_map(|ch| {
                    std::str::from_utf8(ch)
                        .ok()
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
                        &shared,
                        &my_pub_for_derive,
                        &peer_pub,
                        &session_id,
                        counter,
                    )
                } else {
                    crate::keys::derive_dm_key(
                        &shared,
                        &peer_pub,
                        &my_pub_for_derive,
                        &session_id,
                        counter,
                    )
                };

                let nonce_w = crate::crypto::Nonce(nonce_arr);

                if let Ok(plain_bytes) =
                    crate::crypto::decrypt(&aead_key, &nonce_w, &msg.ciphertext, &tag_arr, &aad)
                {
                    if let Ok(inner) =
                        serde_json::from_slice::<crate::packet::DmInnerPayload>(&plain_bytes)
                    {
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
    state: SharedState<'_>,
) -> Result<Vec<PeerInfo>, String> {
    let state_opt = state.lock().await;
    let state = require_state(&state_opt)?;

    let db = state.db_conn.lock().await;
    let peers = crate::store::get_all_peers(&db).map_err(|e| e.to_string())?;

    let online_ids: Vec<String> = {
        let senders = state.peer_senders.lock().await;
        senders.keys().cloned().collect()
    };

    let result: Vec<PeerInfo> = peers
        .iter()
        .map(|p| {
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
        })
        .collect();

    Ok(result)
}

/// Tambah peer secara manual via IP:Port.
#[tauri::command]
pub async fn add_peer_manual(
    ip: String,
    port: Option<u16>,
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<String, String> {
    let port = port.unwrap_or(crate::transport::DATA_PORT);

    if ip.trim().is_empty() {
        return Err("IP address tidak boleh kosong".to_string());
    }

    let state_guard = state.lock().await;
    let st = require_state(&state_guard)?;
    let peer_senders = st.peer_senders.clone();
    drop(state_guard);

    let ip_clone = ip.clone();
    tokio::spawn(async move {
        crate::transport::connect_to_peer(&ip_clone, port, None, app_handle, peer_senders).await;
    });

    Ok(format!("Mencoba connect ke {}:{}", ip, port))
}

/// Get status jaringan.
#[tauri::command]
pub async fn get_network_status(
    state: SharedState<'_>,
) -> Result<NetworkStatus, String> {
    let state_opt = state.lock().await;
    let state = require_state(&state_opt)?;

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
#[tauri::command]
pub async fn get_local_ip() -> Result<String, String> {
    let ifaces = get_if_addrs::get_if_addrs()
        .map_err(|e| format!("Gagal baca interface: {}", e))?;

    for iface in &ifaces {
        if iface.is_loopback() {
            continue;
        }
        if let get_if_addrs::IfAddr::V4(ref v4) = iface.addr {
            let ip = v4.ip.to_string();
            if !ip.starts_with("169.254") {
                return Ok(ip);
            }
        }
    }

    Ok("Tidak diketahui".to_string())
}

/// Kirim pesan Broadcast darurat.
#[tauri::command]
pub async fn send_broadcast(
    text: String,
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("Pesan tidak boleh kosong".to_string());
    }
    if text.len() > 500 {
        return Err("Pesan broadcast terlalu panjang (max 500 karakter)".to_string());
    }

    let state_opt = state.lock().await;
    let state = require_state(&state_opt)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let message_id = uuid::Uuid::new_v4().to_string();

    let payload = crate::packet::BroadcastPayload {
        sender_id: state.node_id_hex.clone(),
        sender_name: state.display_name.clone(),
        text: text.clone(),
        timestamp: now,
        message_id: message_id.clone(),
    };

    let pkt = crate::packet::ClampPacket::build_broadcast(&state.my_node_id.0, &payload);

    {
        let mut router = state.router.lock().await;
        router.register_outgoing(&pkt);
    }

    crate::transport::broadcast_packet(&pkt, &state.peer_senders).await;

    app_handle
        .emit(
            "broadcast_sent",
            serde_json::json!({
                "senderId":   state.node_id_hex,
                "senderName": state.display_name,
                "text":       text,
                "timestamp":  now,
                "messageId":  message_id,
            }),
        )
        .ok();

    Ok("Pesan broadcast terkirim".to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// EMERGENCY MODE COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmergencyStatus {
    pub network_state: String,
    pub hotspot_active: bool,
    pub hotspot_ssid: String,
    pub hotspot_subnet: String,
    pub connected_peers: usize,
    pub known_peers: usize,
}

#[tauri::command]
pub async fn activate_emergency_hotspot(
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<String, String> {
    use crate::hotspot;
    use crate::network_monitor::NetworkState;

    match hotspot::start_emergency_hotspot().await {
        Ok(status) => {
            {
                let state_guard = state.lock().await;
                let st = require_state(&state_guard)?;
                let mut ns = st.network_state.lock().await;
                *ns = NetworkState::Emergency;
            }

            app_handle
                .emit(
                    "emergency_hotspot_started",
                    serde_json::json!({
                        "ssid": status.ssid,
                        "subnet": status.subnet,
                        "message": format!(
                            "Hotspot '{}' aktif. Rekan lain bisa konek ke WiFi '{}' (tanpa password).",
                            hotspot::EMERGENCY_SSID, hotspot::EMERGENCY_SSID
                        ),
                    }),
                )
                .ok();

            Ok(format!(
                "Hotspot '{}' berhasil diaktifkan.",
                hotspot::EMERGENCY_SSID
            ))
        }
        Err(e) => {
            app_handle
                .emit(
                    "emergency_hotspot_manual_needed",
                    serde_json::json!({
                        "ssid": hotspot::EMERGENCY_SSID,
                        "error": e,
                        "instruction": format!(
                            "Aktifkan Mobile Hotspot di Windows Settings.\n\
                             Atur SSID: '{}'\n\
                             Matikan password (open network).",
                            hotspot::EMERGENCY_SSID
                        ),
                    }),
                )
                .ok();

            Err(e)
        }
    }
}

#[tauri::command]
pub async fn deactivate_emergency_hotspot(
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<(), String> {
    use crate::hotspot;
    use crate::network_monitor::NetworkState;

    hotspot::stop_emergency_hotspot().await?;

    {
        let state_guard = state.lock().await;
        let st = require_state(&state_guard)?;
        let mut ns = st.network_state.lock().await;
        *ns = NetworkState::Normal;
    }

    app_handle
        .emit(
            "emergency_hotspot_stopped",
            serde_json::json!({ "message": "Hotspot darurat dimatikan" }),
        )
        .ok();

    Ok(())
}

#[tauri::command]
pub async fn get_emergency_status(
    state: SharedState<'_>,
) -> Result<EmergencyStatus, String> {
    use crate::hotspot;

    let state_guard = state.lock().await;
    let state = require_state(&state_guard)?;

    let network_state_str = {
        let ns = state.network_state.lock().await;
        format!("{}", *ns)
    };

    let connected = {
        let senders = state.peer_senders.lock().await;
        senders.len()
    };

    let known = {
        let db = state.db_conn.lock().await;
        crate::store::get_all_peers(&db).map(|p| p.len()).unwrap_or(0)
    };

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

#[tauri::command]
pub async fn reconnect_known_peers(
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<usize, String> {
    let state_guard = state.lock().await;
    let st = require_state(&state_guard)?;
    let peer_senders = st.peer_senders.clone();

    let peers = {
        let db = st.db_conn.lock().await;
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
            crate::transport::connect_to_peer(&ip, port, Some(&node_id), handle, senders).await;
        });

        attempted += 1;
    }

    Ok(attempted)
}

#[tauri::command]
pub async fn scan_emergency_network(
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<Vec<String>, String> {
    use crate::hotspot;

    let peer_senders = {
        let s = state.lock().await;
        let st = require_state(&s)?;
        st.peer_senders.clone()
    };

    let found_ips = hotspot::scan_hotspot_subnet(crate::transport::DATA_PORT, 300).await;

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
            )
            .await;
        });
    }

    Ok(found_ips)
}

// ═══════════════════════════════════════════════════════════════════════════
// F0 — Tor Transport
// ═══════════════════════════════════════════════════════════════════════════

/// Kembalikan alamat .onion node ini, atau None jika Tor belum bootstrap.
#[tauri::command]
pub async fn get_onion_address(state: SharedState<'_>) -> Result<Option<String>, String> {
    let state_opt = state.lock().await;
    let st = require_state(&state_opt)?;
    let tor = st.tor_ctx.lock().await;
    Ok(tor.as_ref().map(|ctx| ctx.onion_address.clone()))
}

// ═══════════════════════════════════════════════════════════════════════════
// F6 — Text Invite Code + Onion Address Sharing
// ═══════════════════════════════════════════════════════════════════════════

/// Hasilkan kode undangan base64url yang berisi Node ID + alamat (LAN atau onion).
///
/// Format raw sebelum encode:
///   caraka0:<nodeId>:<localIp>:<tcpPort>     — via LAN
///   caraka1:<nodeId>:<onionAddress>:<torPort> — via Tor
#[tauri::command]
pub async fn generate_invite_code(state: SharedState<'_>) -> Result<String, String> {
    use base64::Engine as _;

    let (node_id, tor_addr) = {
        let state_opt = state.lock().await;
        let st = require_state(&state_opt)?;
        let tor = st.tor_ctx.lock().await;
        let tor_addr = tor.as_ref().map(|ctx| ctx.onion_address.clone());
        (st.node_id_hex.clone(), tor_addr)
    };

    let raw = if let Some(onion) = tor_addr {
        format!(
            "caraka1:{}:{}:{}",
            node_id,
            onion,
            crate::tor::TOR_VIRTUAL_PORT
        )
    } else {
        let local_ip = get_local_ip()
            .await
            .unwrap_or_else(|_| "unknown".to_string());
        format!(
            "caraka0:{}:{}:{}",
            node_id,
            local_ip,
            crate::transport::DATA_PORT
        )
    };

    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes()))
}

/// Parse kode undangan dan kembalikan JSON dengan info koneksi.
#[tauri::command]
pub async fn parse_invite_code(code: String) -> Result<serde_json::Value, String> {
    use base64::Engine as _;

    let decoded_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(code.trim())
        .map_err(|_| "Kode undangan tidak valid (bukan base64url)".to_string())?;
    let s = String::from_utf8(decoded_bytes)
        .map_err(|_| "Kode undangan tidak valid (encoding)".to_string())?;

    let parts: Vec<&str> = s.splitn(4, ':').collect();
    match parts.as_slice() {
        ["caraka0", node_id, ip, port] => Ok(serde_json::json!({
            "nodeId":  node_id,
            "ip":      ip,
            "port":    port.parse::<u16>().unwrap_or(crate::transport::DATA_PORT),
            "via":     "lan"
        })),
        ["caraka1", node_id, onion, port] => Ok(serde_json::json!({
            "nodeId":        node_id,
            "onionAddress":  onion,
            "port":          port.parse::<u16>().unwrap_or(crate::tor::TOR_VIRTUAL_PORT),
            "via":           "tor"
        })),
        _ => Err("Format kode undangan tidak dikenali".to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// F2 — File Transfer
// ═══════════════════════════════════════════════════════════════════════════

/// Kirim file ke peer tertentu via jalur E2EE yang sama dengan DM.
/// Batasan: 5 MB per file.
#[tauri::command]
pub async fn send_file(
    recipient_id: String,
    file_path: String,
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<String, String> {
    // 1. Baca file + validasi ukuran
    let (file_bytes, filename, mime_type) =
        crate::file_transfer::read_file_for_transfer(&file_path)?;

    let file_size = file_bytes.len() as u64;

    // 2. Ambil state yang dibutuhkan, lepas lock sesegera mungkin (SECURITY 3B)
    let (db_conn, peer_senders, my_priv_bytes, node_id_hex, my_node_id_arr, router, hop_mac_key) = {
        let state_opt = state.lock().await;
        let st = require_state(&state_opt)?;
        (
            st.db_conn.clone(),
            st.peer_senders.clone(),
            st.my_private_key.0,
            st.node_id_hex.clone(),
            st.my_node_id.0,
            st.router.clone(),
            st.hop_mac_key,
        )
    };

    let recipient_bytes = hex::decode(&recipient_id)
        .map_err(|_| "Recipient ID tidak valid".to_string())?;
    let peer_pub_arr: [u8; 32] = recipient_bytes
        .try_into()
        .map_err(|_| "Recipient ID harus 32 byte".to_string())?;
    let peer_id = crate::keys::NodePublicKey(peer_pub_arr);
    let my_priv = crate::keys::NodePrivateKey(my_priv_bytes);
    let my_node_id = crate::keys::NodePublicKey(my_node_id_arr);

    // 3. Session + counter
    let (session_id, msg_counter) = {
        let db = db_conn.lock().await;
        crate::store::get_or_create_session(&db, &recipient_id)
            .map_err(|e| e.to_string())?
    };

    // 4. ECDH → shared secret → AEAD key
    let shared_secret = crate::keys::ecdh(&my_priv, &peer_id);
    let aead_key = crate::keys::derive_dm_key(
        &shared_secret,
        &my_node_id,
        &peer_id,
        &session_id,
        msg_counter,
    );

    // 5. Build payload
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let transfer_id = uuid::Uuid::new_v4().to_string();
    let file_data_b64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);

    let payload = crate::packet::FilePayload {
        sender_id: node_id_hex.clone(),
        recipient_id: recipient_id.clone(),
        transfer_id: transfer_id.clone(),
        filename: filename.clone(),
        mime_type: mime_type.clone(),
        file_size,
        file_data_b64,
        timestamp: now,
        session_id: hex::encode(session_id),
        msg_counter,
    };
    let payload_json = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;

    // 6. Build header
    let packet_id = ClampPacket::generate_packet_id(&my_node_id_arr);
    let header = ClampHeader {
        magic: MAGIC,
        version: PROTOCOL_VERSION,
        packet_type: PacketType::File,
        ttl: TTL_MAX,
        packet_id,
    };

    let temp_pkt = ClampPacket {
        header: header.clone(),
        hop_auth: HopAuth { hop_counter: 0, mac_tag: [0u8; 16] },
        nonce: [0u8; 16],
        ciphertext: vec![],
        aead_tag: [0u8; 16],
    };
    let aad = temp_pkt.header_bytes();

    // 7. Enkripsi
    let nonce = crate::crypto::generate_nonce();
    let (ciphertext, aead_tag) = crate::crypto::encrypt(&aead_key, &nonce, &payload_json, &aad)
        .map_err(|e| e.to_string())?;

    // 8. Hop-MAC
    let hop_mac = {
        let router_guard = router.lock().await;
        let _ = hop_mac_key; // digunakan di router internal
        router_guard.compute_origin_mac(&ClampPacket {
            header: header.clone(),
            hop_auth: HopAuth { hop_counter: 0, mac_tag: [0u8; 16] },
            nonce: nonce.0,
            ciphertext: ciphertext.clone(),
            aead_tag,
        })
    };

    // 9. Build paket lengkap
    let full_pkt = ClampPacket {
        header,
        hop_auth: HopAuth { hop_counter: 0, mac_tag: hop_mac },
        nonce: nonce.0,
        ciphertext: ciphertext.clone(),
        aead_tag,
    };

    // 10. Increment counter
    {
        let db = db_conn.lock().await;
        crate::store::increment_msg_counter(&db, &recipient_id).map_err(|e| e.to_string())?;
    }

    // 11. Register + broadcast
    {
        let mut router_guard = router.lock().await;
        router_guard.register_outgoing(&full_pkt);
    }
    crate::transport::broadcast_packet(&full_pkt, &peer_senders).await;

    // 12. Notify UI
    app_handle
        .emit(
            "file_sent",
            serde_json::json!({
                "transferId":  transfer_id,
                "recipientId": recipient_id,
                "filename":    filename,
                "mimeType":    mime_type,
                "fileSize":    file_size,
                "timestamp":   now,
            }),
        )
        .ok();

    Ok(transfer_id)
}

/// Dekripsi paket File yang masuk dan simpan ke folder Downloads/CARAKA/.
/// Dipanggil dari frontend saat menerima `clamp_packet_received` dengan packetType = 0x08.
#[tauri::command]
pub async fn try_decrypt_file_packet(
    packet_id: String,
    nonce_hex: String,
    ciphertext_hex: String,
    aead_tag_hex: String,
    app_handle: tauri::AppHandle,
    state: SharedState<'_>,
) -> Result<Option<serde_json::Value>, String> {
    // Ambil state lalu lepas lock (SECURITY 3B)
    let (db_conn, my_priv_bytes, my_node_id_arr) = {
        let state_opt = state.lock().await;
        let st = require_state(&state_opt)?;
        (
            st.db_conn.clone(),
            st.my_private_key.0,
            st.my_node_id.0,
        )
    };

    let my_priv = crate::keys::NodePrivateKey(my_priv_bytes);
    let my_node_id = crate::keys::NodePublicKey(my_node_id_arr);

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

    // AAD untuk File packet type
    let mut aad = [0u8; crate::packet::HEADER_SIZE];
    aad[0..2].copy_from_slice(&MAGIC);
    aad[2] = PROTOCOL_VERSION;
    aad[3] = PacketType::File as u8;
    aad[4] = TTL_MAX;
    aad[5..13].copy_from_slice(&packet_id_arr);

    let peers = {
        let db = db_conn.lock().await;
        crate::store::get_all_peers(&db).map_err(|e| e.to_string())?
    };

    for peer in &peers {
        if let Ok(peer_bytes) = hex::decode(&peer.node_id) {
            if let Ok(arr) = peer_bytes.try_into() {
                let peer_pub = crate::keys::NodePublicKey(arr);
                let shared = crate::keys::ecdh(&my_priv, &peer_pub);

                let (session_id, stored_counter) = {
                    let db = db_conn.lock().await;
                    crate::store::get_or_create_session(&db, &peer.node_id)
                        .unwrap_or(([0u8; 8], 0))
                };

                for counter in stored_counter.saturating_sub(5)..stored_counter + 50 {
                    let aead_key = crate::keys::derive_dm_key(
                        &shared,
                        &peer_pub,
                        &my_node_id,
                        &session_id,
                        counter,
                    );

                    if let Ok(plain) = crate::crypto::decrypt(
                        &aead_key,
                        &crate::crypto::Nonce(nonce),
                        &ciphertext,
                        &aead_tag,
                        &aad,
                    ) {
                        if let Ok(fp) =
                            serde_json::from_slice::<crate::packet::FilePayload>(&plain)
                        {
                            // Dekode base64 → bytes
                            let file_bytes = base64::engine::general_purpose::STANDARD
                                .decode(&fp.file_data_b64)
                                .map_err(|e| format!("Gagal decode base64 file: {e}"))?;

                            // Simpan ke disk
                            let saved_path = crate::file_transfer::save_received_file(
                                &app_handle,
                                &fp.filename,
                                &file_bytes,
                            )?;

                            let path_str = saved_path.to_string_lossy().to_string();
                            let is_image =
                                crate::file_transfer::is_previewable_image(&fp.mime_type);

                            app_handle
                                .emit(
                                    "file_received",
                                    serde_json::json!({
                                        "transferId":  fp.transfer_id,
                                        "senderId":    fp.sender_id,
                                        "filename":    fp.filename,
                                        "mimeType":    fp.mime_type,
                                        "fileSize":    fp.file_size,
                                        "savedPath":   path_str,
                                        "isImage":     is_image,
                                        "timestamp":   fp.timestamp,
                                    }),
                                )
                                .ok();

                            return Ok(Some(serde_json::json!({
                                "transferId":  fp.transfer_id,
                                "senderId":    fp.sender_id,
                                "filename":    fp.filename,
                                "mimeType":    fp.mime_type,
                                "fileSize":    fp.file_size,
                                "savedPath":   path_str,
                                "isImage":     is_image,
                                "timestamp":   fp.timestamp,
                            })));
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

// ═══════════════════════════════════════════════════════════════════════════
// RELEASE 5D — Auto-Updater
// ═══════════════════════════════════════════════════════════════════════════

/// Cek pembaruan tersedia via GitHub Releases.
/// Return JSON: { available, version?, body? }
#[tauri::command]
pub async fn check_for_updates(app_handle: tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app_handle
        .updater()
        .map_err(|e| format!("Updater tidak tersedia: {e}"))?;

    match updater.check().await {
        Ok(Some(update)) => Ok(serde_json::json!({
            "available": true,
            "version":   update.version,
            "body":      update.body.unwrap_or_default()
        })),
        Ok(None) => Ok(serde_json::json!({ "available": false })),
        Err(e)   => Err(format!("Gagal cek update: {e}")),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// FITUR 4A — QR Code Peer Discovery
// ═══════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn generate_peer_qr(
    state: SharedState<'_>,
) -> Result<String, String> {
    use qrcode::QrCode;

    let (node_id, local_ip) = {
        let s = state.lock().await;
        let st = require_state(&s)?;
        (
            st.node_id_hex.clone(),
            get_local_ip()
                .await
                .unwrap_or_else(|_| "unknown".to_string()),
        )
    };

    let qr_data = serde_json::json!({
        "nodeId": node_id,
        "ip": local_ip,
        "port": crate::transport::DATA_PORT,
        "app": "CARAKA"
    })
    .to_string();

    let code = QrCode::new(qr_data.as_bytes())
        .map_err(|e| format!("Gagal buat QR code: {}", e))?;

    let colors = code.to_colors();
    let modules = code.width();
    let scale: u32 = 8;
    let quiet: u32 = scale * 2;
    let img_size = modules as u32 * scale + quiet * 2;

    let mut img_buf = image::GrayImage::new(img_size, img_size);
    for pixel in img_buf.pixels_mut() {
        *pixel = image::Luma([255u8]);
    }
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

    let mut png_bytes: Vec<u8> = Vec::new();
    {
        use std::io::Cursor;
        let mut cursor = Cursor::new(&mut png_bytes);
        image::DynamicImage::ImageLuma8(img_buf)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| format!("Gagal encode PNG: {}", e))?;
    }

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Ok(format!("data:image/png;base64,{}", b64))
}

// ═══════════════════════════════════════════════════════════════════════════
// FITUR 4B — Safety Number Verification
// ═══════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn compute_safety_number(
    peer_id: String,
    state: SharedState<'_>,
) -> Result<String, String> {
    use sha2::{Sha256, Digest};

    let peer_bytes = hex::decode(&peer_id)
        .map_err(|_| "Peer ID tidak valid (bukan hex)".to_string())?;
    let peer_arr: [u8; 32] = peer_bytes
        .try_into()
        .map_err(|_| "Peer ID harus 32 byte".to_string())?;

    let my_pub = {
        let s = state.lock().await;
        let st = require_state(&s)?;
        st.my_node_id.0
    };

    let (first, second) = if my_pub < peer_arr {
        (&my_pub, &peer_arr)
    } else {
        (&peer_arr, &my_pub)
    };

    let mut hasher = Sha256::new();
    hasher.update(first);
    hasher.update(second);
    let hash = hasher.finalize();

    let mut groups: Vec<String> = Vec::with_capacity(12);
    for i in 0..12 {
        let byte_idx = i * 2;
        let val = u32::from_be_bytes([
            if byte_idx < 32 { hash[byte_idx] } else { 0 },
            if byte_idx + 1 < 32 { hash[byte_idx + 1] } else { 0 },
            if byte_idx + 2 < 32 { hash[byte_idx + 2] } else { 0 },
            if byte_idx + 3 < 32 { hash[byte_idx + 3] } else { 0 },
        ]) % 100_000;
        groups.push(format!("{:05}", val));
    }

    let formatted = groups
        .chunks(4)
        .map(|chunk| chunk.join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(formatted)
}
