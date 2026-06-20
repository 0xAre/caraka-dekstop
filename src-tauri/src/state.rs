// src-tauri/src/state.rs
// Fase 6 — Shared Application State
//
// AppState diakses dari semua Tauri command handlers dan background tasks.
// Semua field mutable diproteksi dengan Arc<Mutex<...>>.

use std::sync::Arc;
use tauri::{Manager, Emitter, Listener};
use tokio::sync::Mutex;
use rusqlite::Connection;

use crate::keys::{NodePublicKey, NodePrivateKey};
use crate::routing::Router;
use crate::transport::PeerSenders;
use crate::network_monitor::NetworkState;

/// Shared application state yang dikelola Tauri.
///
/// Diakses via `State<'_, Arc<Mutex<AppState>>>` di command handlers.
pub struct AppState {
    /// Node ID kita (32 byte hex string) — untuk identifikasi di UI
    pub node_id_hex: String,
    /// X25519 public key — identitas permanen node ini
    pub my_node_id: NodePublicKey,
    /// X25519 private key — TIDAK BOLEH di-expose ke frontend!
    pub my_private_key: NodePrivateKey,
    /// SQLite connection (protected)
    pub db_conn: Arc<Mutex<Connection>>,
    /// Router untuk routing decisions
    pub router: Arc<Mutex<Router>>,
    /// Nama tampilan node yang dikonfigurasi user
    pub display_name: String,
    /// Map dari peer_id → sender channel (untuk broadcast)
    pub peer_senders: PeerSenders,
    /// Status jaringan saat ini (Normal / Lost / Emergency)
    pub network_state: Arc<Mutex<NetworkState>>,
    /// BUG #4 FIX: Hop MAC key — derived dari private key via HKDF-SHA256.
    /// Lebih aman dari [0u8;16] karena unik per node.
    /// TODO: untuk relay chain yang benar, perlu key exchange per-channel.
    pub hop_mac_key: [u8; 16],
}

/// Inisialisasi AppState dan start background services.
///
/// Dipanggil sekali saat aplikasi pertama kali dibuka.
pub async fn initialize(app_handle: tauri::AppHandle) -> anyhow::Result<()> {
    use crate::keys;
    use crate::store;

    // 1. Tentukan path database
    let db_path = app_handle
        .path()
        .app_data_dir()?
        .join("caraka.db");

    // Buat direktori jika belum ada
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 2. Buka SQLite database
    let conn = store::open_db(&db_path)?;

    // 3. Load atau generate identity keypair
    // BUG #5 FIX: Coba keyring dulu, fallback ke SQLite
    let (private_key, public_key) = {
        // A. Coba load dari keyring (Windows Credential Manager)
        let from_keyring: Option<Vec<u8>> = {
            match keyring::Entry::new("caraka-desktop", "node-private-key") {
                Ok(entry) => {
                    match entry.get_password() {
                        Ok(hex_key) => {
                            hex::decode(&hex_key).ok()
                        }
                        Err(_) => None,
                    }
                }
                Err(_) => None,
            }
        };

        if let Some(key_bytes) = from_keyring {
            if let Ok(arr) = key_bytes.try_into() {
                let priv_key = NodePrivateKey(arr);
                let pub_key = keys::public_key_from_private(&priv_key);
                tracing::info!("Identity key loaded dari keyring (Windows Credential Manager)");
                (priv_key, pub_key)
            } else {
                generate_and_save_keypair(&conn)?
            }
        } else {
            // B. Fallback: coba load dari database
            let existing = store::load_identity_key(&conn, "node_identity")?;
            if let Some(key_bytes) = existing {
                if let Ok(arr) = key_bytes.try_into() {
                    let priv_key = NodePrivateKey(arr);
                    let pub_key = keys::public_key_from_private(&priv_key);

                    // Migrasi: simpan ke keyring juga untuk keamanan lebih baik
                    if let Ok(entry) = keyring::Entry::new("caraka-desktop", "node-private-key") {
                        let _ = entry.set_password(&hex::encode(&priv_key.0));
                        tracing::info!("Identity key dimigrasikan dari DB ke keyring");
                    }

                    tracing::info!("Identity key loaded dari database (fallback)");
                    (priv_key, pub_key)
                } else {
                    generate_and_save_keypair(&conn)?
                }
            } else {
                generate_and_save_keypair(&conn)?
            }
        }
    };

    let node_id_hex = hex::encode(public_key.0);
    let fingerprint = keys::fingerprint(&public_key);

    tracing::info!(
        "CARAKA node dimulai — ID: {}... fingerprint: {}",
        &node_id_hex[..8],
        fingerprint
    );

    // 4. Load display_name dari database (fallback ke "User")
    let display_name = crate::store::load_setting(&conn, "display_name")
        .ok()
        .flatten()
        .unwrap_or_else(|| "User".to_string());

    // 5. Inisialisasi Router
    let router = Router::new(public_key.clone());

    // 5b. BUG #4 FIX: Derive hop_mac_key dari private key menggunakan HKDF-SHA256
    // Ini lebih aman dari [0u8;16] karena unik per node dan tidak mudah ditebak.
    let hop_mac_key = {
        use hkdf::Hkdf;
        use sha2::Sha256;
        let hk = Hkdf::<Sha256>::new(Some(b"CARAKA-HOP-MAC-v1"), &private_key.0);
        let mut key = [0u8; 16];
        hk.expand(b"hop-authentication", &mut key)
            .expect("HKDF expand untuk hop_mac_key");
        key
    };

    // 6. Buat PeerSenders
    let peer_senders: PeerSenders = Arc::new(Mutex::new(std::collections::HashMap::new()));

    // 6b. Buat NetworkState (mulai dengan Normal)
    let network_state: Arc<Mutex<NetworkState>> = Arc::new(Mutex::new(NetworkState::Normal));

    // 7. Build AppState
    let state = AppState {
        node_id_hex: node_id_hex.clone(),
        my_node_id: public_key.clone(),
        my_private_key: private_key,
        db_conn: Arc::new(Mutex::new(conn)),
        router: Arc::new(Mutex::new(router)),
        display_name,
        peer_senders: peer_senders.clone(),
        network_state: network_state.clone(),
        hop_mac_key,
    };

    // 7. Manage state di Tauri
    app_handle.manage(Arc::new(Mutex::new(state)));

    // 8. Start background services
    let handle = app_handle.clone();
    let senders = peer_senders.clone();
    tokio::spawn(async move {
        crate::transport::start_tcp_server(handle, senders).await;
    });

    let handle = app_handle.clone();
    tokio::spawn(async move {
        crate::discovery::start_broadcaster(handle).await;
    });

    let handle = app_handle.clone();
    tokio::spawn(async move {
        crate::discovery::start_listener(handle).await;
    });

    // Start network monitor — deteksi mati lampu / hilangnya jaringan
    let handle = app_handle.clone();
    let ns = network_state.clone();
    tokio::spawn(async move {
        crate::network_monitor::start_network_monitor(handle, ns).await;
    });

    // 9. Listen event connect_to_peer dari discovery
    {
        let handle = app_handle.clone();
        let senders = peer_senders.clone();
        app_handle.listen("connect_to_peer", move |event| {
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                let ip = payload["ip"].as_str().unwrap_or("").to_string();
                let port = payload["port"].as_u64().unwrap_or(7771) as u16;
                // [FIX #6] Teruskan nodeId ke connect_to_peer untuk cek duplikat yang benar
                let node_id = payload["nodeId"].as_str().unwrap_or("").to_string();
                let handle2 = handle.clone();
                let senders2 = senders.clone();
                tokio::spawn(async move {
                    // [FIX #6] node_id sebagai Option<String>
                    let node_id_hint: Option<String> = if node_id.is_empty() { None } else { Some(node_id) };
                    crate::transport::connect_to_peer(&ip, port, node_id_hint.as_deref(), handle2, senders2).await;
                });
            }
        });
    }

    // 10. Notify frontend node siap
    app_handle.emit("node_ready", serde_json::json!({
        "nodeId": node_id_hex,
        "fingerprint": fingerprint,
        "tcpPort": crate::transport::DATA_PORT,
        "discoveryPort": crate::discovery::DISCOVERY_PORT,
    }))?;

    tracing::info!("CARAKA node siap! TCP:{} UDP:{}", 
        crate::transport::DATA_PORT, 
        crate::discovery::DISCOVERY_PORT
    );

    Ok(())
}

/// Generate keypair baru dan simpan ke keyring + database.
fn generate_and_save_keypair(
    conn: &Connection,
) -> anyhow::Result<(NodePrivateKey, NodePublicKey)> {
    use crate::keys;
    use crate::store;

    let (priv_key, pub_key) = keys::generate_keypair();

    // BUG #5 FIX: Simpan ke keyring DULU (lebih aman)
    if let Ok(entry) = keyring::Entry::new("caraka-desktop", "node-private-key") {
        if entry.set_password(&hex::encode(&priv_key.0)).is_ok() {
            tracing::info!("Identity key tersimpan di Windows Credential Manager");
        } else {
            tracing::warn!("Gagal simpan ke keyring — fallback ke database saja");
        }
    }

    // Tetap simpan ke database sebagai fallback/backup
    store::save_identity_key(conn, "node_identity", "identity", &priv_key.0)?;

    tracing::info!("Identity key baru di-generate dan disimpan");
    Ok((priv_key, pub_key))
}
