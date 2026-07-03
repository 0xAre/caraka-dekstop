// src-tauri/src/state.rs
// Fase 6 — Shared Application State
//
// AppState diakses dari semua Tauri command handlers dan background tasks.
// Semua field mutable diproteksi dengan Arc<Mutex<...>>.
//
// Flow baru (F1 — Argon2id Vault):
//   1. pre_initialize() — cek vault, emit vault_check ke frontend
//   2. User input passphrase → create_vault / unlock_vault command
//   3. complete_initialize(private_key_bytes) — bangun AppState, start services

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
/// Diakses via `State<'_, Arc<Mutex<Option<AppState>>>>` di command handlers.
/// `None` berarti vault belum di-unlock — semua command yang butuh state
/// harus return error jika state masih None.
pub struct AppState {
    pub node_id_hex: String,
    pub my_node_id: NodePublicKey,
    /// X25519 private key — TIDAK BOLEH di-expose ke frontend!
    pub my_private_key: NodePrivateKey,
    pub db_conn: Arc<Mutex<Connection>>,
    pub router: Arc<Mutex<Router>>,
    pub display_name: String,
    pub peer_senders: PeerSenders,
    pub network_state: Arc<Mutex<NetworkState>>,
    pub hop_mac_key: [u8; 16],
    /// Status transport Tor: Bootstrapping → Ready(ctx) | Failed(alasan).
    pub tor_ctx: Arc<Mutex<crate::tor::TorState>>,
}

/// Akses AppState dari background task (transport/discovery/sync).
///
/// PENTING: managed state bertipe `Arc<Mutex<Option<AppState>>>` sejak F1.
/// `try_state::<Arc<Mutex<AppState>>>()` dengan tipe lama tetap compile
/// tapi SELALU None saat runtime — itu bug yang mematikan seluruh networking.
/// Semua akses state dari background task wajib lewat helper ini.
///
/// Closure menerima `&AppState` — ambil clone Arc field yang dibutuhkan,
/// jangan tahan lock lama. Return None jika vault belum di-unlock.
pub async fn with_state<T>(
    app_handle: &tauri::AppHandle,
    f: impl FnOnce(&AppState) -> T,
) -> Option<T> {
    let state_arc = app_handle
        .try_state::<Arc<Mutex<Option<AppState>>>>()?
        .inner()
        .clone();
    let guard = state_arc.lock().await;
    guard.as_ref().map(f)
}

/// Phase 1: Cek vault, emit status ke frontend.
///
/// Dipanggil saat startup SEBELUM user input passphrase.
/// Tidak membuat AppState — hanya emit event ke JS.
pub async fn pre_initialize(app_handle: tauri::AppHandle) {
    let vault_exists = match app_handle.path().app_data_dir() {
        Ok(dir) => crate::vault::vault_exists(&dir),
        Err(_) => false,
    };

    app_handle
        .emit(
            "vault_check",
            serde_json::json!({ "exists": vault_exists }),
        )
        .ok();
}

/// Phase 2: Bangun AppState dari private key yang sudah di-unlock dari vault.
///
/// Dipanggil setelah user berhasil buka vault (create atau unlock).
/// Mengisi managed `Arc<Mutex<Option<AppState>>>` dan start semua services.
pub async fn complete_initialize(
    app_handle: tauri::AppHandle,
    private_key_bytes: [u8; 32],
) -> anyhow::Result<()> {
    use crate::keys;
    use crate::store;

    // 1. DB path
    let db_path = app_handle.path().app_data_dir()?.join("caraka.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 2. Buka SQLite
    let conn = store::open_db(&db_path)?;

    // 3. Bangun keypair dari private key vault
    let priv_key = NodePrivateKey(private_key_bytes);
    let pub_key = keys::public_key_from_private(&priv_key);

    let node_id_hex = hex::encode(pub_key.0);
    let fingerprint = keys::fingerprint(&pub_key);

    tracing::info!(
        "CARAKA node dimulai — ID: {}... fingerprint: {}",
        &node_id_hex[..8],
        fingerprint
    );

    // 4. Load display_name
    let display_name = store::load_setting(&conn, "display_name")
        .ok()
        .flatten()
        .unwrap_or_else(|| "User".to_string());

    // 5. Router
    let router = Router::new(pub_key.clone());

    // 6. Derive hop_mac_key dari private key via HKDF-SHA256
    let hop_mac_key = {
        use hkdf::Hkdf;
        use sha2::Sha256;
        let hk = Hkdf::<Sha256>::new(Some(b"CARAKA-HOP-MAC-v1"), &priv_key.0);
        let mut key = [0u8; 16];
        hk.expand(b"hop-authentication", &mut key)
            .expect("HKDF expand untuk hop_mac_key");
        key
    };

    // 7. Channels
    let peer_senders: PeerSenders = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let network_state: Arc<Mutex<NetworkState>> = Arc::new(Mutex::new(NetworkState::Normal));

    // 7b. Tor state placeholder — diisi async setelah AppState tersimpan
    let tor_ctx: Arc<Mutex<crate::tor::TorState>> =
        Arc::new(Mutex::new(crate::tor::TorState::Bootstrapping));

    // 8. Build AppState
    let app_state = AppState {
        node_id_hex: node_id_hex.clone(),
        my_node_id: pub_key.clone(),
        my_private_key: priv_key,
        db_conn: Arc::new(Mutex::new(conn)),
        router: Arc::new(Mutex::new(router)),
        display_name,
        peer_senders: peer_senders.clone(),
        network_state: network_state.clone(),
        hop_mac_key,
        tor_ctx: tor_ctx.clone(),
    };

    // Clone Arc db untuk task bootstrap Tor (sebelum app_state di-move)
    let db_conn_for_tor = app_state.db_conn.clone();

    // 9. Set managed Option<AppState> dari None → Some
    let managed = app_handle.state::<Arc<Mutex<Option<AppState>>>>();
    {
        let mut lock = managed.lock().await;
        *lock = Some(app_state);
    }

    // 10. Start background services
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

    let handle = app_handle.clone();
    let ns = network_state.clone();
    tokio::spawn(async move {
        crate::network_monitor::start_network_monitor(handle, ns).await;
    });

    // 11. Listen connect_to_peer dari discovery
    {
        let handle = app_handle.clone();
        let senders = peer_senders.clone();
        app_handle.listen("connect_to_peer", move |event| {
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                let ip = payload["ip"].as_str().unwrap_or("").to_string();
                let port = payload["port"].as_u64().unwrap_or(7771) as u16;
                let node_id = payload["nodeId"].as_str().unwrap_or("").to_string();
                let handle2 = handle.clone();
                let senders2 = senders.clone();
                tokio::spawn(async move {
                    let node_id_hint: Option<String> =
                        if node_id.is_empty() { None } else { Some(node_id) };
                    crate::transport::connect_to_peer(
                        &ip,
                        port,
                        node_id_hint.as_deref(),
                        handle2,
                        senders2,
                    )
                    .await;
                });
            }
        });
    }

    // 12. Notify frontend node siap
    app_handle.emit(
        "node_ready",
        serde_json::json!({
            "nodeId": node_id_hex,
            "fingerprint": fingerprint,
            "tcpPort": crate::transport::effective_data_port(),
            "discoveryPort": crate::discovery::effective_discovery_port(),
        }),
    )?;

    tracing::info!(
        "CARAKA node siap! TCP:{} UDP:{}",
        crate::transport::DATA_PORT,
        crate::discovery::DISCOVERY_PORT
    );

    // 13. Bootstrap Tor di background — tidak memblokir startup
    let tor_data_dir = app_handle.path().app_data_dir()?;
    let tor_handle = app_handle.clone();
    let tor_senders = peer_senders.clone();
    let tor_db = db_conn_for_tor;
    tokio::spawn(async move {
        let cache_dir = tor_data_dir.join("tor").join("cache");
        let state_dir = tor_data_dir.join("tor").join("state");

        tor_handle
            .emit("tor_status", serde_json::json!({ "status": "bootstrapping" }))
            .ok();

        let launch_result = tokio::time::timeout(
            std::time::Duration::from_secs(180),
            crate::tor::TorContext::launch(&cache_dir, &state_dir),
        )
        .await;

        match launch_result {
            Ok(Ok(ctx)) => {
                let onion = ctx.onion_address.clone();
                *tor_ctx.lock().await = crate::tor::TorState::Ready(ctx.clone());
                tracing::info!("Tor siap → {}", onion);
                tor_handle
                    .emit(
                        "tor_status",
                        serde_json::json!({ "status": "ready", "onionAddress": onion }),
                    )
                    .ok();

                // 13a. Accept loop — proses stream masuk ke onion service kita.
                // Tanpa loop ini, koneksi dari peer menumpuk di channel dan
                // tidak pernah dibaca (bug utama F0 sebelumnya).
                {
                    let ctx_accept = ctx.clone();
                    let handle = tor_handle.clone();
                    let senders = tor_senders.clone();
                    tokio::spawn(async move {
                        tracing::info!("Tor accept loop dimulai");
                        loop {
                            match ctx_accept
                                .accept_timeout(std::time::Duration::from_secs(10))
                                .await
                            {
                                Some(stream) => {
                                    let conn_id = uuid::Uuid::new_v4().to_string();
                                    tracing::info!("Stream Tor masuk (conn {})", &conn_id[..8]);
                                    let kind = crate::transport::ConnKind::TorInbound { conn_id };
                                    tokio::spawn(crate::transport::handle_connection(
                                        stream,
                                        kind,
                                        handle.clone(),
                                        senders.clone(),
                                    ));
                                }
                                // Timeout ATAU channel tertutup — jeda singkat
                                // agar tidak busy-loop kalau service mati.
                                None => {
                                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                }
                            }
                        }
                    });
                }

                // 13b. Auto-reconnect ke peer yang punya onion address tersimpan
                let known_onion_peers: Vec<(String, String)> = {
                    let db = tor_db.lock().await;
                    crate::store::get_all_peers(&db)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|p| !p.onion_address.is_empty())
                        .map(|p| (p.node_id, p.onion_address))
                        .collect()
                };
                for (node_id, onion) in known_onion_peers {
                    tracing::info!("Auto-reconnect via Tor ke {}...", &node_id[..8.min(node_id.len())]);
                    tokio::spawn(crate::transport::connect_to_tor_peer(
                        ctx.clone(),
                        onion,
                        Some(node_id),
                        tor_handle.clone(),
                        tor_senders.clone(),
                    ));
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Tor bootstrap gagal: {}", e);
                *tor_ctx.lock().await = crate::tor::TorState::Failed(e.clone());
                tor_handle
                    .emit(
                        "tor_status",
                        serde_json::json!({ "status": "failed", "error": e }),
                    )
                    .ok();
            }
            Err(_) => {
                let msg = "Timeout: koneksi Tor diblokir firewall atau jaringan tidak mendukung";
                tracing::warn!("Tor bootstrap timeout (180 detik) — kemungkinan firewall memblokir");
                *tor_ctx.lock().await = crate::tor::TorState::Failed(msg.to_string());
                tor_handle
                    .emit(
                        "tor_status",
                        serde_json::json!({ "status": "failed", "error": msg }),
                    )
                    .ok();
            }
        }
    });

    Ok(())
}
