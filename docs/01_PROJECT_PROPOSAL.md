---

# PROPOSAL PROYEK IMPLEMENTASI

## CARAKA Desktop: Perancangan dan Implementasi Sistem Komunikasi Mesh Offline Aman Menggunakan Lightweight Cryptography

---

| | |
|---|---|
| **Nama Proyek** | CARAKA Desktop |
| **Mata Kuliah** | Implementasi Kriptografi |
| **Pengusul** | [CARAKA TEAM] |
| **NIM** | 2322101878 / 23221019 |
| **Program Studi** | [Nama Program Studi] |
| **Fakultas** | [Nama Fakultas] |
| **Universitas** | [Nama Universitas] |
| **Dosen Pengampu** | [Nama Dosen] |
| **Semester / T.A.** | [Semester] / [Tahun Akademik] |
| **Tanggal Pengajuan** | Juni 2026 |

---

## BAB I — PENDAHULUAN

### 1.1 Latar Belakang

Infrastruktur komunikasi digital kontemporer memiliki dependensi yang hampir mutlak terhadap entitas terpusat: server, *Content Delivery Network (CDN)*, penyedia layanan internet, dan tulang punggung jaringan global. Dependensi ini menciptakan *single point of failure* yang secara arsitektural berbahaya dalam tiga skenario utama: (1) kegagalan infrastruktur akibat bencana alam atau pemadaman listrik, (2) pembatasan akses jaringan yang disengaja oleh pihak yang berwenang, dan (3) degradasi layanan akibat kemacetan jaringan pada kondisi darurat ketika kebutuhan komunikasi paling kritis.

Jaringan *Peer-to-Peer (P2P) Mesh* menawarkan solusi arsitektural alternatif: setiap perangkat berperan ganda sebagai klien sekaligus *router*, memungkinkan pesan mencapai tujuannya melalui jalur alternatif tanpa bergantung pada entitas pusat. Namun, mengamankan komunikasi pada arsitektur ini menghadirkan tantangan kriptografi yang fundamental, khususnya pada konteks *multi-hop relay* di mana setiap node perantara turut memproses setiap paket yang melintas.

Pada tahun 2013, *National Institute of Standards and Technology* (NIST) memulai proses standardisasi *Lightweight Cryptography (LWC)* untuk mengatasi ketidaksesuaian fundamental antara algoritma kriptografi standar (AES-GCM, SHA-256) dengan keterbatasan sumber daya perangkat tertanam [7]. Setelah satu dekade seleksi ketat — termasuk evaluasi melalui kompetisi CAESAR dan proses NIST LWC resmi — NIST mengumumkan terpilihnya **keluarga Ascon** pada Februari 2023, yang kemudian dipublikasikan sebagai **NIST Special Publication 800-232** pada Agustus 2025 [8]. Meskipun demikian, integrasi standar LWC ini ke dalam protokol komunikasi *mesh* terdesentralisasi belum pernah dieksplorasi secara sistematis dalam literatur yang ada.

### 1.2 Identifikasi Masalah

Sistem komunikasi *offline* yang telah ada saat ini — Meshtastic, Berty/Wesh, Briar, dan Secure Scuttlebutt — menghadapi dilema mendasar yang belum terpecahkan antara keamanan dan efisiensi jaringan.

**Meshtastic** [1] beroperasi pada jaringan radio LoRa dengan kapasitas paket yang sangat terbatas (256 byte per transmisi). Sistem ini mendokumentasikan sendiri keterbatasan kritis: tanda tangan XEdDSA (berbasis Ed25519) yang digunakan untuk autentikasi *Direct Message* mengkonsumsi 64 byte per paket — setara dengan lebih dari 25% total kapasitas transmisi LoRa. Sebagai konsekuensinya, *routing header* dibiarkan dalam bentuk *plaintext*, sehingga pihak ketiga dapat memetakan topologi jaringan melalui analisis lalu lintas pasif [1, 6].

**Berty** (Wesh Protocol) [2, 3] menggunakan Ed25519 untuk menandatangani setiap *event* yang direplikasi melalui jaringan *gossip*, menghasilkan *overhead* signifikan yang tidak kompatibel dengan jaringan *low-bandwidth*. Berty secara eksplisit mengakui bahwa protokolnya belum siap untuk data sensitif tinggi [3].

**Briar** [4] berhasil mencapai privasi tinggi melalui routing berbasis Tor, namun routing *offline*-nya dibatasi oleh *social graph*: pesan hanya dapat diteruskan melalui kontak bersama, bukan melalui jaringan *mesh* generik dengan anggota yang belum saling mengenal.

**Secure Scuttlebutt (SSB)** [5] membangun rantai log yang ditandatangani secara permanen menggunakan Ed25519, menciptakan jejak digital yang tidak dapat dihapus — sebuah karakteristik yang secara fundamental bertentangan dengan kebutuhan komunikasi privat dan sementara.

Tabel 1.1 berikut merangkum komparasi sistem yang ada terhadap solusi yang diusulkan:

**Tabel 1.1** Perbandingan Sistem Komunikasi *Offline* yang Ada

| Sistem | *Overhead* Autentikasi | Keamanan Metadata | Dukungan *Offline* |
|---|---|---|---|
| Meshtastic | 64 byte (Ed25519) | Tidak — *header* bersifat *plaintext* | Penuh |
| Berty | Ed25519 per-*event* | Tidak — *gossip* terbuka | Terbatas |
| Briar | AES-GCM + Signal | Ya — via Tor (hanya *online*) | Terbatas (*social graph*) |
| Secure Scuttlebutt | Ed25519 per-log | Tidak — log bersifat permanen | Penuh |
| **CARAKA (Diusulkan)** | **17 byte (Ascon-MAC)** | **Ya — MAC per-hop** | **Penuh** |

Berdasarkan tinjauan terhadap sistem yang ada, diidentifikasi empat permasalahan utama:

1. **Overhead Autentikasi Berlebih:** Penggunaan tanda tangan digital Ed25519 (64 byte per paket) mengkonsumsi proporsi besar dari kapasitas paket yang tersedia, secara langsung membatasi kapasitas *payload* pesan dan skalabilitas protokol pada jaringan *low-bandwidth*.

2. **Kebocoran Metadata Routing:** *Routing header* dikirimkan dalam bentuk *plaintext* pada seluruh sistem yang ada, memungkinkan analisis lalu lintas pasif untuk memetakan topologi jaringan dan mengidentifikasi pola komunikasi tanpa perlu mendekripsi konten pesan.

3. **Ketiadaan Implementasi NIST LWC dalam Protokol Mesh:** Tidak ada sistem komunikasi mesh *offline* yang telah mengintegrasikan standar NIST LWC (Ascon) ke dalam desain protokolnya untuk autentikasi *multi-hop relay* dan enkripsi *routing metadata*.

4. **Manajemen Kunci pada Lingkungan *Delay-Tolerant*:** Mekanisme *Perfect Forward Secrecy* berbasis *Double Ratchet* memerlukan urutan pesan yang terjamin — sebuah asumsi yang tidak dapat dipertahankan dalam jaringan *offline* dengan penundaan tak tentu.

### 1.3 Rumusan Masalah

Berdasarkan identifikasi masalah di atas, rumusan masalah penelitian adalah sebagai berikut:

1. Bagaimana merancang protokol komunikasi *mesh* terdesentralisasi berbasis Ascon (NIST LWC) yang meminimalkan *cryptographic overhead* per-hop untuk autentikasi *relay* pada jaringan *offline*?

2. Bagaimana mengimplementasikan skema autentikasi *multi-hop* yang efisien menggunakan Ascon-MAC sebagai pengganti tanda tangan Ed25519 per-paket, dan seberapa besar reduksi *overhead* yang dapat dicapai?

3. Bagaimana performa implementasi berbasis Ascon (LWC) dibandingkan implementasi berbasis AES-256-GCM (kriptografi konvensional) dalam parameter: *encryption throughput*, *decryption throughput*, ukuran *overhead* paket, *end-to-end latency*, dan *memory footprint*?

4. Bagaimana merancang mekanisme sinkronisasi *store-and-forward* yang *privacy-preserving* menggunakan Ascon-Hash256 untuk pertukaran *fingerprint* pesan tanpa mengekspos konten kepada node perantara?

### 1.4 Batasan Penelitian

Untuk menjaga ketercapaian dalam kerangka waktu satu semester, penelitian ini dibatasi sebagai berikut:

1. **Platform target:** Aplikasi desktop (Windows dan Linux) yang beroperasi dalam jaringan area lokal (LAN/Wi-Fi).
2. **Transport layer:** UDP untuk *peer discovery*, TCP untuk pengiriman paket data.
3. **Skala jaringan uji:** 2 hingga 15 node dalam satu segmen jaringan (satu subnet).
4. **Cakupan implementasi kriptografi:** Seluruh implementasi menggunakan *library* yang telah diaudit komunitas (RustCrypto Project) untuk menghindari risiko kelemahan implementasi primitif kriptografi.
5. **Di luar cakupan:** *Post-Quantum Cryptography*, mekanisme konsensus *blockchain*, infrastruktur *cloud*, dan kecerdasan buatan sebagai komponen utama.

### 1.5 Tujuan Penelitian

#### 1.5.1 Tujuan Umum

Merancang, mengimplementasikan, dan mengevaluasi secara empiris sebuah platform komunikasi desktop berbasis jaringan *Peer-to-Peer Mesh* yang mengintegrasikan standar NIST *Lightweight Cryptography* (Ascon NIST SP 800-232) sebagai fondasi protokol keamanannya.

#### 1.5.2 Tujuan Khusus

1. Merancang **Protokol CLAMP** (*Compact Lightweight Authenticated Mesh Protocol*) yang mendefinisikan: struktur paket biner berlapis, mekanisme autentikasi per-hop berbasis Ascon-MAC, dan model hierarki kunci yang mendukung *forward secrecy* berbasis sesi.

2. Mengimplementasikan **modul kriptografi** menggunakan bahasa Rust dan *library* `ascon-aead` serta `x25519-dalek` dari ekosistem RustCrypto, mencakup fungsi enkripsi E2EE (Ascon-AEAD128), derivasi kunci (Ascon-XOF128), dan komputasi MAC per-hop (Ascon-MAC).

3. Mengimplementasikan **mesin jaringan P2P** mencakup: *peer discovery* berbasis UDP Broadcast, transport berbasis TCP, *Controlled Flooding Routing* dengan TTL dan *Packet ID Cache* (LRU-512), serta sistem *Trust Score* untuk mitigasi *Sybil Attack*.

4. Mengimplementasikan **mekanisme *Store-and-Forward* dan Epidemic Sync** menggunakan vektor *fingerprint* Ascon-Hash256 yang memungkinkan sinkronisasi pesan tanpa mengekspos konten kepada node perantara.

5. Melakukan **evaluasi empiris komparatif** antara implementasi berbasis Ascon dan berbasis AES-256-GCM pada kedua level: *microbenchmark* kriptografi (menggunakan Criterion.rs) dan *network-level benchmark* multi-hop (menggunakan topologi LAN yang dikontrol).

### 1.6 Manfaat Penelitian

#### 1.6.1 Manfaat Akademik

1. Menghasilkan data evaluasi empiris pertama yang membandingkan kinerja Ascon (NIST LWC) vs. AES-256-GCM dalam konteks protokol komunikasi *mesh desktop* — melengkapi literatur yang selama ini hanya mengevaluasi LWC pada perangkat tertanam (AVR, ARM Cortex-M).
2. Mendokumentasikan desain protokol *store-and-forward* berbasis LWC dengan properti *privacy-preserving sync* yang dapat diacu oleh penelitian lanjutan.
3. Menghasilkan referensi implementasi *open-source* protokol CLAMP yang dapat direplikasi dan diverifikasi oleh peneliti independen.

#### 1.6.2 Manfaat Praktis

1. Menghasilkan prototipe aplikasi komunikasi *offline-first* yang fungsional dan dapat diuji dalam skenario nyata (koordinasi darurat bencana, daerah terpencil tanpa infrastruktur internet).
2. Membuktikan skalabilitas protokol untuk ekspansi ke perangkat *constrained* (IoT, LoRa, ESP32) melalui desain berbasis LWC yang *hardware-agnostic*.

---

## BAB II — TINJAUAN PUSTAKA

### 2.1 Lightweight Cryptography dan Standar NIST

Algoritma *Lightweight Cryptography* dirancang untuk memenuhi kebutuhan keamanan pada perangkat dengan sumber daya komputasi, memori, dan energi yang sangat terbatas [7]. NIST memulai proses standardisasi resmi pada 2013 dan menerima 57 kandidat pada 2019. Setelah evaluasi tiga tahap, keluarga **Ascon** dipilih sebagai standar pada Februari 2023 [8].

Ascon, dirancang oleh Dobraunig, Eichlseder, Mendel, dan Schläffer [9], dibangun di atas permutasi *sponge* 320-bit (Ascon-p). Keluarga ini mencakup Ascon-AEAD128 (enkripsi terotentikasi, kunci 128-bit, tag 128-bit), Ascon-Hash256 (fungsi hash 256-bit), dan Ascon-XOF128 (*extendable output function*). Desain berbasis *sponge* memungkinkan satu keluarga primitif untuk memenuhi seluruh kebutuhan kriptografi simetris, signifikan dalam konteks implementasi terbatas yang menginginkan minimalisasi *code size*.

### 2.2 Analisis Algoritma LWC Finalis NIST

Berdasarkan analisis terhadap finalis NIST LWC, berikut adalah evaluasi masing-masing kandidat:

#### 2.2.1 TinyJambu

**TinyJambu** [10] menunjukkan kerentanan yang signifikan dalam analisis diferensial terbaru. *Birthday-bound slide attacks* pada permutasi kunci P2 untuk semua ukuran kunci membuktikan bahwa asumsi *ideal permutation* dalam bukti keamanan mode AEAD-nya tidak berlaku, sehingga tidak direkomendasikan untuk protokol baru.

#### 2.2.2 GIFT-COFB

**GIFT-COFB** [11] memiliki efisiensi *rate*=1 yang menarik (satu operasi *block cipher* per blok input). Namun, analisis oleh Khairallah [12] menunjukkan bahwa dalam skenario *high forgery count* pada jaringan *long-lived* dengan banyak partisipan, batas keamanan efektifnya berperilaku seperti AEAD dengan tag 64-bit, bukan 128-bit.

#### 2.2.3 Grain-128AEAD v2

**Grain-128AEAD v2** membatasi total panjang *keystream* per pasangan kunci/IV hingga sekitar 2^80 bit. Pada jaringan mesh aktif dengan banyak pesan, manajemen rotasi kunci yang ketat menjadi beban operasional yang tidak realistis.

#### 2.2.4 PRESENT

**PRESENT** [13] adalah *block cipher* 64-bit yang rentan terhadap serangan kolisi *birthday bound* (tipe Sweet32 [14]) untuk volume data besar, dan versi kunci 80-bit telah dianggap usang oleh standar kriptografi modern.

#### 2.2.5 Ascon (Terpilih)

**Ascon** adalah satu-satunya algoritma yang memenuhi seluruh kriteria: keamanan 128-bit yang telah dibuktikan dan diverifikasi secara menyeluruh, status standar NIST aktif, kapabilitas AEAD+Hash+XOF dalam satu keluarga, dan ekosistem implementasi Rust yang matang.

### 2.3 Tinjauan Sistem Komunikasi Mesh yang Ada

#### 2.3.1 Briar

**Briar** [4] mengimplementasikan konstruksi mirip-Signal dengan *Perfect Forward Secrecy* berbasis *Double Ratchet* dan AES-GCM untuk enkripsi simetris. Keterbatasan utamanya adalah routing *offline* yang bergantung pada *social graph* — pesan hanya dapat diteruskan melalui kontak bersama, membatasi jangkauan dalam skenario *ad-hoc*.

#### 2.3.2 Berty / Wesh Protocol

**Berty/Wesh Protocol** [2, 3] dibangun di atas IPFS dan OrbitDB dengan menggunakan X25519 untuk enkripsi dan Ed25519 untuk tanda tangan. Berty secara eksplisit mengakui bahwa protokolnya belum siap untuk data sensitif tinggi [3] dan overhead Ed25519 per-*event* menjadi hambatan untuk jaringan *low-bandwidth*.

#### 2.3.3 Meshtastic

**Meshtastic** [1] menggunakan AES-256-CTR dengan PSK untuk *channel* dan AES-CCM dengan X25519 untuk DM (sejak v2.5). Keterbatasan kritis yang terdokumentasi: *routing header* dalam *plaintext*, tanda tangan 64-byte terlalu besar untuk paket LoRa 256-byte, dan tidak ada perlindungan terhadap analisis lalu lintas.

#### 2.3.4 Secure Scuttlebutt

**Secure Scuttlebutt** [5] menggunakan Ed25519 untuk menandatangani *append-only log* per identitas, menciptakan jejak digital permanen yang bertentangan dengan kebutuhan privasi pesan sementara.

### 2.4 Kesenjangan Penelitian (*Research Gap*)

Tinjauan literatur mengungkap kesenjangan yang belum ditangani oleh penelitian manapun:

> **Tidak ada sistem komunikasi mesh *offline* yang mengintegrasikan standar NIST LWC (Ascon) ke dalam desain protokol keamanannya untuk mengatasi masalah *cryptographic overhead* pada autentikasi *multi-hop relay* dan enkripsi *metadata routing*.**

Secara lebih spesifik, belum ada penelitian yang membahas:

1. Penggantian Ed25519 *per-packet signatures* (64 byte) dengan Ascon-MAC *per-hop* (16 byte) dalam konteks protokol *store-and-forward mesh*.
2. Penggunaan Ascon-Hash untuk *Epidemic Sync* yang *privacy-preserving* — sinkronisasi tanpa membocorkan konten pesan kepada node perantara.
3. Evaluasi kuantitatif *ciphertext expansion* dan *multi-hop latency* ketika LWC menggantikan kriptografi standar dalam sistem *desktop mesh*.

### 2.5 Novelty dan Kontribusi Akademik

Proyek ini memberikan tiga kontribusi akademik yang dapat diverifikasi:

**Kontribusi 1 — Rancangan Protokol CLAMP:** Protokol *store-and-forward* baru yang menggunakan Ascon sebagai satu-satunya keluarga kriptografi untuk AEAD, hashing, dan MAC berlapis-hop. Inovasi utama: mengganti Ed25519 *per-packet signature* (64 byte) dengan Ascon-MAC *per-hop* (16 byte) — penghematan 75% *overhead* autentikasi dengan keamanan MAC 128-bit yang setara.

```
Perbandingan Overhead Autentikasi:
────────────────────────────────────────────────────
Pendekatan Existing (Meshtastic DM):
  Header + Routing  : ~30 byte (plaintext)
  AES-CCM Overhead  :  8 byte (nonce) + 8 byte (tag)
  XEdDSA Signature  : 64 byte
  Total Overhead    : ~110 byte
  Payload Available : ~146 byte dari 256 byte LoRa

Protokol CLAMP (CARAKA):
  Routing Header    : 13 byte
  Hop Auth (Ascon)  : 17 byte (counter + MAC)
  Ascon Nonce       : 16 byte
  Ascon AEAD Tag    : 16 byte
  Total Overhead    : 62 byte
  Payload Available : ~194 byte (+33% lebih besar)
────────────────────────────────────────────────────
```

**Kontribusi 2 — Analisis Komparatif LWC:** Evaluasi sistematis terhadap Ascon, TinyJambu, GIFT-COFB, Grain-128AEAD, dan PRESENT dalam konteks *desktop mesh networking*, melengkapi literatur yang hanya mengevaluasi LWC pada perangkat tertanam (AVR, ARM Cortex-M).

**Kontribusi 3 — Arsitektur Sinkronisasi *Privacy-Preserving*:** Skema *Epidemic Sync* berbasis vektor Ascon-Hash256 yang memungkinkan node perantara membantu sinkronisasi pesan tanpa pernah dapat membaca kontennya — sebuah properti yang tidak dimiliki oleh sistem yang ada.

---

## BAB III — METODOLOGI PENELITIAN

### 3.1 Pendekatan Metodologi

Penelitian ini menggunakan metodologi **Design Science Research (DSR)** dalam empat siklus: (1) Analisis Kebutuhan dan Literatur, (2) Desain Artefak (Protokol CLAMP), (3) Implementasi dan Pengujian, (4) Evaluasi Kuantitatif dan Pelaporan.

### 3.2 Desain Protokol CLAMP

Protokol CLAMP mendefinisikan struktur paket biner berlapis sebagai berikut:

```
┌──────────────────────────────────────────────────────────────┐
│  ROUTING HEADER — 13 byte                                    │
│  Magic(2) · Version(1) · Type(1) · TTL(1) · PacketID(8)     │
├──────────────────────────────────────────────────────────────┤
│  HOP AUTHENTICATION — 17 byte                                │
│  HopCounter(1) · Ascon-MAC-Tag(16)                           │
├──────────────────────────────────────────────────────────────┤
│  ENCRYPTED PAYLOAD — variable                                │
│  Nonce(16) · Ciphertext(N) · AEAD-Tag(16)                   │
└──────────────────────────────────────────────────────────────┘
```

Hierarki kunci yang digunakan adalah sebagai berikut:

```
X25519 Key Pair (identitas permanen node)
       │
       │ ECDH(my_private, peer_public) → Shared Secret (32B)
       ▼
  Ascon-XOF128(secret || "CARAKA-DM-v1" || session_id || msg_counter)
       │
       ├─► DM-Key (Ascon-AEAD128): enkripsi payload E2EE
       └─► Channel MAC-Key (Ascon-MAC): autentikasi per-hop relay
```

### 3.3 Stack Teknologi

**Tabel 3.1** Teknologi yang Digunakan dalam Pengembangan CARAKA Desktop

| Komponen | Teknologi | Justifikasi |
|---|---|---|
| Bahasa Implementasi | **Rust** | *Memory safety* tanpa GC; ekosistem kriptografi terlengkap |
| Kriptografi | `ascon-aead`, `x25519-dalek` (RustCrypto) | Diaudit komunitas; sesuai standar NIST |
| Derivasi Kunci | Ascon-XOF128 + `hkdf` (HKDF-SHA256) | Standar yang telah dibuktikan keamanannya |
| GUI Desktop | **Tauri v2** | *Cross-platform*; backend Rust native |
| *Local Storage* | **SQLite** via `rusqlite` | Portabel; hanya menyimpan *ciphertext* |
| *Benchmarking* | **Criterion.rs** | *Framework* benchmarking Rust *de-facto* |
| Runtime Async | **Tokio** | Ekosistem async Rust standar |

### 3.4 Model Ancaman (*Threat Model*)

**Tabel 3.2** Model Ancaman dan Mitigasi pada Sistem CARAKA Desktop

| Ancaman | Level | Mitigasi dalam CARAKA |
|:---|:---:|:---|
| *Eavesdropping* (penyadapan *payload*) | 🔴 Tinggi | E2EE Ascon-AEAD128; *ciphertext* tidak dapat dibaca tanpa kunci |
| *Message Tampering* (modifikasi *ciphertext*) | 🔴 Tinggi | Tag AEAD 128-bit; modifikasi menyebabkan dekripsi gagal |
| *Replay Attack* | 🟡 Sedang | LRU Packet ID Cache (512 entri) + jendela *timestamp* ±5 menit |
| *Node Impersonation* | 🟡 Sedang | TOFU model + verifikasi *fingerprint* luar-jaringan |
| *Sybil Attack / Network Flooding* | 🟡 Sedang | TTL maksimum 7 hop + *Trust Score* + *rate limiting* |
| Analisis Lalu Lintas (*Traffic Analysis*) | 🟡 Sedang | *Payload* E2EE; header hanya berisi metadata routing minimal |
| Kompromi Perangkat Fisik | ❌ Di luar cakupan | — |
| Serangan *Post-Quantum* | ❌ Di luar cakupan v0.1 | Direncanakan pada versi 0.3 (Kyber KEM) |

### 3.5 Desain Evaluasi

#### 3.5.1 Level 1 — *Microbenchmark* Kriptografi

**Tabel 3.3** Parameter *Microbenchmark* Kriptografi

| Metrik | Satuan | Kondisi Uji |
|---|---|---|
| *Encryption Throughput* | MB/s | Ukuran pesan: 64B, 256B, 1KB, 4KB, 16KB |
| *Decryption Throughput* | MB/s | Idem |
| Waktu Komputasi MAC | μs | Ascon-MAC untuk input 13 byte (header) |
| Waktu Derivasi Kunci | μs | Ascon-XOF128 vs HKDF-SHA256 |
| *Memory Footprint* | KB | *Peak* RAM per operasi kriptografi |

Algoritma pembanding: **Ascon-AEAD128** vs **AES-256-GCM** vs **ChaCha20-Poly1305**. Platform uji: CPU Intel Core i5/i7 generasi ke-10 atau lebih baru (dengan dan tanpa akselerasi AES-NI).

#### 3.5.2 Level 2 — *Network-Level Benchmark*

**Tabel 3.4** Parameter *Network-Level Benchmark*

| Metrik | Satuan | Keterangan |
|---|---|---|
| *End-to-End Latency* | ms | Dari pengiriman hingga penerimaan, per topologi |
| *Message Delivery Ratio* | % | Dari 1.000 pesan yang dikirim, berapa yang diterima |
| *Ciphertext Expansion* | % | `(ukuran_paket / ukuran_plaintext) × 100%` |
| *Hop Overhead* | byte | Overhead tambahan yang ditambahkan setiap relay |
| *Sync Throughput* | pesan/detik | Kecepatan *Epidemic Sync* antar dua node |

Topologi uji: Linear (5 hop), Star (1 hub + 4 spoke), Mesh acak (10 node, rata-rata 3 tetangga).

**Baseline Perbandingan:**
- **Baseline A (*No-Crypto*):** Protokol CLAMP tanpa enkripsi — mengukur *overhead* jaringan murni.
- **Baseline B (AES-GCM):** Protokol CLAMP dengan AES-256-GCM + HMAC-SHA256 — mengukur selisih LWC vs kriptografi konvensional.

---

## BAB IV — JADWAL DAN LUARAN

### 4.1 Jadwal Pelaksanaan

**Tabel 4.1** Jadwal Pelaksanaan Penelitian

| Minggu | Kegiatan | *Deliverable* |
|:---:|---|---|
| 1 | Studi literatur mendalam; setup *environment* (Rust, Tauri, Node.js) | Ringkasan literatur; *environment* berjalan |
| 2 | Implementasi modul kriptografi: Ascon-AEAD128, Ascon-MAC, Ascon-XOF128 | `crypto.rs` + unit test lengkap |
| 3 | Implementasi manajemen kunci: X25519 *key pair*, ECDH, derivasi DM-Key | `keys.rs` + unit test |
| 4 | Implementasi struktur paket CLAMP: *encode/decode* biner, validasi header | `packet.rs` + unit test |
| 5 | Implementasi *peer discovery*: UDP Broadcast beacon + listener | `discovery.rs` |
| 6 | Implementasi routing: *Controlled Flooding*, TTL, Packet ID Cache, Trust Score | `routing.rs` |
| 7 | Implementasi *store-and-forward* dan *Epidemic Sync* berbasis Ascon-Hash | `store.rs`, `sync.rs` |
| 8 | Implementasi GUI (Tauri) dan integrasi seluruh modul backend | Aplikasi desktop berjalan |
| 9 | Pengujian E2E; *microbenchmark* kriptografi (Criterion.rs) | Dataset *benchmark* kriptografi |
| 10 | *Network benchmark* multi-topologi; analisis data; penulisan makalah | Laporan evaluasi + makalah final |

### 4.2 Luaran yang Diharapkan

1. **Prototipe Aplikasi CARAKA Desktop** — aplikasi fungsional yang dapat diuji pada jaringan LAN nyata.
2. **Spesifikasi Protokol CLAMP v0.1** — dokumen teknis yang mendefinisikan format paket, operasi kriptografi, dan semantik routing secara formal.
3. **Dataset Benchmark** — data evaluasi empiris perbandingan Ascon vs. AES-256-GCM, tersedia sebagai *open data*.
4. **Makalah Teknis** — laporan penelitian dengan format publikasi akademik (abstrak, pendahuluan, tinjauan pustaka, metodologi, hasil, analisis, kesimpulan).
5. **Repository *Open-Source*** — seluruh kode sumber tersedia publik dengan dokumentasi yang memadai untuk replikasi.

---

## DAFTAR PUSTAKA

[1] Meshtastic Project. (2025). *Updated Security Implementation*. https://meshtastic.org/docs/development/reference/encryption-technical/

[2] Berty Technologies. (2024). *Wesh Protocol Technical Documentation*. https://berty.tech/docs/protocol/

[3] Berty Technologies. (2024). *Challenges in Building a Distributed Messaging System*. https://berty.tech/challenges

[4] Briar Project. (2024). *How Briar Works*. https://briarproject.org/how-it-works/

[5] Tarr, D., Lavoie, C., Meyer, A., & Kermarrec, A.-M. (2019). *Secure Scuttlebutt: An Identity-Centric Protocol for Subjective and Decentralized Applications*. Proceedings of ACM ICN '19. https://doi.org/10.1145/3357150.3357396

[6] Meshtastic Project. (2025). *Known Limitations and Future Plans of Meshtastic's Encryption*. https://meshtastic.org/docs/about/overview/encryption/limitations/

[7] NIST. (2023). *Lightweight Cryptography Project*. https://csrc.nist.gov/Projects/lightweight-cryptography

[8] NIST. (2025). *NIST Special Publication 800-232: Ascon-Based Lightweight Cryptography Standards for Constrained Devices*.

[9] Dobraunig, C., Eichlseder, M., Mendel, F., & Schläffer, M. (2021). *Ascon v1.2: Lightweight Authenticated Encryption and Hashing*. IACR ePrint 2021/1574.

[10] Saha, D., et al. (2022). *Birthday-Bound Slide Attacks on TinyJAMBU's Keyed-Permutations*. Proceedings of ASIACRYPT 2022.

[11] Banik, S., Pandey, S. K., Peyrin, T., Sasaki, Y., Sim, S. M., & Todo, Y. (2017). *GIFT: A Small PRESENT — Towards Reaching the Limit of Lightweight Encryption*. IACR ePrint 2017/622.

[12] Khairallah, M. (2021). *Security of COFB against Chosen Ciphertext Attacks*. IACR ePrint 2021/648.

[13] Bogdanov, A., et al. (2007). *PRESENT: An Ultra-Lightweight Block Cipher*. Proceedings of CHES 2007. https://doi.org/10.1007/978-3-540-74735-2_31

[14] Bhargavan, K., & Leurent, G. (2016). *On the Practical (In-)Security of 64-bit Block Ciphers: Collision Attacks on HTTP over TLS and OpenVPN (Sweet32)*. Proceedings of ACM CCS 2016.

[15] Banik, S., et al. (2023). *NIST IR 8454: Status Report on the Final Round of the NIST Lightweight Cryptography Standardization Process*. NIST.

---

## LEMBAR PERNYATAAN KEASLIAN

Dengan mengajukan proposal ini, kami menyatakan bahwa proyek CARAKA Desktop merupakan karya orisinal yang dikerjakan secara mandiri oleh pengusul. Seluruh referensi, data, dan sumber daya yang digunakan telah dicantumkan sesuai ketentuan akademik yang berlaku. Proyek ini tidak pernah diajukan sebelumnya dalam konteks akademik lain.

&nbsp;

[Tempat], [Tanggal]

&nbsp;

Yang Menyatakan,

&nbsp;

&nbsp;

*[Nama Mahasiswa / Kelompok]*

*NIM: [NIM]*

---

*— Akhir Dokumen Proposal Proyek CARAKA Desktop —*
