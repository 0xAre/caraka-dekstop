// src-tauri/src/tor.rs
// Tor transport (F0) — onion service hosting + dial via arti-client.
//
// Setiap instance CARAKA mendapat persistent .onion address (disimpan di
// arti state dir). Address stabil lintas restart selama state_dir preserved.
//
// CARAKA-specific: versi disederhanakan tanpa restricted discovery / client auth.
// Koneksi bersifat publik — siapapun yang punya onion address bisa dial.

use std::sync::Arc;
use std::time::Duration;

use arti_client::config::CfgPath;
use arti_client::{DataStream, TorClient, TorClientConfig};
use futures::StreamExt as _;
use safelog::DisplayRedacted as _;
use tokio::sync::{mpsc, Mutex};
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::config::OnionServiceConfigBuilder;
use tor_hsservice::{handle_rend_requests, RunningOnionService};
use tor_rtcompat::PreferredRuntime;

/// Port virtual yang digunakan saat connect ke onion peer.
pub const TOR_VIRTUAL_PORT: u16 = 9999;

/// Status transport Tor — dipegang AppState, dibaca commands + UI.
pub enum TorState {
    /// Bootstrap sedang berjalan (state awal).
    Bootstrapping,
    /// Tor siap — onion service aktif, bisa dial/accept.
    Ready(Arc<TorContext>),
    /// Bootstrap gagal; simpan alasan untuk ditampilkan ke user.
    Failed(String),
}

impl TorState {
    /// Ambil TorContext jika ready.
    pub fn context(&self) -> Option<Arc<TorContext>> {
        match self {
            TorState::Ready(ctx) => Some(ctx.clone()),
            _ => None,
        }
    }
}

/// Konteks Tor aktif — TorClient ter-bootstrap + onion service berjalan.
pub struct TorContext {
    client: Arc<TorClient<PreferredRuntime>>,
    _service: Arc<RunningOnionService>,
    pub onion_address: String,
    incoming: Mutex<mpsc::UnboundedReceiver<DataStream>>,
}

impl TorContext {
    /// Bootstrap Tor, launch onion service, mulai accept loop.
    ///
    /// Blocking sampai Tor siap (~30-60 detik pertama, < 5 detik setelahnya
    /// karena cache directory disimpan).
    pub async fn launch(
        cache_dir: &std::path::Path,
        state_dir: &std::path::Path,
    ) -> Result<Arc<Self>, String> {
        // Windows: arti memeriksa Unix-style file permissions yang tidak ada di Windows.
        #[cfg(target_os = "windows")]
        std::env::set_var("FS_MISTRUST_DISABLE_PERMISSIONS_CHECKS", "true");

        std::fs::create_dir_all(cache_dir)
            .map_err(|e| format!("Gagal buat tor cache dir: {e}"))?;
        std::fs::create_dir_all(state_dir)
            .map_err(|e| format!("Gagal buat tor state dir: {e}"))?;

        let cache_str = cache_dir.to_string_lossy().to_string();
        let state_str = state_dir.to_string_lossy().to_string();

        let mut builder = TorClientConfig::builder();
        builder
            .storage()
            .cache_dir(CfgPath::new(cache_str))
            .state_dir(CfgPath::new(state_str));
        let config = builder
            .build()
            .map_err(|e| format!("Tor config error: {e}"))?;

        tracing::info!("Memulai bootstrap Tor (30-60 detik pertama kali)...");
        let client = TorClient::create_bootstrapped(config)
            .await
            .map_err(|e| format!("Tor bootstrap gagal: {e}"))?;

        tracing::info!("Tor siap, launching onion service...");

        let svc_cfg = OnionServiceConfigBuilder::default()
            .nickname("caraka".parse().map_err(|e| format!("nickname error: {e}"))?)
            .build()
            .map_err(|e| format!("onion service config error: {e}"))?;

        let (service, rend_stream) = client
            .launch_onion_service(svc_cfg)
            .map_err(|e| format!("launch onion service gagal: {e}"))?
            .ok_or_else(|| "onion service disabled di config".to_string())?;

        let onion_address = service
            .onion_address()
            .ok_or_else(|| "alamat onion belum tersedia".to_string())?
            .display_unredacted()
            .to_string();

        tracing::info!("Onion address: {}", onion_address);

        let (in_tx, in_rx) = mpsc::unbounded_channel::<DataStream>();
        tokio::spawn(async move {
            let mut streams = Box::pin(handle_rend_requests(rend_stream));
            while let Some(req) = streams.next().await {
                match req.accept(Connected::new_empty()).await {
                    Ok(ds) => {
                        if in_tx.send(ds).is_err() {
                            break;
                        }
                    }
                    Err(_) => continue,
                }
            }
        });

        Ok(Arc::new(Self {
            client,
            _service: service,
            onion_address,
            incoming: Mutex::new(in_rx),
        }))
    }

    /// Connect ke onion peer via Tor (sebagai initiator).
    pub async fn connect(&self, onion_host: &str) -> Result<DataStream, String> {
        self.client
            .connect((onion_host.to_string(), TOR_VIRTUAL_PORT))
            .await
            .map_err(|e| format!("Tor connect gagal: {e}"))
    }

    /// Tunggu stream masuk berikutnya dengan timeout.
    /// Returns None jika timeout habis (loop lagi).
    pub async fn accept_timeout(&self, timeout: Duration) -> Option<DataStream> {
        let mut rx = self.incoming.lock().await;
        tokio::time::timeout(timeout, rx.recv()).await.ok().flatten()
    }
}
