---

# PROPOSAL TUGAS AKHIR

## CARAKA Desktop: Perancangan dan Implementasi Protokol Komunikasi Mesh Offline Terdesentralisasi Berbasis *Lightweight Cryptography* (Ascon NIST SP 800-232)

---

### IDENTITAS PENGUSUL

| | |
|---|---|
| **Nama Mahasiswa** | [Nama Mahasiswa / Kelompok] |
| **NIM** | [NIM] |
| **Program Studi** | [Nama Program Studi] |
| **Fakultas** | [Nama Fakultas] |
| **Universitas** | [Nama Universitas] |
| **Dosen Pembimbing** | [Nama Dosen Pembimbing] |
| **Dosen Penguji** | [Nama Dosen Penguji] |
| **Semester / T.A.** | [Semester] / [Tahun Akademik] |
| **Tanggal Pengajuan** | Juni 2026 |

---

## I. LATAR BELAKANG

### A. Alasan

Komunikasi digital kontemporer memiliki dependensi yang hampir mutlak terhadap infrastruktur terpusat: server, *Content Delivery Network (CDN)*, penyedia layanan internet, dan tulang punggung jaringan global. Dependensi ini menciptakan *single point of failure* yang secara arsitektural berbahaya dalam tiga skenario utama:

1. **Kegagalan Infrastruktur** — akibat bencana alam, gempa bumi, banjir, atau pemadaman listrik yang melumpuhkan jaringan telekomunikasi justru pada momen kebutuhan komunikasi paling kritis.
2. **Pembatasan Akses Disengaja** — pembatasan atau pemblokiran jaringan oleh pihak berwenang di kondisi tertentu yang memutus komunikasi secara masif.
3. **Degradasi Layanan** — kemacetan jaringan pada kondisi darurat massal ketika ribuan pengguna mencoba berkomunikasi secara bersamaan melalui satu infrastruktur terpusat.

Selain isu ketersediaan, sistem komunikasi terpusat secara inheren mengekspos metadata pengguna kepada penyedia layanan dan potensi pengawasan pihak ketiga — sebuah pelanggaran privasi yang semakin menjadi perhatian serius di era digital.

Jaringan *Peer-to-Peer (P2P) Mesh* menawarkan solusi arsitektural alternatif yang fundamental: setiap perangkat berperan ganda sebagai klien sekaligus *router*, memungkinkan pesan mencapai tujuannya melalui jalur alternatif tanpa bergantung pada satu entitas pusat. Paradigma komunikasi seperti ini relevan untuk skenario koordinasi darurat bencana, komunikasi di daerah terpencil tanpa infrastruktur internet, serta kebutuhan akan privasi komunikasi yang tinggi.

Namun, mengamankan komunikasi pada arsitektur mesh menghadirkan tantangan kriptografi yang fundamental, khususnya pada konteks *multi-hop relay* di mana setiap node perantara turut memproses setiap paket yang melintas. Tantangan utama yang belum terpecahkan secara optimal oleh sistem yang ada meliputi:

1. **Overhead Ukuran Paket** — pada kanal *low-bandwidth* (misalnya LoRa, 256 byte/paket), penggunaan tanda tangan digital konvensional seperti Ed25519 mengkonsumsi 64 byte per paket (>25% kapasitas transmisi) hanya untuk autentikasi.
2. **Kebocoran Metadata Routing** — banyak sistem membiarkan *routing header* dalam bentuk *plaintext* agar node perantara dapat meneruskan paket, sehingga pihak ketiga dapat memetakan topologi jaringan melalui analisis lalu lintas pasif.
3. **Manajemen Kunci pada Jaringan *Delay-Tolerant*** — mekanisme *Perfect Forward Secrecy* berbasis *Double Ratchet* memerlukan urutan pesan yang terjamin, sebuah asumsi yang tidak dapat dipertahankan dalam jaringan offline dengan penundaan tak tentu.

Berdasarkan permasalahan tersebut, penelitian ini bertujuan merancang dan mengimplementasikan **CARAKA Desktop** (*Cryptographically Authenticated Relay Architecture for Knowledge and Autonomy*) — sebuah aplikasi komunikasi desktop berbasis jaringan mesh P2P *offline-first* yang dirancang dari awal dengan keamanan kriptografi modern dan efisiensi tinggi.

---

### B. Komparasi dengan Tools Serupa

Sistem komunikasi *offline* yang telah ada saat ini menghadapi dilema mendasar yang belum terpecahkan antara keamanan dan efisiensi jaringan. Berikut adalah tinjauan kritis terhadap empat sistem yang paling relevan:

#### Meshtastic

**Meshtastic** adalah platform mesh *off-grid* berbasis radio LoRa yang berjalan pada mikrokontroler ESP32/NRF52. Sistem ini menggunakan AES-256-CTR dengan *Pre-Shared Key (PSK)* untuk enkripsi channel, dan AES-CCM dengan pertukaran kunci X25519 untuk *Direct Messages* (sejak firmware v2.5).

**Keterbatasan kritis yang terdokumentasi sendiri oleh Meshtastic:**
- *Routing header* dikirimkan dalam *plaintext* untuk memungkinkan relay oleh node yang tidak memiliki kunci channel, sehingga pihak ketiga dapat memetakan topologi jaringan secara pasif.
- Tanda tangan XEdDSA (berbasis Ed25519) untuk DM mengkonsumsi 64 byte per paket — lebih dari 25% total kapasitas transmisi LoRa (256 byte), secara langsung membatasi kapasitas payload.
- *Pre-Shared Key* channel yang statis berarti kompromi satu node membocorkan seluruh riwayat channel.
- Tidak ada perlindungan terhadap analisis lalu lintas (*traffic analysis*).

#### Berty / Wesh Protocol

**Berty** adalah protokol komunikasi *offline-first* terdesentralisasi yang dibangun di atas IPFS dan OrbitDB. Menggunakan X25519 untuk enkripsi dan Ed25519 untuk tanda tangan per-*event*.

**Keterbatasan:**
- Berty secara eksplisit mengakui bahwa protokolnya belum siap untuk data sensitif tinggi dan belum sepenuhnya diaudit.
- Overhead Ed25519 per-*event* yang direplikasi melalui jaringan *gossip* menghasilkan beban signifikan pada jaringan *low-bandwidth*.
- Sinkronisasi berbasis OrbitDB/IPFS memiliki *overhead* protokol yang besar.

#### Briar

**Briar** adalah aplikasi pesan P2P yang dirancang untuk aktivis dan jurnalis. Mengimplementasikan konstruksi mirip-Signal dengan *Perfect Forward Secrecy* berbasis *Double Ratchet* dan AES-GCM.

**Keterbatasan:**
- Routing *offline* Briar bergantung sepenuhnya pada *social graph*: pesan hanya dapat diteruskan melalui kontak bersama (*mutual contacts*), bukan jaringan mesh generik.
- Membatasi jangkauan komunikasi dalam skenario *ad-hoc* di mana anggota jaringan belum saling mengenal.

#### Secure Scuttlebutt (SSB)

**Secure Scuttlebutt** adalah protokol gossip berbasis *append-only signed log* per identitas. Setiap entri log ditandatangani dengan Ed25519 dan membentuk rantai hash yang tidak dapat dipalsukan.

**Keterbatasan:**
- Menciptakan rekam jejak digital yang permanen dan dapat diatribusikan — secara fundamental bertentangan dengan kebutuhan privasi pesan sementara.
- Sinkronisasi memerlukan seluruh riwayat log, tidak efisien untuk komunikasi pesan biasa.

#### Tabel Perbandingan Komprehensif

**Tabel 1.** Perbandingan Sistem Komunikasi *Offline* yang Ada


| Sistem | *Overhead* Autentikasi | Keamanan Metadata | Dukungan *Offline* |
|---|---|---|---|
| Meshtastic | 64 byte (Ed25519) | Tidak — *header* bersifat *plaintext* | Penuh |
| Berty | Ed25519 per-*event* | Tidak — *gossip* terbuka | Terbatas |
| Briar | AES-GCM + Signal | Ya — via Tor (hanya *online*) | Terbatas (*social graph*) |
| Secure Scuttlebutt | Ed25519 per-log | Tidak — log bersifat permanen | Penuh |
| **CARAKA (Diusulkan)** | **17 byte (Ascon-MAC)** | **Ya — MAC per-hop** | **Penuh** |

---

### C. Kebutuhan Keamanan

Berdasarkan analisis sistem yang ada dan karakteristik jaringan mesh *offline*, CARAKA Desktop dirancang untuk memenuhi kebutuhan keamanan berikut:

#### 1. Kerahasiaan Data (*Confidentiality*)

Seluruh konten pesan harus terenkripsi secara *End-to-End (E2EE)* sebelum meninggalkan perangkat pengirim. Node perantara (relay) tidak boleh memiliki kemampuan untuk membaca isi pesan dalam kondisi apapun — termasuk pada proses sinkronisasi *store-and-forward*.

#### 2. Integritas dan Autentikasi Pesan (*Integrity & Authentication*)

Setiap paket harus dilindungi dari modifikasi oleh pihak tidak berwenang. Mekanisme autentikasi harus beroperasi pada **dua lapisan**:
- **Lapisan per-hop:** Relay node memvalidasi integritas paket sebelum meneruskannya, tanpa perlu mendekripsi payload.
- **Lapisan end-to-end:** Penerima akhir memvalidasi integritas dan keaslian payload setelah dekripsi.

#### 3. Perlindungan terhadap Replay Attack

Paket yang sama tidak boleh dapat diproses ulang oleh sistem. Mekanisme deduplication berbasis *Packet ID cache* dengan jendela waktu (*timestamp window*) diperlukan untuk mencegah serangan replay.

#### 4. Privasi Metadata Routing

Analisis lalu lintas pasif oleh pihak ketiga harus diminimalkan. Routing header harus sesedikit mungkin mengekspos informasi tentang identitas pengirim, penerima, atau topologi jaringan.

#### 5. Resistensi terhadap Sybil Attack dan Network Flooding

Sistem harus mampu membatasi dampak node berbahaya yang mencoba membanjiri jaringan atau menyamar sebagai banyak identitas. Mekanisme *Trust Score* berbasis perilaku dan pembatasan TTL menjadi mitigasi utama.

#### 6. Forward Secrecy (Best-Effort)

Kompromi kunci di masa depan tidak boleh secara retroaktif membocorkan pesan-pesan lama. Implementasi *full Double Ratchet* tidak realistis untuk lingkungan *delay-tolerant*, sehingga CARAKA mengadopsi **Session-Based Forward Secrecy** dengan `session_id` dan `msg_counter` yang unik per sesi.

#### Model Ancaman (*Threat Model*)

**Tabel 2.** Model Ancaman dan Mitigasi CARAKA Desktop

| Kategori | Ancaman Spesifik | Level Risiko | Mitigasi dalam CARAKA |
|:---|:---|:---:|:---|
| *Confidentiality* | Penyadapan *payload* (*Eavesdropping*) | Tinggi | Enkripsi E2EE Ascon-AEAD128; *ciphertext* tidak terbaca tanpa kunci |
| *Integrity* | Modifikasi *ciphertext* (*Tampering*) | Tinggi | *AEAD Tag* 128-bit; modifikasi menyebabkan dekripsi gagal |
| *Replay* | Pengiriman ulang paket lama (*Replay Attack*) | Sedang | *Packet ID Cache* (LRU-512) + jendela *timestamp* ±5 menit |
| *Routing* | Analisis lalu lintas (*Traffic Analysis*) | Sedang | *Payload* E2EE; header hanya berisi metadata routing minimal |
| *Authentication* | Penyamaran identitas (*Impersonation*) | Sedang | Model TOFU + verifikasi *fingerprint* luar-jaringan |
| *Availability* | Banjir jaringan (*Network Flooding*) | Sedang | TTL maksimum 7 hop + *Trust Score* + *rate limiting* |
| *Availability* | *Sybil Attack* | Sedang | *Trust Score* berbasis perilaku; node baru dimulai dengan skor rendah |
| *Integrity* | Manipulasi *routing* | Rendah | Hop-MAC divalidasi tiap relay; paket tidak valid dibuang |
| *Confidentiality* | Kompromi perangkat fisik | Di luar cakupan | Tidak ditangani pada versi ini |
| *Confidentiality* | Serangan kuantum (*Post-Quantum*) | Di luar cakupan | Direncanakan pada pengembangan versi selanjutnya |

---

### D. Algoritma

Pada tahun 2013, *National Institute of Standards and Technology (NIST)* memulai proses standardisasi *Lightweight Cryptography (LWC)* untuk mengatasi ketidaksesuaian fundamental antara algoritma kriptografi standar (AES-GCM, SHA-256) dengan keterbatasan sumber daya perangkat tertanam. Setelah satu dekade seleksi ketat, NIST mengumumkan terpilihnya **keluarga Ascon** pada Februari 2023, yang kemudian dipublikasikan sebagai **NIST Special Publication 800-232** pada Agustus 2025.

#### Analisis Komparatif Algoritma Kandidat LWC

**Tabel 3.** Matriks Perbandingan Algoritma *Lightweight Cryptography* Finalis NIST

| Kriteria | ASCON | TinyJambu | GIFT-COFB | Grain-128AEAD | PRESENT |
|:---|:---:|:---:|:---:|:---:|:---:|
| Status NIST | Standar (SP 800-232) | Finalis (tidak dipilih) | Finalis (tidak dipilih) | Finalis (tidak dipilih) | ISO/IEC 29192-2 |
| Tingkat Keamanan | 128-bit | ~120-bit | ~64-bit (efektif) | 128-bit | 80- atau 128-bit |
| Kapabilitas | AEAD + Hash + XOF | AEAD saja | AEAD saja | AEAD saja | *Block Cipher* saja |
| Isu Kriptanalisis | Minimal | *Birthday-bound slide attacks* | *Effective* 64-bit *tag forgery* | Batas panjang *keystream* | *Sweet32* (blok 64-bit) |
| Dukungan Rust | Tersedia (`ascon-aead`, `ascon`) | Tidak ada *crate* resmi | Tidak ada *crate* resmi | Terbatas | Minimal |
| Kesesuaian *Desktop Mesh* | Sangat sesuai | Tidak sesuai | Terbatas | Dengan syarat ketat | Tidak sesuai |

#### Justifikasi Pemilihan ASCON

**Ascon** merupakan *sponge-based permutation* yang dibangun di atas *state* internal 320-bit yang dioperasikan dalam mode *duplex*. Keluarga ini mencakup:

- **Ascon-AEAD128:** *Authenticated Encryption with Associated Data* (kunci 128-bit, nonce 128-bit, tag 128-bit)
- **Ascon-Hash256:** Fungsi hash kriptografi 256-bit
- **Ascon-XOF128 / Ascon-CXOF128:** *Extendable Output Function* untuk derivasi kunci (KDF)

ASCON dipilih sebagai satu-satunya primitif kriptografi inti CARAKA karena:
1. Satu-satunya kandidat yang memenuhi *seluruh* kriteria: keamanan 128-bit, standar NIST aktif, kapabilitas AEAD+Hash+XOF dalam satu keluarga.
2. Ekosistem implementasi Rust yang paling matang dan terawat (RustCrypto Project).
3. *Compact state* 320-bit yang efisien untuk CPU *general-purpose* maupun perangkat tertanam.
4. Landasan analisis kriptografi paling luas di antara semua kandidat finalis.

#### Stack Kriptografi CARAKA

```
┌────────────────────────────────────────────────────────────────┐
│                   CARAKA Cryptographic Stack                   │
├────────────────────┬───────────────────────────────────────────┤
│ Fungsi             │ Primitif                                   │
├────────────────────┼───────────────────────────────────────────┤
│ Enkripsi Payload   │ Ascon-AEAD128 (NIST SP 800-232)           │
│ Fungsi Hash        │ Ascon-Hash256 / Ascon-XOF128               │
│ MAC per-Hop        │ Ascon-MAC (berbasis Ascon permutation)      │
│ Key Exchange       │ X25519 (ECDH) via `x25519-dalek`           │
│ KDF                │ Ascon-XOF128 / HKDF-SHA256                 │
│ Node Identity      │ Ed25519 Public Key (hanya untuk identitas) │
└────────────────────┴───────────────────────────────────────────┘
```

---

### E. Tinjauan Rancangan

CARAKA Desktop mengimplementasikan **Protokol CLAMP** (*Compact Lightweight Authenticated Mesh Protocol*) — sebuah protokol lapisan aplikasi *store-and-forward* yang dirancang khusus untuk jaringan mesh *offline*.

#### Model Kunci

CLAMP mendefinisikan dua konteks kunci yang berbeda:

1. **Direct Message Key (DM-Key):** Diderivasi dari ECDH antara dua node.
   ```
   shared_secret = X25519(my_private_key, peer_public_key)
   dm_key = Ascon-XOF128(shared_secret || "CARAKA-DM-v1" || sender_id || receiver_id)
   ```

2. **Channel Key (CH-Key):** Kunci simetris yang didistribusikan *out-of-band* (QR Code) kepada anggota channel, digunakan untuk autentikasi MAC per-hop.

#### Struktur Paket CLAMP

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
Total Overhead Kriptografi: 62 byte
```

#### Perbandingan Overhead dengan Sistem Existing

```
Meshtastic (untuk paket LoRa 256 byte):
  Header + Routing : ~30 byte (plaintext)
  AES-CCM Overhead :  8 byte (nonce) + 8 byte (tag)
  XEdDSA Signature : 64 byte
  Total Overhead   : ~110 byte → hanya ~146 byte untuk payload

CARAKA / Protokol CLAMP:
  Routing Header   : 13 byte
  Hop Auth (Ascon) : 17 byte (counter + MAC)
  Ascon Nonce      : 16 byte
  Ascon AEAD Tag   : 16 byte
  Total Overhead   : 62 byte → ~194 byte untuk payload (+33%)
```

#### Mekanisme Jaringan

**Peer Discovery:** Setiap node mengirimkan beacon `CARAKA-HELLO` via UDP Broadcast setiap 30 detik, berisi Node ID (X25519 Public Key) dan alamat TCP. Pengguna juga dapat menambahkan peer secara manual atau via QR Code.

**Routing — Controlled Flooding:** Paket diteruskan ke seluruh peer yang terhubung kecuali pengirim, dengan TTL maksimum 7 hop. *Trust Score* berbasis perilaku digunakan untuk memfilter paket dari node tidak terpercaya, mengurangi risiko Sybil Attack.

**Store-and-Forward — Epidemic Sync:** Ketika dua node pertama kali terhubung, mereka menukar *Message Fingerprint Vector* berisi daftar `Ascon-Hash256` dari pesan yang tersimpan. Hanya *ciphertext* yang dipertukarkan untuk mengisi gap, sehingga node perantara tidak pernah dapat membaca konten pesan.

---

### F. Target Output

Penelitian ini menargetkan lima luaran yang dapat diverifikasi:

1. **Prototipe Aplikasi CARAKA Desktop** — Aplikasi desktop fungsional (Windows/Linux) yang dapat diuji pada jaringan LAN nyata, mencakup antarmuka pengguna grafis (GUI) berbasis Tauri, fitur *peer discovery*, pengiriman pesan terenkripsi E2EE, dan sinkronisasi *offline*.

2. **Spesifikasi Protokol CLAMP v0.1** — Dokumen teknis formal yang mendefinisikan format paket biner, operasi kriptografi, semantik routing, dan model kunci secara lengkap dan dapat direplikasi oleh peneliti independen.

3. **Dataset Evaluasi Empiris** — Data *benchmark* terstruktur yang membandingkan performa Ascon-AEAD128 vs. AES-256-GCM vs. ChaCha20-Poly1305 pada level *microbenchmark* kriptografi dan *network-level benchmark* multi-hop, tersedia sebagai *open data*.

4. **Laporan Penelitian Akademik** — Laporan dengan format publikasi akademik mencakup: tinjauan pustaka, analisis komparatif algoritma LWC, deskripsi desain protokol, hasil evaluasi kuantitatif, analisis, dan kesimpulan.

5. **Repository *Open-Source*** — Seluruh kode sumber (core library Rust, protokol, GUI) tersedia publik di GitHub dengan dokumentasi yang memadai untuk replikasi dan audit independen.

---

## II. RUMUSAN MASALAH

Berdasarkan latar belakang yang telah dipaparkan, rumusan masalah penelitian ini adalah sebagai berikut:

1. **Bagaimana merancang protokol komunikasi mesh terdesentralisasi berbasis Ascon (NIST LWC) yang meminimalkan *cryptographic overhead* per-hop untuk autentikasi *relay* pada jaringan *offline*?**

   Permasalahan ini berkaitan dengan desain struktur paket, pemilihan primitif kriptografi, dan mekanisme autentikasi berlapis-hop (Ascon-MAC) yang mampu menggantikan tanda tangan Ed25519 per-paket dengan penghematan overhead yang signifikan.

2. **Bagaimana mengimplementasikan skema autentikasi *multi-hop* yang efisien menggunakan Ascon-MAC, dan seberapa besar reduksi *overhead* yang dapat dicapai secara empiris dibandingkan dengan pendekatan Ed25519 yang digunakan sistem existing?**

   Permasalahan ini berkaitan dengan implementasi konkret dan pengukuran kuantitatif penghematan byte overhead per paket (hipotesis: dari 64 byte Ed25519 menjadi 16 byte Ascon-MAC, penghematan 75%).

3. **Bagaimana performa implementasi berbasis Ascon (NIST LWC) dibandingkan implementasi berbasis AES-256-GCM (kriptografi konvensional) dalam parameter: *encryption throughput*, *decryption throughput*, ukuran overhead paket, *end-to-end latency*, dan *memory footprint* pada platform desktop?**

   Permasalahan ini mengisi kesenjangan dalam literatur yang selama ini hanya mengevaluasi Ascon pada perangkat tertanam (AVR, ARM Cortex-M), bukan pada CPU *general-purpose* untuk aplikasi desktop.

4. **Bagaimana merancang mekanisme sinkronisasi *store-and-forward* yang *privacy-preserving* menggunakan Ascon-Hash256 untuk pertukaran *fingerprint* pesan tanpa mengekspos konten kepada node perantara dalam jaringan *delay-tolerant*?**

   Permasalahan ini berkaitan dengan desain protokol *Epidemic Sync* yang memungkinkan sinkronisasi pesan antar node yang baru terhubung tanpa melanggar properti E2EE yang telah dijamin pada lapisan enkripsi.

---

## III. DESAIN ARSITEKTUR

### A. Analisis Objek Penelitian

Objek penelitian ini adalah **sistem komunikasi desktop berbasis jaringan P2P Mesh yang beroperasi dalam mode *offline-first***. Objek ini didefinisikan oleh karakteristik-karakteristik berikut:

#### 1. Domain Masalah

Sistem komunikasi *offline* berbasis mesh berada di persimpangan tiga domain ilmu:
- **Kriptografi Terapan:** Penerapan algoritma kriptografi ringan (*Lightweight Cryptography*) dalam protokol komunikasi nyata.
- **Sistem Terdistribusi:** Desain protokol tanpa otoritas pusat (*decentralized*) dengan mekanisme *store-and-forward* dan *epidemic sync*.
- **Rekayasa Perangkat Lunak:** Implementasi sistem yang aman, efisien, dan dapat digunakan dalam kondisi nyata.

#### 2. Karakteristik Lingkungan Target

| Parameter | Spesifikasi |
|:---|:---|
| Platform | Desktop (Windows 10/11 dan Linux Ubuntu 22.04 LTS) |
| *Transport* | UDP untuk *peer discovery*; TCP untuk pengiriman data |
| Skala Jaringan | 2 hingga 15 node dalam satu segmen LAN/*Wi-Fi* |
| Topologi Jaringan | *Ad-hoc mesh* (tanpa hierarki tetap) |
| Konektivitas | *Offline-first*; tidak memerlukan koneksi internet |
| Bahasa Implementasi | Rust (*backend* inti + kriptografi) |
| *Framework* GUI | Tauri v2 (*cross-platform desktop*) |

#### 3. Subjek Evaluasi

Subjek utama yang dievaluasi dalam penelitian ini adalah:
- **Protokol CLAMP** — kebenaran, keamanan, dan efisiensi protokol yang dirancang.
- **Implementasi Ascon** — performa kriptografi Ascon-AEAD128 vs. AES-256-GCM pada platform desktop.
- **Mekanisme Multi-hop** — efisiensi autentikasi per-hop Ascon-MAC vs. Ed25519 signature per-paket.

#### 4. Batasan Penelitian

Untuk menjaga ketercapaian dalam kerangka waktu yang tersedia, penelitian ini dibatasi sebagai berikut:
- **Platform target:** Aplikasi desktop (Windows dan Linux) dalam jaringan LAN/Wi-Fi.
- **Skala jaringan uji:** 2 hingga 15 node dalam satu subnet.
- **Cakupan kriptografi:** Seluruh implementasi menggunakan *library* terawat dari RustCrypto Project.
- **Di luar cakupan:** *Post-Quantum Cryptography*, mekanisme konsensus *blockchain*, infrastruktur *cloud*, kecerdasan buatan, dan transport radio (LoRa, Bluetooth).

---

### B. Analisis Kebutuhan

#### Kebutuhan Fungsional

Berikut adalah kebutuhan fungsional yang harus dipenuhi oleh sistem CARAKA Desktop:

**F-01 — Manajemen Identitas Node**
- Sistem harus dapat membangkitkan pasangan kunci X25519 secara aman sebagai identitas permanen node.
- Setiap node harus memiliki Node ID (X25519 Public Key, 32 byte) dan Node Fingerprint (Ascon-Hash256 dari Node ID) yang dapat dibagikan kepada pengguna lain untuk verifikasi.

**F-02 — Peer Discovery**
- Sistem harus mendukung penemuan node lain secara otomatis melalui UDP Broadcast pada jaringan lokal (beacon interval: 30 detik).
- Sistem harus mendukung penambahan peer secara manual melalui input alamat IP:Port atau pemindaian QR Code.

**F-03 — Enkripsi End-to-End untuk Direct Message**
- Sistem harus mengenkripsi seluruh konten pesan menggunakan Ascon-AEAD128 dengan kunci yang diderivasi dari ECDH (X25519) sebelum paket meninggalkan perangkat pengirim.
- Hanya penerima yang dituju yang dapat mendekripsi dan membaca isi pesan.

**F-04 — Autentikasi Multi-Hop (Hop-MAC)**
- Setiap paket yang diteruskan oleh node relay harus dilindungi dengan Ascon-MAC yang divalidasi di setiap lompatan.
- Node relay yang tidak dapat memvalidasi Hop-MAC harus membuang paket secara diam-diam (*drop silently*).

**F-05 — Mesh Routing dengan Controlled Flooding**
- Sistem harus dapat meneruskan paket melalui beberapa node perantara (multi-hop) menggunakan mekanisme *Controlled Flooding*.
- TTL maksimum adalah 7 hop; paket dengan TTL = 0 tidak diteruskan lebih lanjut.

**F-06 — Deduplication dan Anti-Replay**
- Sistem harus mempertahankan *Packet ID Cache* (LRU-512) untuk mencegah pemrosesan ulang paket yang sama.
- Paket dengan timestamp lebih dari ±5 menit dari waktu sistem lokal harus dibuang secara otomatis.

**F-07 — Store-and-Forward**
- Pesan yang ditujukan kepada node yang sedang offline harus disimpan secara terenkripsi di penyimpanan lokal (SQLite terenkripsi).
- Pesan tersebut harus dikirimkan secara otomatis ketika node tujuan kembali online.

**F-08 — Epidemic Sync**
- Ketika dua node pertama kali terhubung, mereka harus dapat menyinkronkan pesan yang belum diterima melalui pertukaran *Message Fingerprint Vector* (daftar Ascon-Hash256).
- Hanya *ciphertext* yang dipertukarkan selama proses sinkronisasi; node perantara tidak pernah memiliki akses ke plaintext.

**F-09 — Antarmuka Pengguna**
- Sistem harus menyediakan GUI yang memungkinkan pengguna: melihat daftar peer aktif, mengirim dan menerima pesan, melihat status koneksi, dan mengatur identitas.

---

#### Kebutuhan Non-Fungsional

**NF-01 — Keamanan**
- Seluruh implementasi kriptografi harus menggunakan *library* yang telah diaudit komunitas (RustCrypto Project) tanpa implementasi primitif kriptografi secara mandiri (*no custom crypto*).
- Properti keamanan yang harus terpenuhi: *confidentiality*, *integrity*, *authenticity*, *replay resistance*, dan *best-effort forward secrecy*.

**NF-02 — Efisiensi**
- Total overhead kriptografi per paket tidak boleh melebihi 62 byte.
- *Latency* pengiriman pesan pada jaringan LAN (hop tunggal) tidak boleh melebihi 100 ms dalam kondisi normal.
- *Encryption throughput* Ascon-AEAD128 untuk pesan berukuran 256 byte harus dapat diukur dan didokumentasikan.

**NF-03 — Keandalan (*Reliability*)**
- *Message Delivery Ratio* pada topologi linear 5 hop harus ≥ 95% untuk 1000 pesan yang dikirim dalam kondisi jaringan normal.
- Sistem harus beroperasi secara stabil tanpa *crash* dalam sesi pengujian selama minimal 1 jam.

**NF-04 — Portabilitas**
- Aplikasi harus dapat dikompilasi dan berjalan pada Windows 10/11 dan Ubuntu 22.04 LTS tanpa modifikasi kode yang signifikan.

**NF-05 — *Maintainability***
- Seluruh modul harus memiliki unit test dengan *code coverage* minimal 80% pada fungsi kriptografi inti.
- Kode sumber harus terdokumentasi dengan *doc-comment* standar Rust.

**NF-06 — Privasi**
- Sistem tidak boleh mengirimkan data apapun ke server eksternal atau pihak ketiga.
- Seluruh data tersimpan secara lokal dan terenkripsi.

---

#### Kebutuhan Perangkat Keras

**Tabel 4.** Spesifikasi Minimum Perangkat Keras

| Komponen | Spesifikasi Minimum | Spesifikasi Rekomendasi |
|:---|:---|:---|
| Prosesor | *Dual-core* 64-bit, 1,6 GHz | *Quad-core* 64-bit, 2,0 GHz atau lebih |
| Memori (RAM) | 2 GB | 4 GB atau lebih |
| Penyimpanan | 500 MB ruang kosong | 2 GB atau lebih (untuk log dan *store*) |
| Antarmuka Jaringan | *Network Interface Card* (NIC) Wi-Fi atau Ethernet | Ethernet Gigabit untuk *benchmark* |
| Sistem Operasi | Windows 10 (64-bit) / Ubuntu 22.04 LTS | Windows 11 / Ubuntu 24.04 LTS |

**Tabel 4a.** Spesifikasi Lingkungan Pengujian *Benchmark*

| Komponen | Spesifikasi Perangkat Uji |
|:---|:---|
| Prosesor | Intel Core i5/i7 Generasi ke-10 atau lebih baru |
| Fitur CPU | Dukungan instruksi AES-NI (untuk perbandingan AES-GCM vs. Ascon yang adil) |
| Memori (RAM) | 8 GB DDR4 |
| Jaringan | LAN Ethernet 100 Mbps (simulasi topologi *multi-hop*) |
| Jumlah Node | Minimal 5 mesin fisik atau mesin virtual (VM) untuk *benchmark multi-hop* |

---

#### Kebutuhan Perangkat Lunak

**Tabel 5.** *Stack* Teknologi Perangkat Lunak CARAKA Desktop

| Komponen | Teknologi | Versi | Justifikasi |
|:---|:---|:---:|:---|
| Bahasa Implementasi | Rust | 1.78+ (*stable*) | *Memory safety* tanpa *garbage collector*; ekosistem kriptografi terlengkap |
| Pustaka Kriptografi | `ascon-aead` (RustCrypto) | 0.2.x | Implementasi Ascon-AEAD128 yang telah diaudit komunitas |
| Pertukaran Kunci | `x25519-dalek` (Dalek Cryptography) | 2.x | Implementasi X25519 yang telah diaudit secara formal |
| Derivasi Kunci (KDF) | `hkdf` + `sha2` (RustCrypto) | terbaru | Implementasi HKDF-SHA256 standar |
| *Runtime* Asinkron | Tokio | 1.x | *Framework async* Rust *de-facto* untuk I/O jaringan |
| *Framework* GUI | Tauri | v2.x | *Cross-platform*; integrasi *backend* Rust secara *native* |
| Basis Data Lokal | SQLite via `rusqlite` | terbaru | Penyimpanan lokal portabel; hanya menyimpan *ciphertext* |
| *Benchmarking* | Criterion.rs | 0.5.x | *Framework benchmarking* statistik untuk Rust |
| Serialisasi | `bincode` atau format biner kustom | terbaru | Efisiensi serialisasi untuk protokol biner |
| Sistem *Build* | Cargo (Rust) + Node.js (Tauri) | — | Sistem *build* standar ekosistem Rust |

**Tabel 5a.** Dependensi Perangkat Pengembangan

| Perangkat | Fungsi |
|:---|:---|
| `cargo-criterion` | Antarmuka baris perintah untuk menjalankan dan melaporkan *benchmark* |
| `rustfmt` | *Formatter* kode standar Rust |
| `clippy` | *Linter* analisis kode statis Rust |
| `cargo-tarpaulin` | Pengukuran *code coverage* pada unit *test* |
| Wireshark | Analisis paket jaringan untuk verifikasi enkripsi |
| iperf3 | Pengukuran *throughput* jaringan sebagai *baseline* |

---

### C. Desain Rancangan

#### 1. Arsitektur Sistem Keseluruhan

CARAKA Desktop dirancang dengan arsitektur berlapis (*layered architecture*) yang memisahkan concern kriptografi, protokol, jaringan, penyimpanan, dan antarmuka pengguna:

```
┌─────────────────────────────────────────────────────────────┐
│                     PRESENTATION LAYER                      │
│              Tauri GUI (HTML/CSS/JS + Tauri API)            │
├─────────────────────────────────────────────────────────────┤
│                    APPLICATION LAYER                        │
│    Message Handler · Session Manager · UI Event Bridge      │
├───────────────────┬─────────────────────────────────────────┤
│   PROTOCOL LAYER  │             STORAGE LAYER               │
│   CLAMP Protocol  │   SQLite (Encrypted) · LRU Cache        │
│   Packet Encoder  │   Message Store · Fingerprint Vector    │
│   Packet Decoder  │                                         │
├───────────────────┼─────────────────────────────────────────┤
│   NETWORK LAYER   │          CRYPTOGRAPHY LAYER             │
│   UDP Discovery   │   Ascon-AEAD128 · Ascon-Hash256         │
│   TCP Transport   │   Ascon-MAC · X25519 ECDH               │
│   Mesh Routing    │   Ascon-XOF128 (KDF) · Ed25519          │
│   Trust Score     │                                         │
└───────────────────┴─────────────────────────────────────────┘
```

#### 2. Desain Modul Kode (Rust Workspace)

```
caraka-desktop/
├── caraka-core/          (library: kriptografi + protokol)
│   ├── src/
│   │   ├── crypto.rs     (Ascon-AEAD128, Ascon-MAC, Ascon-XOF128)
│   │   ├── keys.rs       (X25519 key pair, ECDH, DM-Key derivation)
│   │   ├── packet.rs     (struktur data + encode/decode CLAMP packet)
│   │   ├── routing.rs    (Controlled Flooding, TTL, Trust Score)
│   │   ├── discovery.rs  (UDP Broadcast beacon + listener)
│   │   ├── transport.rs  (TCP server/client dengan framing)
│   │   ├── store.rs      (SQLite encrypted message store)
│   │   └── sync.rs       (Epidemic Sync dengan Fingerprint Vector)
│   └── benches/
│       ├── crypto_bench.rs    (microbenchmark kriptografi)
│       └── network_bench.rs   (network-level benchmark)
└── caraka-app/           (binary: Tauri GUI application)
    ├── src-tauri/        (backend Rust)
    └── src/              (frontend HTML/CSS/JS)
```

#### 3. Alur Kerja Pengiriman Pesan End-to-End

```
Pengirim (Alice)               Relay (Bob)          Penerima (Charlie)
     │                              │                       │
     │ [1] Derivasi DM-Key          │                       │
     │     X25519(Alice_priv,       │                       │
     │           Charlie_pub)       │                       │
     │                              │                       │
     │ [2] Enkripsi Payload         │                       │
     │     Ascon-AEAD128(dm_key,    │                       │
     │       nonce, plaintext, aad) │                       │
     │                              │                       │
     │ [3] Buat CLAMP Packet        │                       │
     │     Header + Hop-MAC         │                       │
     │     (Ascon-MAC dengan CH-Key)│                       │
     │                              │                       │
     │ ──── CLAMP Packet ──────────►│                       │
     │                              │                       │
     │                              │ [4] Validasi Hop-MAC  │
     │                              │     (Ascon-MAC verify)│
     │                              │     Kurangi TTL       │
     │                              │     Perbarui Hop-MAC  │
     │                              │                       │
     │                              │ ── CLAMP Packet ─────►│
     │                              │                       │
     │                              │                 [5] Validasi Hop-MAC
     │                              │                     Dekripsi Payload
     │                              │                     Ascon-AEAD128
     │                              │                     Tampilkan pesan
```

#### 4. Desain Evaluasi Empiris

Untuk memvalidasi kontribusi akademik, evaluasi dilakukan pada dua level:

**Level 1 — *Microbenchmark* Kriptografi** (menggunakan Criterion.rs)

**Tabel 6.** Parameter *Microbenchmark* Kriptografi

| Metrik | Satuan | Kondisi Uji |
|:---|:---:|:---|
| *Encryption Throughput* | MB/s | Ukuran pesan: 64 B, 256 B, 1 KB, 4 KB, 16 KB |
| *Decryption Throughput* | MB/s | Sama dengan kondisi enkripsi |
| Waktu Komputasi MAC | μs | Ascon-MAC untuk input 13 byte (*header*) |
| Waktu Derivasi Kunci | μs | Ascon-XOF128 dibandingkan HKDF-SHA256 |
| *Memory Footprint* | KB | Puncak penggunaan RAM per operasi kriptografi |

Algoritma pembanding: Ascon-AEAD128 vs. AES-256-GCM vs. ChaCha20-Poly1305.

**Level 2 — *Network-Level Benchmark*** (multi-node LAN)

**Tabel 7.** Parameter *Network-Level Benchmark*

| Metrik | Satuan | Keterangan |
|:---|:---:|:---|
| *End-to-End Latency* | ms | Dari pengiriman hingga penerimaan, per konfigurasi topologi |
| *Message Delivery Ratio* | % | Dari 1.000 pesan yang dikirim, berapa yang diterima |
| *Ciphertext Expansion* | % | (ukuran paket CARAKA / ukuran *plaintext*) × 100% |
| *Hop Overhead* | byte | *Overhead* tambahan yang ditambahkan setiap node relay |
| *Sync Throughput* | pesan/detik | Kecepatan sinkronisasi *Epidemic Sync* antar dua node |

Topologi uji yang digunakan: Linear (5 hop), *Star* (1 hub + 4 *spoke*), *Mesh* acak (10 node, rata-rata 3 tetangga per node).

Konfigurasi *baseline* perbandingan:
- **Baseline A (*No-Crypto*):** Protokol CLAMP tanpa enkripsi, untuk mengukur *overhead* jaringan murni.
- **Baseline B (AES-GCM):** Protokol CLAMP dengan AES-256-GCM + HMAC-SHA256, untuk mengukur selisih antara LWC dan kriptografi konvensional.

---

## DAFTAR PUSTAKA

[1] Meshtastic Project. (2025). *Updated Security Implementation*. https://meshtastic.org/docs/development/reference/encryption-technical/

[2] Berty Technologies. (2024). *Wesh Protocol Technical Documentation*. https://berty.tech/docs/protocol/

[3] Berty Technologies. (2024). *Challenges in Building a Distributed Messaging System*. https://berty.tech/challenges

[4] Briar Project. (2024). *How Briar Works*. https://briarproject.org/how-it-works/

[5] Tarr, D., Lavoie, C., Meyer, A., & Kermarrec, A.-M. (2019). *Secure Scuttlebutt: An Identity-Centric Protocol for Subjective and Decentralized Applications*. Proceedings of ACM ICN '19.

[6] Meshtastic Project. (2025). *Known Limitations and Future Plans of Meshtastic's Encryption*. https://meshtastic.org/docs/about/overview/encryption/limitations/

[7] NIST. (2023). *Lightweight Cryptography Project*. https://csrc.nist.gov/Projects/lightweight-cryptography

[8] NIST. (2025). *NIST Special Publication 800-232: Ascon-Based Lightweight Cryptography Standards for Constrained Devices*.

[9] Dobraunig, C., Eichlseder, M., Mendel, F., & Schläffer, M. (2021). *Ascon v1.2: Lightweight Authenticated Encryption and Hashing*. IACR ePrint 2021/1574.

[10] Saha, D., et al. (2022). *Birthday-Bound Slide Attacks on TinyJAMBU's Keyed-Permutations*. Proceedings of ASIACRYPT 2022.

[11] Banik, S., Pandey, S. K., Peyrin, T., Sasaki, Y., Sim, S. M., & Todo, Y. (2017). *GIFT: A Small PRESENT — Towards Reaching the Limit of Lightweight Encryption*. IACR ePrint 2017/622.

[12] Khairallah, M. (2021). *Security of COFB against Chosen Ciphertext Attacks*. IACR ePrint 2021/648.

[13] Bogdanov, A., et al. (2007). *PRESENT: An Ultra-Lightweight Block Cipher*. Proceedings of CHES 2007.

[14] Bhargavan, K., & Leurent, G. (2016). *On the Practical (In-)Security of 64-bit Block Ciphers: Collision Attacks on HTTP over TLS and OpenVPN (Sweet32)*. Proceedings of ACM CCS 2016.

[15] Banik, S., et al. (2023). *NIST IR 8454: Status Report on the Final Round of the NIST Lightweight Cryptography Standardization Process*. NIST.

---

*— Akhir Dokumen Draft Proposal Tugas Akhir CARAKA Desktop —*
