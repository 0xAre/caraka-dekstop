# Development Guide — CARAKA Desktop

> **Panduan Implementasi Lengkap**: dari *setup* lingkungan hingga aplikasi siap dievaluasi.
> Berdasarkan Protokol CLAMP v0.1 dan stack kriptografi Ascon (NIST SP 800-232).

---

## Daftar Isi

1. [Arsitektur Implementasi](#1-arsitektur-implementasi)
2. [Persiapan Lingkungan](#2-persiapan-lingkungan)
3. [Setup Proyek Tauri](#3-setup-proyek-tauri)
4. [Fase 1 — Modul Kriptografi](#4-fase-1--modul-kriptografi-cryptors-dan-keysrs)
5. [Fase 2 — Protokol CLAMP](#5-fase-2--protokol-clamp-packetrs)
6. [Fase 3 — Routing Engine](#6-fase-3--routing-engine-routingrs)
7. [Fase 4 — Lapisan Jaringan](#7-fase-4--lapisan-jaringan-discoveryrs--transportrs)
8. [Fase 5 — Lapisan Penyimpanan](#8-fase-5--lapisan-penyimpanan-storers--syncrs)
9. [Fase 6 — Integrasi Tauri dan GUI](#9-fase-6--integrasi-tauri-dan-gui)
10. [Fase 7 — Benchmarking dan Evaluasi](#10-fase-7--benchmarking-dan-evaluasi)
11. [Pengujian (Testing)](#11-pengujian-testing)
12. [Aturan Kode Wajib](#12-aturan-kode-wajib)
13. [Troubleshooting](#13-troubleshooting)
14. [Roadmap Pengembangan](#14-roadmap-pengembangan)

---

## 1. Arsitektur Implementasi

Sebelum menulis satu baris kode pun, pahami struktur ini. **Urutan implementasi mengikuti dependency graph** — setiap fase bergantung pada fase sebelumnya.

```
Fase 1: crypto.rs + keys.rs
    │ (semua modul lain bergantung pada ini)
    ▼
Fase 2: packet.rs
    │ (mendefinisikan format data yang mengalir di jaringan)
    ▼
Fase 3: routing.rs
    │ (logika forwarding, bergantung pada crypto + packet)
    ▼
Fase 4: discovery.rs + transport.rs
    │ (lapisan jaringan, bergantung pada packet + routing)
    ▼
Fase 5: store.rs + sync.rs
    │ (persistensi, bergantung pada crypto + packet)
    ▼
Fase 6: commands.rs + Frontend GUI
    │ (mengintegrasikan semua modul via Tauri IPC)
    ▼
Fase 7: Benchmarking + Evaluasi
    (microbenchmark Criterion.rs + network benchmark multi-node)
```

### Struktur File Final

```
src-tauri/
└── src/
    ├── main.rs          ← Entry point Tauri, setup runtime Tokio
    ├── state.rs         ← AppState (Arc<Mutex<...>>) — shared state
    ├── commands.rs      ← Tauri IPC command handlers
    │
    ├── crypto.rs        ← [FASE 1] Ascon-AEAD128, MAC, Hash, XOF
    ├── keys.rs          ← [FASE 1] X25519 keypair, ECDH, key derivation
    ├── packet.rs        ← [FASE 2] CLAMP packet struct, encode, decode
    ├── routing.rs       ← [FASE 3] Flooding, TTL, Packet Cache, Trust Score
    ├── discovery.rs     ← [FASE 4] UDP Broadcast peer discovery
    ├── transport.rs     ← [FASE 4] TCP server/client
    ├── store.rs         ← [FASE 5] SQLite: pesan, peer, kunci
    └── sync.rs          ← [FASE 5] Epidemic Sync

src/
├── index.html
├── main.ts              ← [FASE 6] Frontend entry point + event listeners
├── api/
│   └── tauri.ts         ← [FASE 6] Type-safe IPC wrappers
└── styles/
    └── main.css

benches/
└── crypto_bench.rs      ← [FASE 7] Criterion benchmarks

Cargo.toml               ← Dependencies
```

---

## 2. Persiapan Lingkungan

### 2.1 Tools yang Diperlukan

| Tool | Versi Minimum | Catatan |
|---|---|---|
| **Rust** (rustup) | stable ≥ 1.78 | Wajib: terinstall via rustup |
| **Node.js** | 20 LTS | Untuk frontend Tauri |
| **pnpm** | ≥ 9.x | Package manager frontend |
| **Tauri CLI** | v2.x | `cargo install tauri-cli` |
| **Git** | ≥ 2.40 | Version control |
| **VS Code** (opsional) | Latest | Dengan extension `rust-analyzer` |

### 2.2 Instalasi di Windows

```powershell
# Step 1: Install Rust via rustup
# Download dan jalankan: https://rustup.rs/
# Atau via winget:
winget install --id Rustlang.Rustup -e

# Step 2: Visual Studio Build Tools (WAJIB untuk linking di Windows)
winget install --id Microsoft.VisualStudio.2022.BuildTools -e `
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# Step 3: WebView2 Runtime (untuk Tauri window)
winget install --id Microsoft.EdgeWebView2Runtime -e

# Step 4: Node.js
winget install --id OpenJS.NodeJS.LTS -e

# Step 5: pnpm
npm install -g pnpm

# Restart terminal, lalu verifikasi:
rustc --version     # rustc 1.xx.x (...)
cargo --version     # cargo 1.xx.x (...)
node --version      # v20.x.x
pnpm --version      # 9.x.x
```

### 2.3 Instalasi di Linux (Ubuntu/Debian)

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.bashrc

# System dependencies untuk Tauri
sudo apt update
sudo apt install -y build-essential libssl-dev pkg-config \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev \
  patchelf libsqlite3-dev

# Node.js via NodeSource
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt install -y nodejs

# pnpm
npm install -g pnpm
```

### 2.4 Verifikasi Instalasi

```bash
rustc --version
cargo --version
node --version
pnpm --version

# Test bahwa Rust dapat compile Tauri
cargo install tauri-cli
cargo tauri --version
```

---

## 3. Setup Proyek Tauri

### 3.1 Buat Proyek Baru

```bash
# Di dalam folder project kamu
cd "e:\Project APP\CARAKA-DEKSTOP"

# Inisialisasi proyek Tauri v2
cargo tauri init

# Ketika ditanya:
# App name: caraka-desktop
# Window title: CARAKA Desktop
# Web assets location: ../src
# Dev server URL: http://localhost:5173
# Frontend dev command: pnpm dev
# Frontend build command: pnpm build
```

### 3.2 Struktur Setelah Init

```
caraka-desktop/
├── src-tauri/
│   ├── src/
│   │   └── main.rs      ← Akan kita modifikasi
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── build.rs
├── src/
│   └── index.html       ← Akan kita kembangkan
└── package.json
```

### 3.3 Setup `Cargo.toml` — Semua Dependensi

Buka `src-tauri/Cargo.toml` dan ganti konten `[dependencies]` dengan:

```toml
[package]
name = "caraka-desktop"
version = "0.1.0"
edition = "2021"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
# ── KRIPTOGRAFI ─────────────────────────────────────────────
# Ascon-AEAD128: enkripsi E2EE payload (NIST SP 800-232)
ascon-aead = { version = "0.4", features = ["ascon128"] }
# Ascon permutation: untuk Hash256, XOF128, MAC
ascon = "0.4"
# X25519: key exchange ECDH untuk identitas node dan DM-Key
x25519-dalek = { version = "2", features = ["static_secrets"] }
# HKDF: Key Derivation Function backup (RFC 5869)
hkdf = "0.12"
sha2 = "0.10"
# Cryptographically secure random number generator
rand = { version = "0.8", features = ["std"] }
# Automatic zeroing of key material from memory when dropped
zeroize = { version = "1", features = ["derive"] }

# ── JARINGAN ────────────────────────────────────────────────
tokio = { version = "1", features = ["full"] }

# ── STORAGE ─────────────────────────────────────────────────
# SQLite — bundled agar portable tanpa install sqlite terpisah
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bincode = "1"

# ── UTILITIES ───────────────────────────────────────────────
# LRU Cache untuk Packet ID deduplication (replay protection)
lru = "0.12"
# Error type derivation yang ergonomis
thiserror = "1"
# Structured logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# ── TAURI ───────────────────────────────────────────────────
tauri = { version = "2", features = [] }

[dev-dependencies]
# Benchmarking framework (untuk Fase 7)
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "crypto_bench"
harness = false
```

### 3.4 Setup `main.rs` — Entry Point

```rust
// src-tauri/src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod crypto;
mod discovery;
mod keys;
mod packet;
mod routing;
mod state;
mod store;
mod sync;
mod transport;

use tauri::Manager;

#[tokio::main]
async fn main() {
    // Aktifkan logging. Set RUST_LOG=debug untuk verbose.
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "caraka_desktop=info".into())
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            // Inisialisasi AppState saat aplikasi pertama kali dibuka
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                state::initialize(app_handle).await
                    .expect("Gagal menginisialisasi CARAKA node");
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::init_node,
            commands::send_dm,
            commands::get_messages,
            commands::get_peers,
            commands::add_peer_manual,
            commands::get_network_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CARAKA Desktop");
}
```

### 3.5 Setup `state.rs` — Shared Application State

```rust
// src-tauri/src/state.rs
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AppState {
    pub my_node_id: keys::NodePublicKey,
    pub my_private_key: keys::NodePrivateKey,
    pub db_conn: Arc<Mutex<rusqlite::Connection>>,
    pub router: Arc<Mutex<routing::Router>>,
    pub display_name: String,
}

pub async fn initialize(app_handle: tauri::AppHandle) -> anyhow::Result<()> {
    // 1. Load atau generate identity keypair
    let (private_key, public_key) = keys::load_or_generate_identity()?;

    // 2. Buka/buat database SQLite
    let db_path = app_handle.path().app_data_dir()?.join("caraka.db");
    let conn = store::open_db(&db_path)?;

    // 3. Inisialisasi Router
    let router = routing::Router::new(public_key.clone());

    // 4. Simpan ke AppState
    let state = AppState {
        my_node_id: public_key,
        my_private_key: private_key,
        db_conn: Arc::new(Mutex::new(conn)),
        router: Arc::new(Mutex::new(router)),
        display_name: String::from("User"),
    };

    app_handle.manage(Arc::new(Mutex::new(state)));

    // 5. Mulai background tasks (discovery + TCP listener)
    let handle = app_handle.clone();
    tokio::spawn(async move { discovery::start_broadcaster(handle).await });
    let handle = app_handle.clone();
    tokio::spawn(async move { transport::start_tcp_server(handle).await });

    // 6. Beritahu frontend bahwa node siap
    app_handle.emit("node_ready", serde_json::json!({
        "nodeId": hex::encode(public_key.0),
        "fingerprint": keys::fingerprint(&public_key)
    }))?;

    Ok(())
}
```

---

## 4. Fase 1 — Modul Kriptografi (`crypto.rs` dan `keys.rs`)

> **Tujuan Fase 1:** Implementasi seluruh primitif kriptografi yang akan digunakan oleh semua modul lain.
> Fase ini adalah fondasi — jangan lanjut ke Fase 2 sebelum semua test di sini lulus.

### Konteks dari Research Report

Berdasarkan analisis LWC finalis NIST:
- **Ascon** dipilih karena: standar NIST aktif, keamanan 128-bit terverifikasi, satu keluarga untuk AEAD+Hash+XOF, dukungan Rust terbaik
- **TinyJambu**: ditolak — birthday-bound slide attacks, margin keamanan tipis
- **GIFT-COFB**: ditolak — effective 64-bit tag dalam skenario high-forgery
- **X25519**: digunakan untuk key exchange (LWC tidak menstandardisasi skema asimetrik)

### 4.1 Implementasi `crypto.rs`

```rust
// src-tauri/src/crypto.rs
use ascon_aead::{Ascon128, Key, Nonce as AsconNonce, aead::{Aead, AeadInPlace, KeyInit}};
use rand::rngs::OsRng;
use rand::RngCore;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

// ─── Error Types ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("AEAD decryption failed: authentication tag mismatch")]
    DecryptionFailed,
    #[error("Timestamp out of validity window (±300 seconds)")]
    TimestampInvalid,
    #[error("Invalid nonce length")]
    InvalidNonce,
}

// ─── Key Types ─────────────────────────────────────────────────────────────

/// Kunci 128-bit untuk Ascon-AEAD128
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct AeadKey(pub [u8; 16]);

/// Kunci 128-bit untuk Ascon-MAC
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MacKey(pub [u8; 16]);

/// Nonce 128-bit untuk Ascon-AEAD128
/// Format: timestamp_u32_LE[0..4] || OsRng[4..16]
#[derive(Clone)]
pub struct Nonce(pub [u8; 16]);

// ─── Nonce Generation ──────────────────────────────────────────────────────

/// Buat nonce baru yang SELALU unik per enkripsi.
/// Embed timestamp untuk validasi replay protection.
pub fn generate_nonce() -> Nonce {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;

    let mut nonce = [0u8; 16];
    nonce[0..4].copy_from_slice(&ts.to_le_bytes());
    OsRng.fill_bytes(&mut nonce[4..16]); // 12 byte random
    Nonce(nonce)
}

/// Validasi bahwa timestamp yang di-embed dalam nonce masih dalam jendela ±300 detik
pub fn validate_nonce_timestamp(nonce: &Nonce) -> Result<(), CryptoError> {
    let embedded = u32::from_le_bytes(nonce.0[0..4].try_into().unwrap()) as u64;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if now.abs_diff(embedded) > 300 {
        return Err(CryptoError::TimestampInvalid);
    }
    Ok(())
}

// ─── AEAD Encryption ───────────────────────────────────────────────────────

/// Enkripsi payload dengan Ascon-AEAD128.
///
/// `aad` = associated data (CLAMP header bytes) — diautentikasi tapi tidak dienkripsi.
/// Return: (ciphertext, auth_tag)
pub fn encrypt(
    key: &AeadKey,
    nonce: &Nonce,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, [u8; 16]), CryptoError> {
    let ascon_key = Key::<Ascon128>::from_slice(&key.0);
    let ascon_nonce = AsconNonce::<Ascon128>::from_slice(&nonce.0);
    let cipher = Ascon128::new(ascon_key);

    // ascon_aead menghasilkan ciphertext + appended 16-byte tag
    let mut ct_with_tag = cipher
        .encrypt(ascon_nonce, ascon_aead::aead::Payload { msg: plaintext, aad })
        .map_err(|_| CryptoError::DecryptionFailed)?;

    // Pisahkan tag dari ciphertext
    let tag_start = ct_with_tag.len() - 16;
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&ct_with_tag[tag_start..]);
    ct_with_tag.truncate(tag_start);

    Ok((ct_with_tag, tag))
}

/// Dekripsi payload dengan Ascon-AEAD128.
/// Gagal (CryptoError::DecryptionFailed) jika tag tidak cocok.
pub fn decrypt(
    key: &AeadKey,
    nonce: &Nonce,
    ciphertext: &[u8],
    tag: &[u8; 16],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    validate_nonce_timestamp(nonce)?;

    let ascon_key = Key::<Ascon128>::from_slice(&key.0);
    let ascon_nonce = AsconNonce::<Ascon128>::from_slice(&nonce.0);
    let cipher = Ascon128::new(ascon_key);

    // Gabungkan ciphertext + tag untuk Ascon
    let mut ct_with_tag = ciphertext.to_vec();
    ct_with_tag.extend_from_slice(tag);

    cipher
        .decrypt(ascon_nonce, ascon_aead::aead::Payload { msg: &ct_with_tag, aad })
        .map_err(|_| CryptoError::DecryptionFailed)
}

// ─── Ascon-MAC ─────────────────────────────────────────────────────────────

/// Hitung Ascon-MAC untuk autentikasi per-hop relay.
///
/// mac_input = packet_id(8B) || hop_counter(1B) || relay_node_prefix(4B)
/// = 13 byte total
pub fn compute_mac(key: &MacKey, data: &[u8]) -> [u8; 16] {
    // Implementasi Ascon-MAC menggunakan Ascon permutation dalam keyed-sponge mode.
    // Karena ascon crate mungkin belum expose MAC secara langsung,
    // gunakan HKDF-HMAC sebagai fallback yang kuat.
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(Some(&key.0), data);
    let mut tag = [0u8; 16];
    hk.expand(b"CARAKA-MAC-v1", &mut tag).unwrap();
    tag
}

pub fn verify_mac(key: &MacKey, data: &[u8], expected: &[u8; 16]) -> bool {
    let computed = compute_mac(key, data);
    // Constant-time comparison untuk mencegah timing attack
    computed.iter().zip(expected.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

// ─── Ascon-Hash256 ─────────────────────────────────────────────────────────

/// Hitung Ascon-Hash256 dari data.
/// Digunakan untuk: Message Fingerprint dalam Epidemic Sync
pub fn hash256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    // Gunakan SHA-256 sebagai proxy hingga ascon-hash crate stabil
    // TODO: Ganti dengan Ascon-Hash256 native saat crate tersedia
    let mut hasher = Sha256::new();
    hasher.update(b"CARAKA-HASH-v1"); // domain separation
    hasher.update(data);
    hasher.finalize().into()
}

// ─── Ascon-XOF128 (Key Derivation) ────────────────────────────────────────

/// Turunkan kunci dari secret dan context menggunakan HKDF (sebagai proxy Ascon-XOF128).
/// Output panjang `output.len()` byte.
pub fn xof_derive(secret: &[u8], context: &[u8], output: &mut [u8]) {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(None, secret);
    hk.expand(context, output).expect("HKDF expand gagal — output terlalu panjang");
}
```

### 4.2 Implementasi `keys.rs`

```rust
// src-tauri/src/keys.rs
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};
use crate::crypto::{AeadKey, MacKey};

// ─── Tipe Kunci ─────────────────────────────────────────────────────────────

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct NodePrivateKey(pub [u8; 32]);

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct NodePublicKey(pub [u8; 32]);

impl NodePublicKey {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

// ─── Identitas Node ─────────────────────────────────────────────────────────

/// Generate keypair X25519 baru menggunakan OsRng (cryptographically secure).
pub fn generate_keypair() -> (NodePrivateKey, NodePublicKey) {
    use rand::rngs::OsRng;
    let private = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&private);
    (
        NodePrivateKey(private.to_bytes()),
        NodePublicKey(public.to_bytes()),
    )
}

/// Load keypair dari penyimpanan lokal, atau generate baru jika belum ada.
pub fn load_or_generate_identity() -> Result<(NodePrivateKey, NodePublicKey), anyhow::Error> {
    // TODO: Fase 5 — simpan ke/baca dari SQLite key store
    // Untuk sementara: generate fresh setiap kali (tidak persistent)
    Ok(generate_keypair())
}

/// Fingerprint node: 8 karakter hex pertama dari hash public key.
/// Digunakan untuk verifikasi out-of-band (verbal atau QR code).
pub fn fingerprint(public_key: &NodePublicKey) -> String {
    let hash = crate::crypto::hash256(&public_key.0);
    hex::encode(&hash[0..4]) // 8 hex chars
}

// ─── Key Exchange (ECDH) ────────────────────────────────────────────────────

/// Hitung Shared Secret menggunakan X25519 ECDH.
///
/// ECDH bersifat commutative:
/// X25519(alice_private, bob_public) == X25519(bob_private, alice_public)
pub fn ecdh(my_private: &NodePrivateKey, peer_public: &NodePublicKey) -> [u8; 32] {
    let private = StaticSecret::from(my_private.0);
    let public = PublicKey::from(peer_public.0);
    let shared = private.diffie_hellman(&public);
    shared.to_bytes()
}

// ─── Key Derivation ─────────────────────────────────────────────────────────

/// Turunkan DM-Key (AeadKey) untuk enkripsi Direct Message.
///
/// Kunci berubah setiap pesan (msg_counter++) → simple forward secrecy.
/// Context string berbeda untuk sender dan receiver mencegah key reuse.
pub fn derive_dm_key(
    shared_secret: &[u8; 32],
    my_id: &NodePublicKey,
    peer_id: &NodePublicKey,
    session_id: &[u8; 8],
    msg_counter: u64,
    is_sender: bool,
) -> AeadKey {
    let direction = if is_sender { b"CARAKA-DM-SEND-v1" as &[u8] }
                    else          { b"CARAKA-DM-RECV-v1" };

    let mut context = Vec::with_capacity(17 + 32 + 32 + 8 + 8);
    context.extend_from_slice(direction);
    context.extend_from_slice(&my_id.0);
    context.extend_from_slice(&peer_id.0);
    context.extend_from_slice(session_id);
    context.extend_from_slice(&msg_counter.to_le_bytes());

    let mut key_bytes = [0u8; 16];
    crate::crypto::xof_derive(shared_secret, &context, &mut key_bytes);
    AeadKey(key_bytes)
}

/// Turunkan Channel MAC-Key untuk autentikasi per-hop relay.
///
/// Kunci ini DIBAGIKAN ke semua anggota channel (out-of-band).
/// Bukan untuk enkripsi payload — hanya untuk relay authentication.
pub fn derive_channel_mac_key(master_secret: &[u8], channel_id: &[u8; 8]) -> MacKey {
    let mut context = Vec::with_capacity(18 + 8);
    context.extend_from_slice(b"CARAKA-CH-MAC-v1");
    context.extend_from_slice(channel_id);

    let mut key_bytes = [0u8; 16];
    crate::crypto::xof_derive(master_secret, &context, &mut key_bytes);
    MacKey(key_bytes)
}
```

### 4.3 Unit Tests Fase 1

```rust
// Di bagian bawah crypto.rs, tambahkan:
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = AeadKey([42u8; 16]);
        let nonce = generate_nonce();
        let plaintext = b"Hello, CARAKA!";
        let aad = b"test-header";

        let (ct, tag) = encrypt(&key, &nonce, plaintext, aad).unwrap();
        let pt = decrypt(&key, &nonce, &ct, &tag, aad).unwrap();

        assert_eq!(pt, plaintext);
    }

    #[test]
    fn test_decrypt_fails_on_tampered_ciphertext() {
        let key = AeadKey([42u8; 16]);
        let nonce = generate_nonce();
        let plaintext = b"Secret message";

        let (mut ct, tag) = encrypt(&key, &nonce, plaintext, b"").unwrap();
        ct[0] ^= 0xFF; // Tamper dengan ciphertext

        // Harus gagal
        assert!(decrypt(&key, &nonce, &ct, &tag, b"").is_err());
    }

    #[test]
    fn test_mac_verify() {
        let key = MacKey([99u8; 16]);
        let data = b"packet_id || hop_counter || relay_prefix";

        let tag = compute_mac(&key, data);
        assert!(verify_mac(&key, data, &tag));

        let mut bad_tag = tag;
        bad_tag[0] ^= 0x01;
        assert!(!verify_mac(&key, data, &bad_tag));
    }

    #[test]
    fn test_ecdh_key_agreement() {
        let (alice_priv, alice_pub) = keys::generate_keypair();
        let (bob_priv, bob_pub) = keys::generate_keypair();

        let alice_shared = keys::ecdh(&alice_priv, &bob_pub);
        let bob_shared = keys::ecdh(&bob_priv, &alice_pub);

        // ECDH commutative property
        assert_eq!(alice_shared, bob_shared);
    }

    #[test]
    fn test_dm_key_derivation_asymmetric() {
        let (alice_priv, alice_pub) = keys::generate_keypair();
        let (bob_priv, bob_pub) = keys::generate_keypair();
        let session_id = [0u8; 8];
        let msg_counter = 0u64;

        let shared_alice = keys::ecdh(&alice_priv, &bob_pub);
        let shared_bob = keys::ecdh(&bob_priv, &alice_pub);

        let alice_send = keys::derive_dm_key(&shared_alice, &alice_pub, &bob_pub,
                                              &session_id, msg_counter, true);
        let bob_recv = keys::derive_dm_key(&shared_bob, &alice_pub, &bob_pub,
                                            &session_id, msg_counter, false);

        // Alice.send_key == Bob.recv_key (simetris via ECDH)
        assert_eq!(alice_send.0, bob_recv.0);
    }
}
```

### ✅ Checklist Fase 1

```
- [ ] cargo add berhasil: ascon-aead, x25519-dalek, hkdf, sha2, rand, zeroize
- [ ] crypto.rs: encrypt/decrypt tersedia dan terkompilasi
- [ ] crypto.rs: compute_mac/verify_mac tersedia
- [ ] crypto.rs: hash256 dan xof_derive tersedia
- [ ] keys.rs: generate_keypair() menggunakan OsRng
- [ ] keys.rs: ecdh() tersedia
- [ ] keys.rs: derive_dm_key() dengan context string berbeda untuk sender/receiver
- [ ] cargo test -p caraka-desktop -- crypto::tests → semua PASS
- [ ] cargo test -p caraka-desktop -- keys::tests → semua PASS
```

---

## 5. Fase 2 — Protokol CLAMP (`packet.rs`)

> **Tujuan Fase 2:** Definisikan format paket biner CLAMP dan implementasikan encode/decode.

### 5.1 Struktur Paket

```
Offset  Size  Field           Nilai / Deskripsi
──────  ────  ─────────────  ──────────────────────────────────────
  0       2   magic           0xCA, 0x52
  2       1   version         0x01
  3       1   packet_type     0x01=DM | 0x02=Channel | 0x03=SyncReq
                              0x04=SyncResp | 0x05=Hello | 0x06=SyncData
  4       1   ttl             0–7, dikurangi setiap relay
  5       8   packet_id       origin_node_id[0..4] || OsRng[4..8]
─────────────────────────────────── (13 byte Header)
 13       1   hop_counter     0 saat origin, +1 tiap relay
 14      16   hop_mac_tag     Ascon-MAC(ch_key, pkt_id||hop_ctr||relay_id[0..4])
─────────────────────────────────── (17 byte HopAuth)
 30      16   nonce           timestamp_u32[0..4] || OsRng[4..16]
 46       N   ciphertext      Ascon-AEAD128 encrypted inner payload
 46+N    16   aead_tag        Authentication tag
─────────────────────────────────── (Total fixed overhead: 62 byte)
```

### 5.2 Implementasi `packet.rs`

```rust
// src-tauri/src/packet.rs
use rand::rngs::OsRng;
use rand::RngCore;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PacketError {
    #[error("Invalid magic bytes")]
    InvalidMagic,
    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(u8),
    #[error("Packet too short: expected >= {expected}, got {got}")]
    TooShort { expected: usize, got: usize },
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

pub const MAGIC: [u8; 2] = [0xCA, 0x52];
pub const PROTOCOL_VERSION: u8 = 0x01;
pub const TTL_MAX: u8 = 7;
pub const HEADER_SIZE: usize = 13;
pub const HOP_AUTH_SIZE: usize = 17;
pub const NONCE_SIZE: usize = 16;
pub const TAG_SIZE: usize = 16;
pub const FIXED_OVERHEAD: usize = HEADER_SIZE + HOP_AUTH_SIZE + NONCE_SIZE + TAG_SIZE; // 62

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PacketType {
    Dm = 0x01,
    Channel = 0x02,
    SyncReq = 0x03,
    SyncResp = 0x04,
    Hello = 0x05,
    SyncData = 0x06,
}

impl TryFrom<u8> for PacketType {
    type Error = PacketError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0x01 => Ok(PacketType::Dm),
            0x02 => Ok(PacketType::Channel),
            0x03 => Ok(PacketType::SyncReq),
            0x04 => Ok(PacketType::SyncResp),
            0x05 => Ok(PacketType::Hello),
            0x06 => Ok(PacketType::SyncData),
            _ => Err(PacketError::InvalidMagic),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClampHeader {
    pub magic: [u8; 2],
    pub version: u8,
    pub packet_type: PacketType,
    pub ttl: u8,
    pub packet_id: [u8; 8],
}

#[derive(Debug, Clone)]
pub struct HopAuth {
    pub hop_counter: u8,
    pub mac_tag: [u8; 16],
}

#[derive(Debug, Clone)]
pub struct ClampPacket {
    pub header: ClampHeader,
    pub hop_auth: HopAuth,
    pub nonce: [u8; 16],
    pub ciphertext: Vec<u8>,
    pub aead_tag: [u8; 16],
}

impl ClampPacket {
    /// Buat Packet ID baru: origin_node_id[0..4] + random[4..8]
    pub fn generate_packet_id(origin_node_id: &[u8; 32]) -> [u8; 8] {
        let mut id = [0u8; 8];
        id[0..4].copy_from_slice(&origin_node_id[0..4]);
        OsRng.fill_bytes(&mut id[4..8]);
        id
    }

    /// Serialize header ke bytes (13 byte) — digunakan sebagai Associated Data untuk AEAD
    pub fn header_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..2].copy_from_slice(&self.header.magic);
        buf[2] = self.header.version;
        buf[3] = self.header.packet_type as u8;
        buf[4] = self.header.ttl;
        buf[5..13].copy_from_slice(&self.header.packet_id);
        buf
    }

    /// Encode seluruh paket ke bytes untuk transmisi via TCP
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(FIXED_OVERHEAD + self.ciphertext.len());
        // Header (13B)
        buf.extend_from_slice(&self.header_bytes());
        // HopAuth (17B)
        buf.push(self.hop_auth.hop_counter);
        buf.extend_from_slice(&self.hop_auth.mac_tag);
        // Encrypted Payload
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&self.ciphertext);
        buf.extend_from_slice(&self.aead_tag);
        buf
    }

    /// Decode bytes menjadi ClampPacket
    pub fn decode(bytes: &[u8]) -> Result<Self, PacketError> {
        if bytes.len() < FIXED_OVERHEAD {
            return Err(PacketError::TooShort {
                expected: FIXED_OVERHEAD,
                got: bytes.len(),
            });
        }

        if bytes[0..2] != MAGIC {
            return Err(PacketError::InvalidMagic);
        }

        let version = bytes[2];
        if version != PROTOCOL_VERSION {
            return Err(PacketError::UnsupportedVersion(version));
        }

        let packet_type = PacketType::try_from(bytes[3])?;
        let ttl = bytes[4];
        let mut packet_id = [0u8; 8];
        packet_id.copy_from_slice(&bytes[5..13]);

        let hop_counter = bytes[13];
        let mut mac_tag = [0u8; 16];
        mac_tag.copy_from_slice(&bytes[14..30]);

        let mut nonce = [0u8; 16];
        nonce.copy_from_slice(&bytes[30..46]);

        // Pisahkan ciphertext dan AEAD tag
        let end = bytes.len();
        if end < 46 + 16 {
            return Err(PacketError::TooShort { expected: 46 + 16, got: end });
        }
        let tag_start = end - 16;
        let ciphertext = bytes[46..tag_start].to_vec();
        let mut aead_tag = [0u8; 16];
        aead_tag.copy_from_slice(&bytes[tag_start..end]);

        Ok(ClampPacket {
            header: ClampHeader { magic: MAGIC, version, packet_type, ttl, packet_id },
            hop_auth: HopAuth { hop_counter, mac_tag },
            nonce,
            ciphertext,
            aead_tag,
        })
    }
}
```

### ✅ Checklist Fase 2

```
- [ ] PacketType enum dengan semua 6 tipe tersedia
- [ ] ClampPacket::encode() → decode() roundtrip identik
- [ ] header_bytes() menghasilkan tepat 13 byte
- [ ] Decode mengembalikan error jika magic bytes salah
- [ ] Decode mengembalikan error jika bytes terlalu pendek
- [ ] cargo test -- packet::tests → semua PASS
```

---

## 6. Fase 3 — Routing Engine (`routing.rs`)

> **Tujuan Fase 3:** Implementasi Controlled Flooding, replay protection, dan Trust Score.

```rust
// src-tauri/src/routing.rs
use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use crate::crypto::{MacKey, verify_mac};
use crate::keys::NodePublicKey;
use crate::packet::{ClampPacket, HEADER_SIZE};

const CACHE_SIZE: usize = 512;
const TRUST_INITIAL: f32 = 1.0;
const TRUST_THRESHOLD: f32 = 0.5;
const TRUST_VALID_DELTA: f32 = 0.01;
const TRUST_INVALID_MAC_DELTA: f32 = -0.5;
const TRUST_RATE_LIMIT_DELTA: f32 = -1.0;

pub enum RoutingDecision {
    DeliverToApp,          // Paket untuk node ini — kirim ke UI
    Relay,                 // Teruskan ke peers lain (TTL sudah dikurangi)
    Drop(DropReason),      // Buang paket
}

pub enum DropReason {
    InvalidMagic,
    DuplicatePacket,       // Replay protection hit
    InvalidHopMac,
    TtlExpired,
    TimestampInvalid,
    PeerUntrusted,
}

pub struct Router {
    packet_cache: LruCache<[u8; 8], ()>,
    trust_scores: HashMap<NodePublicKey, f32>,
    channel_mac_key: MacKey,
    my_node_id: NodePublicKey,
}

impl Router {
    pub fn new(my_node_id: NodePublicKey) -> Self {
        Router {
            packet_cache: LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap()),
            trust_scores: HashMap::new(),
            // Default channel key — dalam implementasi nyata, dikonfigurasi user
            channel_mac_key: MacKey([0u8; 16]),
            my_node_id,
        }
    }

    /// Proses paket yang masuk. Return RoutingDecision.
    pub fn handle_incoming(
        &mut self,
        packet: &mut ClampPacket,
        source: &NodePublicKey,
    ) -> RoutingDecision {
        // 1. Cek trust score source peer
        let trust = self.trust_scores.get(source).copied().unwrap_or(TRUST_INITIAL);
        if trust < TRUST_THRESHOLD {
            return RoutingDecision::Drop(DropReason::PeerUntrusted);
        }

        // 2. Replay protection — cek Packet ID di cache
        if self.packet_cache.contains(&packet.header.packet_id) {
            return RoutingDecision::Drop(DropReason::DuplicatePacket);
        }

        // 3. Validasi Hop-MAC
        if !self.verify_hop_mac(packet, source) {
            self.update_trust(source, TRUST_INVALID_MAC_DELTA);
            return RoutingDecision::Drop(DropReason::InvalidHopMac);
        }

        // 4. Tambah ke packet cache (setelah validasi berhasil)
        self.packet_cache.put(packet.header.packet_id, ());
        self.update_trust(source, TRUST_VALID_DELTA);

        // 5. Cek apakah paket untuk node ini (coba dekripsi akan terjadi di commands.rs)
        // Untuk routing: kita tidak tahu apakah paket untuk kita tanpa mencoba dekripsi
        // Decision ini akan di-return sebagai DeliverToApp, dan jika dekripsi gagal
        // maka akan di-relay jika TTL masih ada

        // 6. Cek TTL untuk relay
        if packet.header.ttl == 0 {
            // Masih bisa deliver jika untuk kita, tapi tidak bisa relay
            return RoutingDecision::DeliverToApp;
        }

        // 7. Persiapkan untuk relay: update TTL dan Hop-MAC
        packet.header.ttl -= 1;
        packet.hop_auth.hop_counter += 1;
        packet.hop_auth.mac_tag = self.compute_relay_mac(packet);

        RoutingDecision::Relay
    }

    fn verify_hop_mac(&self, packet: &ClampPacket, source: &NodePublicKey) -> bool {
        let mac_input = self.build_mac_input(
            &packet.header.packet_id,
            packet.hop_auth.hop_counter,
            &source.0,
        );
        verify_mac(&self.channel_mac_key, &mac_input, &packet.hop_auth.mac_tag)
    }

    fn compute_relay_mac(&self, packet: &ClampPacket) -> [u8; 16] {
        let mac_input = self.build_mac_input(
            &packet.header.packet_id,
            packet.hop_auth.hop_counter,
            &self.my_node_id.0,
        );
        crate::crypto::compute_mac(&self.channel_mac_key, &mac_input)
    }

    fn build_mac_input(&self, packet_id: &[u8; 8], hop_counter: u8, node_id: &[u8; 32]) -> Vec<u8> {
        let mut input = Vec::with_capacity(13);
        input.extend_from_slice(packet_id);        // 8 byte
        input.push(hop_counter);                   // 1 byte
        input.extend_from_slice(&node_id[0..4]);   // 4 byte
        input
    }

    pub fn update_trust(&mut self, peer: &NodePublicKey, delta: f32) {
        let score = self.trust_scores.entry(peer.clone()).or_insert(TRUST_INITIAL);
        *score = (*score + delta).clamp(0.0, 5.0);
    }

    pub fn get_trust(&self, peer: &NodePublicKey) -> f32 {
        self.trust_scores.get(peer).copied().unwrap_or(TRUST_INITIAL)
    }
}
```

### ✅ Checklist Fase 3

```
- [ ] Packet ID cache bekerja — duplicate packet di-drop
- [ ] Hop-MAC verification bekerja (valid → OK, invalid → drop + trust -0.5)
- [ ] TTL decrement bekerja
- [ ] Trust Score update bekerja (clamp ke [0.0, 5.0])
- [ ] Peer dengan trust < 0.5 di-drop
- [ ] cargo test -- routing::tests → semua PASS
```

---

## 7. Fase 4 — Lapisan Jaringan (`discovery.rs` + `transport.rs`)

> **Tujuan Fase 4:** Dua node bisa saling menemukan dan bertukar paket CLAMP.

### 7.1 `discovery.rs` — UDP Broadcast

```rust
// src-tauri/src/discovery.rs
use std::net::{UdpSocket, SocketAddr};
use serde::{Serialize, Deserialize};

pub const DISCOVERY_PORT: u16 = 7770;
pub const BEACON_INTERVAL_SEC: u64 = 30;

/// Kirim UDP beacon setiap BEACON_INTERVAL_SEC detik
pub async fn start_broadcaster(app_handle: tauri::AppHandle) {
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    socket.set_broadcast(true).unwrap();

    loop {
        let beacon = build_beacon(&app_handle);
        let target: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT).parse().unwrap();
        let _ = socket.send_to(&beacon, target);

        tokio::time::sleep(std::time::Duration::from_secs(BEACON_INTERVAL_SEC)).await;
    }
}

/// Listen untuk beacon dari peer lain
pub async fn start_listener(app_handle: tauri::AppHandle) {
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).unwrap();
    let mut buf = [0u8; 1024];

    loop {
        if let Ok((len, src_addr)) = socket.recv_from(&mut buf) {
            if let Ok(peer) = parse_beacon(&buf[..len]) {
                // Simpan ke peer table
                // Coba connect TCP ke peer
                app_handle.emit("peer_discovered", serde_json::json!({
                    "nodeId": peer.node_id_hex,
                    "displayName": peer.display_name,
                    "ip": src_addr.ip().to_string()
                })).ok();
            }
        }
    }
}
```

### 7.2 `transport.rs` — TCP

```rust
// src-tauri/src/transport.rs
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const DATA_PORT: u16 = 7771;

/// Start TCP server — listen untuk koneksi masuk
pub async fn start_tcp_server(app_handle: tauri::AppHandle) {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", DATA_PORT))
        .await.unwrap();

    loop {
        if let Ok((stream, addr)) = listener.accept().await {
            let handle = app_handle.clone();
            tokio::spawn(async move {
                handle_connection(stream, addr, handle).await;
            });
        }
    }
}

/// Kirim CLAMP packet via TCP dengan 2-byte length prefix framing
pub async fn send_packet(stream: &mut TcpStream, packet_bytes: &[u8]) -> anyhow::Result<()> {
    let len = packet_bytes.len() as u16;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(packet_bytes).await?;
    Ok(())
}

/// Terima satu CLAMP packet dari TCP stream
async fn recv_packet(stream: &mut TcpStream) -> anyhow::Result<Vec<u8>> {
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_le_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn handle_connection(mut stream: TcpStream, addr: std::net::SocketAddr,
                           app_handle: tauri::AppHandle) {
    loop {
        match recv_packet(&mut stream).await {
            Ok(raw) => {
                // Proses paket melalui Router
                // TODO: akses AppState dan panggil router.handle_incoming()
            }
            Err(_) => break, // Koneksi terputus
        }
    }
}
```

### ✅ Checklist Fase 4

```
- [ ] UDP broadcast terkirim setiap 30 detik (verifikasi dengan Wireshark)
- [ ] Node B menerima beacon dari Node A dan muncul di peer list
- [ ] TCP koneksi berhasil dibangun antara dua node
- [ ] Paket CLAMP berhasil dikirim dan diterima (encode → TCP → decode)
- [ ] Test end-to-end: Node A kirim HELLO → Node B terima dan proses
```

---

## 8. Fase 5 — Lapisan Penyimpanan (`store.rs` + `sync.rs`)

> **Tujuan Fase 5:** Pesan tersimpan lokal dan bisa disinkronkan antar node yang offline.

### 8.1 `store.rs` — SQLite

```rust
// src-tauri/src/store.rs
use rusqlite::{Connection, params};
use std::path::Path;

pub fn open_db(path: &Path) -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(path)?;
    create_tables(&conn)?;
    Ok(conn)
}

fn create_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch("
        PRAGMA journal_mode=WAL;

        CREATE TABLE IF NOT EXISTS messages (
            id           TEXT PRIMARY KEY,
            packet_id    TEXT NOT NULL UNIQUE,
            sender_id    TEXT NOT NULL,
            recipient_id TEXT NOT NULL,
            nonce        BLOB NOT NULL,
            ciphertext   BLOB NOT NULL,
            aead_tag     BLOB NOT NULL,
            received_at  INTEGER NOT NULL,
            delivered    INTEGER DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS peers (
            node_id      TEXT PRIMARY KEY,
            display_name TEXT,
            last_seen    INTEGER,
            ip_address   TEXT,
            tcp_port     INTEGER,
            trust_score  REAL DEFAULT 1.0
        );

        CREATE TABLE IF NOT EXISTS local_keys (
            key_id       TEXT PRIMARY KEY,
            key_type     TEXT NOT NULL,
            key_material BLOB NOT NULL,
            created_at   INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sync_state (
            peer_id    TEXT NOT NULL,
            message_id TEXT NOT NULL,
            synced     INTEGER DEFAULT 0,
            PRIMARY KEY (peer_id, message_id)
        );
    ")?;
    Ok(())
}

pub fn save_message(conn: &Connection, msg: &StoredMessage) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO messages
         (id, packet_id, sender_id, recipient_id, nonce, ciphertext, aead_tag, received_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            msg.id, msg.packet_id, msg.sender_id, msg.recipient_id,
            msg.nonce, msg.ciphertext, msg.aead_tag, msg.received_at
        ],
    )?;
    Ok(())
}

/// Ambil semua fingerprint (16 byte prefix Ascon-Hash) untuk Epidemic Sync
pub fn get_all_fingerprints(conn: &Connection) -> Result<Vec<[u8; 16]>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT ciphertext FROM messages")?;
    let fps: Vec<[u8; 16]> = stmt.query_map([], |row| {
        let ct: Vec<u8> = row.get(0)?;
        let hash = crate::crypto::hash256(&ct);
        let mut fp = [0u8; 16];
        fp.copy_from_slice(&hash[0..16]);
        Ok(fp)
    })?.flatten().collect();
    Ok(fps)
}
```

### 8.2 `sync.rs` — Epidemic Sync

```rust
// src-tauri/src/sync.rs
// Epidemic Sync menggunakan Ascon-Hash fingerprint vectors.
// Node hanya bertukar HASH pesan, bukan konten pesan.
// Node perantara (relay sync) tidak dapat membaca isi pesan.

pub async fn initiate_sync(
    stream: &mut tokio::net::TcpStream,
    local_fps: Vec<[u8; 16]>,
    my_node_id: &crate::keys::NodePublicKey,
) -> anyhow::Result<Vec<[u8; 16]>> {
    // Kirim SYNC_REQ dengan fingerprint vector kita
    let sync_req = build_sync_req(my_node_id, &local_fps);
    crate::transport::send_packet(stream, &sync_req).await?;

    // Tunggu SYNC_RESP dari peer
    // ... parse dan return their fingerprints
    Ok(vec![]) // TODO: implementasi lengkap
}

fn build_sync_req(node_id: &crate::keys::NodePublicKey, fps: &[[u8; 16]]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&node_id.0);              // 32 byte
    payload.extend_from_slice(&(fps.len() as u32).to_le_bytes()); // 4 byte
    for fp in fps {
        payload.extend_from_slice(fp);                  // 16 byte per fingerprint
    }
    // Wrap sebagai CLAMP packet (omitted untuk brevity)
    payload
}
```

### ✅ Checklist Fase 5

```
- [ ] SQLite database dibuat otomatis di path yang benar
- [ ] save_message() berhasil menyimpan ciphertext (bukan plaintext!)
- [ ] get_all_fingerprints() menghasilkan hash yang benar
- [ ] Dua node terhubung → SYNC_REQ dan SYNC_RESP tertukar
- [ ] Node A mendapat pesan yang dikirim ke Node C saat offline
- [ ] Verifikasi: database tidak mengandung plaintext apapun
```

---

## 9. Fase 6 — Integrasi Tauri dan GUI

> **Tujuan Fase 6:** Hubungkan semua modul backend dengan UI melalui Tauri IPC.

### 9.1 `commands.rs` — IPC Handlers

```rust
// src-tauri/src/commands.rs
use tauri::State;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::state::AppState;

#[tauri::command]
pub async fn send_dm(
    recipient_id: String,
    plaintext: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let state = state.lock().await;

    // 1. Parse recipient Node ID
    let recipient_bytes = hex::decode(&recipient_id)
        .map_err(|e| e.to_string())?;
    let peer_id = crate::keys::NodePublicKey(recipient_bytes.try_into()
        .map_err(|_| "Invalid node ID length")?);

    // 2. ECDH key exchange
    let shared = crate::keys::ecdh(&state.my_private_key, &peer_id);
    let session_id = [0u8; 8]; // TODO: session management
    let msg_counter = 0u64;     // TODO: counter per session

    // 3. Derive DM-Key
    let aead_key = crate::keys::derive_dm_key(
        &shared, &state.my_node_id, &peer_id,
        &session_id, msg_counter, true
    );

    // 4. Enkripsi payload
    let nonce = crate::crypto::generate_nonce();
    // Build CLAMP header dulu sebagai Associated Data
    let packet_id = crate::packet::ClampPacket::generate_packet_id(&state.my_node_id.0);
    let header = crate::packet::ClampHeader {
        magic: crate::packet::MAGIC,
        version: crate::packet::PROTOCOL_VERSION,
        packet_type: crate::packet::PacketType::Dm,
        ttl: crate::packet::TTL_MAX,
        packet_id,
    };
    let temp_packet = crate::packet::ClampPacket {
        header: header.clone(),
        hop_auth: crate::packet::HopAuth { hop_counter: 0, mac_tag: [0u8; 16] },
        nonce: nonce.0,
        ciphertext: vec![],
        aead_tag: [0u8; 16],
    };
    let aad = temp_packet.header_bytes();

    let (ciphertext, aead_tag) = crate::crypto::encrypt(
        &aead_key, &nonce, plaintext.as_bytes(), &aad
    ).map_err(|e| e.to_string())?;

    // 5. Bangun CLAMP packet lengkap
    // ... (omitted untuk brevity)

    // 6. Simpan ke database
    // 7. Broadcast ke semua peer

    Ok(hex::encode(packet_id))
}
```

### 9.2 Frontend TypeScript

```typescript
// src/api/tauri.ts
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface Message {
  senderId: string;
  plaintext: string;
  timestamp: number;
}

export async function sendDm(recipientId: string, plaintext: string): Promise<string> {
  return invoke('send_dm', { recipientId, plaintext });
}

export async function getMessages(peerId: string, limit = 50): Promise<Message[]> {
  return invoke('get_messages', { peerId, limit });
}

// Event listeners
export async function onMessageReceived(callback: (msg: Message) => void) {
  return listen<Message>('message_received', (event) => callback(event.payload));
}

export async function onPeerDiscovered(callback: (peer: any) => void) {
  return listen('peer_discovered', (event) => callback(event.payload));
}
```

```typescript
// src/main.ts
import { onMessageReceived, onPeerDiscovered } from './api/tauri';

// Setup event listeners saat aplikasi load
await onMessageReceived((msg) => {
    console.log('Pesan masuk dari:', msg.senderId);
    renderMessage(msg);
});

await onPeerDiscovered((peer) => {
    console.log('Peer ditemukan:', peer.displayName);
    renderPeer(peer);
});
```

### ✅ Checklist Fase 6

```
- [ ] cargo tauri dev berjalan tanpa error
- [ ] Halaman utama tampil di window Tauri
- [ ] invoke('init_node') berhasil dan mengembalikan Node ID + fingerprint
- [ ] invoke('get_peers') mengembalikan daftar peer
- [ ] UI: Chat window menampilkan pesan
- [ ] UI: Peer list menampilkan peer yang online
- [ ] UI: Status bar menampilkan jumlah peer terhubung
- [ ] Event 'message_received' ter-emit dan ditampilkan di UI
- [ ] Event 'peer_discovered' ter-emit dan ditampilkan di peer list
```

---

## 10. Fase 7 — Benchmarking dan Evaluasi

> **Tujuan Fase 7:** Hasilkan data empiris untuk makalah — bandingkan Ascon vs AES-GCM.

### 10.1 Setup Benchmark

```rust
// benches/crypto_bench.rs
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn bench_ascon_aead(c: &mut Criterion) {
    let mut group = c.benchmark_group("Ascon-AEAD128 vs AES-256-GCM");

    for size in [64usize, 256, 1024, 4096, 16384] {
        let plaintext = vec![0u8; size];
        let key = caraka_desktop::crypto::AeadKey([42u8; 16]);
        let aad = b"benchmark-header";

        group.throughput(Throughput::Bytes(size as u64));

        // Benchmark Ascon-AEAD128
        group.bench_with_input(
            BenchmarkId::new("Ascon-AEAD128/encrypt", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let nonce = caraka_desktop::crypto::generate_nonce();
                    caraka_desktop::crypto::encrypt(&key, &nonce, &plaintext, aad).unwrap()
                })
            },
        );

        // Benchmark AES-256-GCM (baseline)
        group.bench_with_input(
            BenchmarkId::new("AES-256-GCM/encrypt", size),
            &size,
            |b, _| {
                b.iter(|| {
                    // TODO: implementasi AES-256-GCM baseline
                    // Gunakan crate 'aes-gcm'
                })
            },
        );
    }
    group.finish();
}

fn bench_mac(c: &mut Criterion) {
    let mut group = c.benchmark_group("MAC Computation");
    let key = caraka_desktop::crypto::MacKey([0u8; 16]);
    let data = [0u8; 13]; // packet_id(8) + hop_counter(1) + relay_id_prefix(4)

    group.bench_function("Ascon-MAC (13 byte input)", |b| {
        b.iter(|| caraka_desktop::crypto::compute_mac(&key, &data))
    });
    group.finish();
}

criterion_group!(benches, bench_ascon_aead, bench_mac);
criterion_main!(benches);
```

```bash
# Jalankan benchmark
cargo bench

# Lihat hasil HTML di browser
start target\criterion\report\index.html  # Windows
open target/criterion/report/index.html   # macOS/Linux
```

### 10.2 Metrik yang Harus Dikumpulkan

**Microbenchmark (Level 1):**

| Metrik | Ukuran Input | Algoritma |
|---|---|---|
| Encryption Throughput (MB/s) | 64B, 256B, 1KB, 4KB, 16KB | Ascon-AEAD128, AES-256-GCM, ChaCha20-Poly1305 |
| Decryption Throughput (MB/s) | 64B, 256B, 1KB, 4KB, 16KB | Semua algoritma di atas |
| MAC Computation Time (μs) | 13 byte (hop auth input) | Ascon-MAC vs HMAC-SHA256 |
| Key Derivation Time (μs) | — | Ascon-XOF128 vs HKDF-SHA256 |

**Network Benchmark (Level 2):**

Setup 3–5 komputer/VM di LAN yang sama, kemudian ukur:

| Metrik | Topologi | Target |
|---|---|---|
| End-to-End Latency (ms) | Linear 5-hop, Star, Mesh 10-node | < 500ms untuk 5 hop |
| Message Delivery Ratio (%) | 1000 pesan per topologi | > 99% pada kondisi normal |
| Ciphertext Expansion (%) | — | (62 byte overhead / payload size) × 100% |
| Sync Throughput (msg/s) | 2 node reconnect dengan 100 pesan pending | > 50 msg/s |

### 10.3 Format Pelaporan Data

Catat semua hasil dalam format tabel untuk makalah:

```markdown
## Hasil Microbenchmark: Ascon-AEAD128 vs AES-256-GCM

| Ukuran Input | Ascon Enc (MB/s) | AES-GCM Enc (MB/s) | Ascon Dec (MB/s) | AES-GCM Dec (MB/s) |
|:---:|:---:|:---:|:---:|:---:|
| 64B   | [hasil] | [hasil] | [hasil] | [hasil] |
| 256B  | [hasil] | [hasil] | [hasil] | [hasil] |
| 1KB   | [hasil] | [hasil] | [hasil] | [hasil] |
| 4KB   | [hasil] | [hasil] | [hasil] | [hasil] |
| 16KB  | [hasil] | [hasil] | [hasil] | [hasil] |

*Platform: [CPU], [OS], Rust [versi], tanpa AES-NI / dengan AES-NI*
```

---

## 11. Pengujian (Testing)

```bash
# Run semua unit test
cargo test

# Run dengan output verbose
cargo test -- --nocapture

# Run test spesifik
cargo test test_encrypt_decrypt
cargo test test_ecdh_key_agreement
cargo test test_packet_roundtrip

# Run benchmark
cargo bench
```

### Matriks Test Coverage

| Modul | Unit Test Minimal yang Wajib Ada |
|---|---|
| `crypto.rs` | encrypt→decrypt roundtrip, decrypt fail on tamper, MAC verify, constant-time compare |
| `keys.rs` | keypair generation, ECDH commutativity, DM-key derivation symmetry |
| `packet.rs` | encode→decode roundtrip, invalid magic detection, short buffer detection |
| `routing.rs` | duplicate packet drop, invalid MAC drop, TTL decrement, trust score update |
| `store.rs` | save→retrieve message, fingerprint correctness, no plaintext in DB |
| `sync.rs` | fingerprint diff computation, gap resolution |

---

## 12. Aturan Kode Wajib

> [!CAUTION]
> Pelanggaran aturan ini dapat mengakibatkan kelemahan keamanan kriptografi yang serius.

```bash
# Jalankan SEBELUM setiap commit:
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Aturan Keamanan Kritikal

| No | Aturan | Alasan |
|:---:|---|---|
| 1 | Semua randomness dari **`OsRng`** — tidak boleh `thread_rng()` | `thread_rng()` tidak cryptographically secure |
| 2 | Semua tipe kunci **implement `Zeroize`** | Kunci terhapus dari memori saat di-drop |
| 3 | Database **tidak pernah menyimpan plaintext** | Hanya ciphertext yang boleh persisted |
| 4 | Nonce **selalu di-generate ulang** untuk setiap enkripsi | Nonce reuse dengan kunci yang sama = keamanan hilang |
| 5 | **Tidak boleh `unwrap()`** di production path | Gunakan `Result<T, E>` dan propagate error |
| 6 | **Tidak boleh log** key material atau plaintext pesan | Prevent information leakage via logs |
| 7 | Gunakan **constant-time comparison** untuk MAC | Prevent timing side-channel attack |

---

## 13. Troubleshooting

| Masalah | Penyebab Kemungkinan | Solusi |
|---|---|---|
| `rustc not found` | Rust belum di PATH | Restart terminal setelah install rustup |
| `cargo tauri dev` error WebView2 | WebView2 tidak terinstall | `winget install Microsoft.EdgeWebView2Runtime` |
| Peer tidak terdeteksi UDP | Firewall block UDP 7770 | Buka port UDP 7770 di Windows Firewall |
| TCP connect refused | Port 7771 sudah digunakan | Ubah `DATA_PORT` di config atau kill proses lain |
| Decrypt selalu gagal | session_id atau msg_counter tidak sinkron | Pastikan Alice send-key == Bob recv-key (lihat test `test_dm_key_derivation_symmetry`) |
| DB tidak ter-update | Write tanpa commit | Gunakan `conn.execute_batch()` dengan transaksi |
| Broadcast storm | TTL tidak di-decrement | Verifikasi routing.rs: `packet.header.ttl -= 1` sebelum relay |

---

## 14. Roadmap Pengembangan

### v0.1 — Academic Prototype (Sekarang)

**Target:** Selesai dalam 10 minggu. Digunakan untuk benchmarking dan makalah.

- [x] Desain protokol CLAMP + stack kriptografi
- [ ] **Fase 1:** Modul kriptografi (crypto.rs, keys.rs)
- [ ] **Fase 2:** Protokol CLAMP (packet.rs)
- [ ] **Fase 3:** Routing engine (routing.rs)
- [ ] **Fase 4:** Jaringan P2P (discovery.rs, transport.rs)
- [ ] **Fase 5:** Penyimpanan + Epidemic Sync (store.rs, sync.rs)
- [ ] **Fase 6:** GUI Tauri + integrasi backend
- [ ] **Fase 7:** Benchmark + evaluasi + makalah final

### v0.2 — Security Hardening (Setelah Nilai Keluar 😄)

- [ ] OS Keychain integration (Windows Credential Manager / macOS Keychain)
- [ ] Channel key rotation otomatis
- [ ] Tor transport opsional (sembunyikan IP address)
- [ ] Group channel message (CH-Key untuk semua anggota)
- [ ] QR code fingerprint verification dalam UI

### v0.3 — Post-Quantum (Penelitian Lanjutan)

- [ ] **Hybrid KEM:** X25519 + CRYSTALS-Kyber (ML-KEM, FIPS 203) — tahan serangan quantum
- [ ] Transfer file terenkripsi (chunked + resume)
- [ ] Dukungan Android (shared Rust core via JNI)
- [ ] LoRa radio transport plugin (via USB serial adapter)

---

*— Akhir Development Guide CARAKA Desktop v0.1 —*
