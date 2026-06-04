<div align="center">

<img src="src-tauri/icons/icon.png" alt="CARAKA Logo" width="120"/>

# CARAKA Desktop

### *Secure Decentralized Offline Mesh Communication*

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-blue?style=flat-square&logo=tauri)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey?style=flat-square)]()
[![NIST LWC](https://img.shields.io/badge/Crypto-Ascon--AEAD128%20%28NIST%20SP%20800--232%29-purple?style=flat-square)]()

> **Platform komunikasi mesh offline terdesentralisasi** berbasis kriptografi ringan Ascon-AEAD128 standar NIST.  
> Berkomunikasi aman antar perangkat di jaringan lokal **tanpa server pusat, tanpa internet.**

[📥 Download](#-instalasi) • [🚀 Quick Start](#-menjalankan-aplikasi) • [📖 Dokumentasi](#-dokumentasi) • [🏗️ Arsitektur](#️-arsitektur)

</div>

---

## ✨ Fitur Utama

| Fitur | Keterangan |
|---|---|
| 🔒 **End-to-End Encrypted** | Setiap pesan dienkripsi dengan Ascon-AEAD128 (NIST SP 800-232) sebelum meninggalkan perangkat |
| 🌐 **Mesh Networking** | Pesan dapat di-relay melalui node perantara tanpa perantara yang bisa membaca isinya |
| 📡 **Auto Peer Discovery** | Penemuan peer otomatis via UDP Broadcast tanpa konfigurasi manual |
| 🔑 **X25519 ECDH Key Exchange** | Pertukaran kunci Diffie-Hellman berbasis kurva eliptik untuk forward secrecy |
| 💾 **Offline-First** | Pesan disimpan terenkripsi di database lokal dan disinkronkan saat peer kembali online |
| 🛡️ **Replay Protection** | LRU cache 512 entri untuk mencegah serangan replay paket |
| ⚡ **Lightweight** | Binary Rust dengan overhead memori minimal, cocok untuk perangkat low-end |
| 🖥️ **Cross-Platform** | Tersedia untuk Windows, Linux, dan macOS via Tauri v2 |

---

## 🏗️ Arsitektur

CARAKA mengimplementasikan **Protokol CLAMP** (*Custom Lightweight Authenticated Mesh Protocol*), sebuah protokol lapisan aplikasi biner yang dirancang untuk keamanan dan efisiensi di jaringan mesh lokal.

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
│   │X25519    │       │Trust Score       │ Only     │        │
│   └──────────┘       └──────────┘       └──────────┘        │
│                             │                                │
│         ┌───────────────────┼───────────────────┐           │
│         ▼                   ▼                   ▼           │
│   ┌──────────┐       ┌──────────┐       ┌──────────┐        │
│   │discovery │       │transport │       │  sync.rs │        │
│   │  .rs     │       │  .rs     │       │          │        │
│   │UDP :7770 │       │TCP :7771 │       │Epidemic  │        │
│   │Broadcast │       │Framed    │       │Sync      │        │
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
| **Database** | SQLite via `rusqlite` (hanya menyimpan ciphertext) |
| **Transport** | TCP (data) + UDP (discovery) |

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

# Install Node.js (untuk npx serve)
# https://nodejs.org/
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

- **Plaintext tidak pernah menyentuh disk** — hanya tersimpan di memori RAM selama proses enkripsi/dekripsi.
- **Database SQLite** hanya menyimpan `ciphertext` biner — tidak ada data yang bisa dibaca tanpa kunci.
- **Setiap relay node** (Bob) hanya meneruskan paket terenkripsi dan memvalidasi Hop-MAC, tidak bisa membaca isi pesan.
- **Replay Protection** — setiap paket memiliki ID unik yang dicek di LRU cache 512 entri.

---

## 📁 Struktur Proyek

```
CARAKA-DEKSTOP/
├── src/                        # Frontend (HTML/CSS/JS)
│   ├── index.html              # Tampilan utama
│   ├── main.js                 # Logic UI & Tauri IPC
│   └── styles/main.css         # Styling
│
├── src-tauri/                  # Backend Rust
│   ├── src/
│   │   ├── main.rs             # Entry point Tauri
│   │   ├── state.rs            # Node lifecycle & AppState
│   │   ├── commands.rs         # Tauri IPC commands
│   │   ├── crypto.rs           # Ascon-AEAD128 + X25519
│   │   ├── keys.rs             # Key management
│   │   ├── packet.rs           # CLAMP protocol framing
│   │   ├── routing.rs          # Mesh routing + trust score
│   │   ├── discovery.rs        # UDP peer discovery
│   │   ├── transport.rs        # TCP transport layer
│   │   ├── store.rs            # SQLite encrypted store
│   │   └── sync.rs             # Epidemic sync
│   ├── capabilities/
│   │   └── default.json        # Tauri v2 permissions
│   └── tauri.conf.json         # Konfigurasi Tauri
│
└── docs/                       # Dokumentasi
    ├── 01_PROJECT_PROPOSAL.md
    ├── 02_TECHNICAL_DESIGN.md
    └── 03_DEVELOPMENT_GUIDE.md
```

---

## 📖 Dokumentasi

Dokumentasi teknis tersedia di dalam source code masing-masing modul (`src-tauri/src/*.rs`).
Untuk spesifikasi protokol CLAMP secara lengkap, silakan lihat komentar di [`packet.rs`](src-tauri/src/packet.rs) dan [`routing.rs`](src-tauri/src/routing.rs).

---

## 🧪 Testing

```bash
cd src-tauri

# Jalankan semua unit test
cargo test

# Jalankan dengan output verbose
cargo test -- --nocapture
```

**Status:** 66/66 tests passing ✅

---

## 🗺️ Roadmap

- [x] **Fase 1** — Core Cryptographic Engine (Ascon-AEAD128, X25519 ECDH)
- [x] **Fase 2** — CLAMP Protocol Engine (packet framing, replay protection)
- [x] **Fase 3** — P2P Networking (UDP discovery, TCP transport, mesh routing)
- [x] **Fase 4** — GUI Desktop (Tauri v2, chat UI, peer management)
- [ ] **Fase 5** — Evaluasi & Benchmark (microbenchmark, network benchmark, paper)

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
