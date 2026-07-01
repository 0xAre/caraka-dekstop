<div align="center">

<img src="src-tauri/icons/icon.png" alt="CARAKA Logo" width="120"/>

# CARAKA Desktop

### *Secure Decentralized Offline Mesh Communication*

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-blue?style=flat-square&logo=tauri)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows-lightgrey?style=flat-square)]()
[![NIST LWC](https://img.shields.io/badge/Crypto-Ascon--AEAD128%20%28NIST%20SP%20800--232%29-purple?style=flat-square)]()

> **Platform komunikasi mesh offline terdesentralisasi** berbasis kriptografi ringan Ascon-AEAD128 standar NIST.  
> Berkomunikasi aman antar perangkat di jaringan lokal **tanpa server pusat, tanpa internet.**

[📥 Download](#-instalasi) • [🚀 Quick Start](#-menjalankan-aplikasi) • [📖 Dokumentasi](#-dokumentasi) • [🏗️ Arsitektur](#️-arsitektur)

</div>

---

## ✨ Fitur Utama

| Fitur | Keterangan |
|---|---|
| 🔒 **End-to-End Encrypted** | Setiap DM dienkripsi dengan Ascon-AEAD128 (NIST SP 800-232) + X25519 ECDH sebelum meninggalkan perangkat |
| 🌐 **Mesh Networking** | Pesan di-relay melalui node perantara via protokol CLAMP tanpa perantara yang bisa membaca isinya |
| 📡 **Auto Peer Discovery** | Penemuan peer otomatis via UDP Broadcast tanpa konfigurasi manual |
| 🔑 **X25519 ECDH Key Exchange** | Pertukaran kunci Diffie-Hellman berbasis kurva eliptik, HKDF-SHA256 untuk key derivation |
| 🛡️ **Private Key di Keychain OS** | Private key disimpan di Windows Credential Manager — tidak pernah ada di disk sebagai plaintext |
| 📊 **Token Bucket Rate Limiter** | Perlindungan per-peer: burst 200 paket, 100 paket/detik — pelanggaran menurunkan trust score |
| 💾 **Offline-First + Epidemic Sync** | Pesan tersimpan lokal dan disinkronkan otomatis saat peer kembali terhubung |
| 🔍 **QR Code Peer Verification** | Verifikasi identitas peer via QR code dan Safety Number (SHA-256 canonical) |
| 🆘 **Emergency Broadcast** | Siaran darurat bertipe INFO / EVAC / STATUS / RESOURCE dengan flooding mesh |
| 🗺️ **Radar Topology View** | Visualisasi real-time topologi jaringan mesh dengan garis koneksi antar node |
| 🛡️ **Replay Protection** | LRU cache 512 entri Packet ID untuk mencegah serangan replay paket |
| ⚡ **Lightweight** | Binary Rust dengan overhead memori minimal, cocok untuk perangkat low-end |

---

## 🏗️ Arsitektur

CARAKA mengimplementasikan **Protokol CLAMP** (*Compact Lightweight Authenticated Mesh Protocol*), protokol lapisan aplikasi biner yang dirancang untuk keamanan dan efisiensi di jaringan mesh lokal.

```
┌─────────────────────────────────────────────────────────────┐
│                     CARAKA Desktop (Tauri v2)                │
│  ┌─────────────┐    ┌──────────────────────────────────┐    │
│  │   Frontend  │    │         Backend (Rust)            │    │
│  │ HTML/CSS/JS │◄──►│  commands.rs  │  state.rs        │    │
│  └─────────────┘    └──────────────────────────────────┘    │
│                             │                                │
│         ┌───────────────────┼───────────────────┐           │
│         ▼                   ▼                   ▼           │
│   ┌──────────┐       ┌──────────┐       ┌──────────┐        │
│   │crypto.rs │       │routing.rs│       │ store.rs │        │
│   │          │       │          │       │          │        │
│   │Ascon-    │       │Controlled│       │ SQLite + │        │
│   │AEAD128   │       │Flooding  │       │Ciphertext│        │
│   │X25519    │       │Rate Limit│       │ Only     │        │
│   │HKDF-SHA2 │       │TrustScore│       │          │        │
│   └──────────┘       └──────────┘       └──────────┘        │
│                             │                                │
│         ┌───────────────────┼───────────────────┐           │
│         ▼                   ▼                   ▼           │
│   ┌──────────┐       ┌──────────┐       ┌──────────┐        │
│   │discovery │       │transport │       │  sync.rs │        │
│   │  .rs     │       │  .rs     │       │          │        │
│   │UDP :7770 │       │TCP :7771 │       │Epidemic  │        │
│   │Broadcast │       │+ Sync    │       │Sync      │        │
│   └──────────┘       └──────────┘       └──────────┘        │
└─────────────────────────────────────────────────────────────┘
```

### Stack Teknologi

| Lapisan | Teknologi |
|---|---|
| **Framework Desktop** | [Tauri v2](https://tauri.app/) |
| **Backend** | Rust 1.75+ |
| **Frontend** | HTML5, CSS3, Vanilla JavaScript |
| **Enkripsi Simetris** | Ascon-AEAD128 (NIST SP 800-232 / NIST LWC Winner) |
| **Key Exchange** | X25519 ECDH via `x25519-dalek` |
| **Key Derivation** | HKDF-SHA256 via `hkdf` + `sha2` |
| **Penyimpanan Kunci** | Windows Credential Manager via `keyring v2` |
| **Database** | SQLite via `rusqlite` (hanya menyimpan ciphertext) |
| **Transport** | TCP :7771 (data) + UDP :7770 (discovery) |
| **QR Code** | `qrcode 0.14` + `image 0.25` + `base64 0.22` |

---

## 📦 Instalasi

### Opsi 1: Download Installer (Direkomendasikan)

Unduh installer terbaru dari halaman [**Releases**](https://github.com/0xAre/CARAKA-DEKSTOP/releases):

- **Windows** → `CARAKA Desktop_x.x.x_x64-setup.exe` (NSIS Installer)
- **Windows** → `CARAKA Desktop_x.x.x_x64_en-US.msi` (MSI Package)

### Opsi 2: Build dari Source

**Prasyarat:**
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Tauri CLI
cargo install tauri-cli
```

**Build:**
```bash
# Clone repository
git clone https://github.com/0xAre/CARAKA-DEKSTOP.git
cd CARAKA-DEKSTOP

# Jalankan mode development
cargo tauri dev

# Atau build release
cargo tauri build
```

---

## 🚀 Menjalankan Aplikasi

### Demo Komunikasi Multi-Node (3 Laptop)

1. **Hubungkan semua laptop ke jaringan Wi-Fi yang sama**

2. **Buka port Firewall** di setiap laptop (jalankan sebagai Administrator):
   ```powershell
   New-NetFirewallRule -DisplayName "CARAKA UDP Discovery" -Direction Inbound -Protocol UDP -LocalPort 7770 -Action Allow
   New-NetFirewallRule -DisplayName "CARAKA TCP Data" -Direction Inbound -Protocol TCP -LocalPort 7771 -Action Allow
   ```

3. **Jalankan `caraka-desktop.exe`** di ketiga laptop. Saat Windows Firewall meminta izin, klik **Allow**.

4. **Peer Discovery Otomatis** — dalam ±30 detik, laptop lain akan muncul di sidebar secara otomatis.

5. **Tambah Peer Manual** (jika discovery tidak berjalan):
   - Klik tombol `+` di sidebar
   - Masukkan IP address laptop lain (cek via `ipconfig` → bagian `Wi-Fi`)
   - Port: `7771`

---

## 🔐 Model Keamanan

```
Alice (Laptop 1)                Bob (Relay)              Charlie (Laptop 3)
      │                              │                          │
      │──── DM-Key = HKDF(ECDH) ────►│                          │
      │                              │    (tidak bisa baca isi) │
      │──── Ascon-AEAD128(msg) ──────►│──── forward paket ───────►│
      │                              │                          │
      │◄═══════════════════ Pesan hanya terbaca oleh Charlie ══════╝
```

| Properti | Implementasi |
|---|---|
| **Private key** | Windows Credential Manager (tidak pernah di disk plaintext) |
| **Ciphertext di disk** | SQLite hanya menyimpan bytes terenkripsi |
| **Relay node** | Hanya meneruskan paket terenkripsi, tidak bisa baca isi |
| **Replay attack** | LRU cache 512 Packet ID + nonce window ±300 detik |
| **Rate limiting** | Token bucket per-peer: burst 200, 100 pkt/det |
| **AAD binding** | Header CLAMP 13-byte sebagai AAD — ciphertext terikat ke konteks paket |
| **Hop authentication** | HKDF-derived HopMAC di setiap relay hop |

---

## 📁 Struktur Proyek

```
CARAKA-DEKSTOP/
├── src/                          # Frontend (HTML/CSS/JS)
│   ├── index.html                # Tampilan utama (chat, radar, settings)
│   ├── main.js                   # Logic UI & Tauri IPC
│   └── styles/main.css           # Styling
│
├── src-tauri/                    # Backend Rust
│   ├── src/
│   │   ├── main.rs               # Entry point Tauri + command registration
│   │   ├── state.rs              # AppState, node lifecycle
│   │   ├── commands.rs           # Tauri IPC: send_dm, broadcast, QR, safety number
│   │   ├── crypto.rs             # Ascon-AEAD128 + X25519 ECDH
│   │   ├── keys.rs               # Key generation & Credential Manager
│   │   ├── packet.rs             # CLAMP packet framing & types
│   │   ├── routing.rs            # Mesh routing, trust score, rate limiting
│   │   ├── discovery.rs          # UDP peer discovery (:7770)
│   │   ├── transport.rs          # TCP transport + epidemic sync handlers
│   │   ├── store.rs              # SQLite encrypted store
│   │   ├── sync.rs               # Epidemic sync coordination
│   │   ├── hotspot.rs            # Wi-Fi hotspot management
│   │   └── network_monitor.rs    # Network interface monitoring
│   ├── icons/                    # App icons (semua ukuran, CARAKA brand)
│   ├── capabilities/
│   │   └── default.json          # Tauri v2 permissions
│   └── tauri.conf.json           # Konfigurasi Tauri (CSP, NSIS, signing)
│
├── docs/
│   ├── CLAMP-SPEC.md             # Spesifikasi protokol CLAMP lengkap
│   ├── 01_PROJECT_PROPOSAL.md
│   ├── 02_TECHNICAL_DESIGN.md
│   └── 03_DEVELOPMENT_GUIDE.md
│
└── .github/
    └── workflows/
        └── release.yml           # CI/CD: build & release otomatis via tag v*.*.*
```

---

## 📖 Dokumentasi

Spesifikasi teknis protokol CLAMP tersedia di [`docs/CLAMP-SPEC.md`](docs/CLAMP-SPEC.md), mencakup:

- Format paket biner (62-byte fixed header)
- Semua tipe paket (DM, Channel, SyncReq/Resp/Data, Hello, Broadcast)
- Detail kriptografi: Ascon-AEAD128, X25519 ECDH, HKDF-SHA256 DM key, HopMAC
- Routing, replay protection, trust score, rate limiting
- Epidemic sync, inner DM payload JSON, broadcast payload JSON

---

## 🗺️ Roadmap

- [x] **Fase 1** — Core Cryptographic Engine (Ascon-AEAD128, X25519 ECDH, HKDF)
- [x] **Fase 2** — CLAMP Protocol Engine (packet framing, replay protection, trust score)
- [x] **Fase 3** — P2P Networking (UDP discovery, TCP transport, mesh routing)
- [x] **Fase 4** — GUI Desktop (Tauri v2, chat UI, radar topology, peer management)
- [x] **Fase 5** — Security Hardening (keyring, rate limiting, CSP, QR verification, safety number)
- [x] **Fase 6** — Fitur Lanjutan (epidemic sync, emergency broadcast types, CI/CD release pipeline)
- [ ] **Fase 7** — Evaluasi & Benchmark (microbenchmark, network benchmark, paper)

---

## 🤝 Kontribusi

Kontribusi sangat disambut! Silakan buka *Issue* atau *Pull Request*.

1. Fork repository ini
2. Buat branch fitur baru: `git checkout -b feat/nama-fitur`
3. Commit perubahan: `git commit -m 'feat: tambah fitur X'`
4. Push ke branch: `git push origin feat/nama-fitur`
5. Buka Pull Request

---

## 📄 Lisensi

Didistribusikan di bawah **Lisensi MIT**. Lihat [`LICENSE`](LICENSE) untuk informasi lebih lanjut.

---

<div align="center">

Dibuat dengan ❤️ menggunakan **Rust** + **Tauri** + **Ascon**

*"Komunikasi aman bukan hak istimewa — ini adalah kebutuhan dasar."*

</div>
