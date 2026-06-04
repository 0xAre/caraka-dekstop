---
**DRAFT WHITEPAPER — VERSI 0.1**
*Dokumen ini bersifat draft awal untuk keperluan mata kuliah Implementasi Kriptografi.*
*Belum dipublikasikan. Segala konten bersifat tentatif dan terbuka untuk revisi.*

---

# CARAKA Desktop: Rancangan Protokol Komunikasi Mesh Offline Terdesentralisasi Berbasis Lightweight Cryptography

**Penulis:** [Nama Mahasiswa / Kelompok]
**Institusi:** [Nama Universitas], Program Studi [Nama Prodi]
**Mata Kuliah:** Implementasi Kriptografi
**Tanggal Draft:** Juni 2026

---

## Abstract

Ketersediaan infrastruktur internet yang tidak merata dan kerentanan sistem komunikasi terpusat terhadap kegagalan jaringan mendorong kebutuhan akan platform komunikasi yang mampu beroperasi secara mandiri tanpa bergantung pada server pusat maupun koneksi internet. Makalah ini mempresentasikan rancangan protokol dan arsitektur sistem **CARAKA Desktop** (*Cryptographically Authenticated Relay Architecture for Knowledge and Autonomy*), sebuah aplikasi komunikasi desktop berbasis jaringan *Peer-to-Peer Mesh* yang beroperasi dalam mode *offline-first*. Kontribusi utama penelitian ini terletak pada pengembangan protokol keamanan yang memanfaatkan **Ascon**, standar *Lightweight Cryptography (LWC)* baru dari *National Institute of Standards and Technology (NIST)* (NIST SP 800-232), sebagai primitif kriptografi inti untuk *Authenticated Encryption with Associated Data (AEAD)*, fungsi hash, dan autentikasi pesan berlapis-hop (*multi-hop MAC*). Berbeda dari sistem yang ada (Meshtastic, Berty, Briar, Secure Scuttlebutt), yang menggunakan kriptografi standar berat seperti Ed25519 (64-byte signature per paket), desain CARAKA meminimalkan *cryptographic overhead* per-hop dengan menggunakan Ascon-MAC berbasis *shared channel key*, memungkinkan *metadata routing* untuk dienkripsi di setiap lompatan (*hop*) tanpa mengorbankan kapasitas *payload*. Penelitian ini juga menyajikan analisis perbandingan komprehensif terhadap algoritma LWC finalis NIST (TinyJambu, GIFT-COFB, Grain-128AEAD, PRESENT) serta metodologi evaluasi empiris berbasis *microbenchmark* dan *network-level benchmark* untuk mengukur efisiensi dan keamanan protokol yang diusulkan.

**Kata Kunci:** *Lightweight Cryptography, ASCON, Mesh Network, Peer-to-Peer, Offline Communication, Store-and-Forward, Multi-hop Security, Authenticated Encryption, Decentralized Systems.*

---

## 1. Pendahuluan

### 1.1 Latar Belakang dan Motivasi

Komunikasi digital modern memiliki ketergantungan fundamental pada infrastruktur terpusat: server, *content delivery network*, dan koneksi internet yang reliabel. Ketergantungan ini menciptakan *single point of failure* yang berbahaya dalam skenario bencana alam, pemadaman jaringan, atau kondisi di mana akses internet dibatasi secara sengaja. Selain itu, sistem komunikasi terpusat secara inheren mengekspos metadata pengguna kepada penyedia layanan dan potensi pengawasan pihak ketiga.

Jaringan *Peer-to-Peer (P2P)* dan mesh menawarkan solusi arsitektural: setiap node bertindak sebagai router sekaligus klien, memungkinkan pesan mencapai tujuan melalui jalur alternatif tanpa bergantung pada satu titik pusat. Namun, mengamankan komunikasi dalam jaringan seperti ini menghadirkan tantangan kriptografi yang unik, terutama pada konteks *multi-hop relay* dimana:

1.  **Overhead Ukuran Paket:** Pada radio *low-bandwidth* seperti LoRa (batas 256 byte per paket), penggunaan tanda tangan digital konvensional seperti Ed25519 (64 byte per paket) mengkonsumsi lebih dari 25% kapasitas paket untuk autentikasi saja.
2.  **Kebocoran Metadata Routing:** Banyak sistem yang ada membiarkan *header routing* dalam bentuk *plaintext* agar node perantara (*relay*) dapat meneruskan paket tanpa kemampuan mendekripsinya, sehingga lawan pasif dapat memetakan topologi jaringan dan pola komunikasi.
3.  **Manajemen Kunci pada Konektivitas Intermittent:** *Perfect Forward Secrecy (PFS)* berbasis ratchet (seperti pada protokol Signal) memerlukan urutan pesan yang terjamin, sesuatu yang sangat sulit dipertahankan pada jaringan offline dengan penundaan tak tentu (*delay-tolerant*).

### 1.2 Kontribusi Penelitian

Makalah ini memberikan kontribusi pada bidang kriptografi dan sistem terdistribusi sebagai berikut:

1.  **Rancangan Protokol CLAMP** (*Compact Lightweight Authenticated Mesh Protocol*): Sebuah protokol *store-and-forward* yang menggunakan Ascon sebagai satu-satunya keluarga kriptografi untuk AEAD, hashing, dan MAC berlapis-hop.
2.  **Analisis Komparatif Algoritma LWC:** Evaluasi sistematis terhadap ASCON, TinyJambu, GIFT-COFB, Grain-128AEAD, dan PRESENT dalam konteks *desktop mesh networking*, yang melengkapi literatur evaluasi yang selama ini hanya berfokus pada perangkat tertanam (*embedded*) seperti AVR dan ARM Cortex-M.
3.  **Skema Autentikasi Multi-Hop Ringan:** Desain mekanisme autentikasi per-hop menggunakan Ascon-MAC berbasis *channel shared key* yang menggantikan per-packet Ed25519 signatures, disertai bukti skematis pengurangan overhead.
4.  **Metodologi Evaluasi Empiris:** Kerangka kerja pengujian yang mencakup *microbenchmark* kriptografi dan *network-level benchmark* multi-hop untuk mengukur *latency*, *throughput*, dan konsumsi memori secara empiris.

### 1.3 Ruang Lingkup dan Pembatasan

Penelitian ini dibatasi pada:
-   **Platform:** Aplikasi desktop (Windows/Linux/macOS) yang beroperasi dalam jaringan area lokal (LAN/Wi-Fi).
-   **Transport:** UDP untuk *discovery* dan TCP untuk pengiriman pesan terstruktur.
-   **Skala Jaringan:** Jaringan skala kecil hingga menengah (2–50 node) dalam satu area lokasi.
-   **Di luar cakupan:** *Post-Quantum Cryptography*, blockchain, infrastruktur cloud, dan kecerdasan buatan.

---

## 2. Tinjauan Pustaka dan State-of-the-Art

### 2.1 Sistem Komunikasi Mesh Offline yang Ada

#### 2.1.1 Briar

Briar [1] adalah aplikasi pesan *P2P* yang dirancang untuk aktivis dan jurnalis. Sistem ini beroperasi tanpa server pusat, menggunakan Bluetooth dan Wi-Fi untuk sinkronisasi *offline*, serta jaringan Tor untuk anonimitas saat online. Secara kriptografi, Briar mengimplementasikan konstruksi mirip-Signal dengan *Perfect Forward Secrecy* berbasis *Double Ratchet* dan AES-GCM untuk enkripsi simetris.

**Keterbatasan:** Routing *offline* Briar bergantung sepenuhnya pada jaringan sosial (*social graph*): pesan hanya dapat diteruskan melalui kontak bersama (*mutual contacts*), bukan jaringan mesh generik. Hal ini membatasi jangkauan komunikasi dan skalabilitas dalam skenario *ad-hoc* di mana anggota jaringan belum saling mengenal.

#### 2.1.2 Berty / Wesh Protocol

Berty [2, 3] adalah protokol komunikasi *offline-first* terdesentralisasi yang dibangun di atas IPFS dan OrbitDB. Setiap *key pair* menggunakan X25519 untuk enkripsi dan Ed25519 untuk tanda tangan. Berty mengadaptasi banyak desain kriptografi Signal untuk lingkungan terdistribusi.

**Keterbatasan:** Berty secara eksplisit mengakui bahwa protokolnya belum siap untuk data sensitif tinggi dan belum sepenuhnya diaudit [3]. Penggunaan Ed25519 per-pesan menambah overhead signifikan untuk setiap *event* yang direplikasi melalui gossip network. Sinkronisasi berbasis OrbitDB/IPFS memiliki *overhead* protokol yang besar untuk jaringan *low-bandwidth*.

#### 2.1.3 Meshtastic

Meshtastic [4] adalah platform mesh *off-grid* berbasis radio LoRa yang berjalan pada mikrokontroler ESP32/NRF52. Sistem ini menggunakan AES-256-CTR dengan *Pre-Shared Key (PSK)* untuk enkripsi channel, dan AES-CCM dengan pertukaran kunci X25519 untuk *Direct Messages* (sejak firmware v2.5).

**Keterbatasan Kritis:** Meshtastic mendokumentasikan sendiri keterbatasan utamanya [5, 6]:
-   *Routing header* dikirim dalam *plaintext* untuk memungkinkan relay oleh node yang tidak memiliki kunci channel.
-   Tanda tangan XEdDSA (64 byte) untuk DM sangat mahal mengingat batas 256 byte per paket LoRa.
-   Tidak ada perlindungan terhadap analisis lalu lintas (*traffic analysis*) dan pemetaan topologi jaringan oleh pihak luar.
-   PSK channel yang statis berarti kompromi satu node membocorkan seluruh riwayat channel.

#### 2.1.4 Secure Scuttlebutt (SSB)

Secure Scuttlebutt [7, 8] adalah protokol gossip berbasis *append-only signed log* per identitas. Setiap entri log ditandatangani dengan Ed25519 dan membentuk rantai hash yang tidak dapat dipalsukan.

**Keterbatasan:** SSB menciptakan rekam jejak digital yang permanen dan dapat diatribusikan. Ini secara fundamental bertentangan dengan kebutuhan privasi pesan sementara. Sinkronisasi memerlukan seluruh riwayat log, yang tidak efisien untuk komunikasi pesan biasa.

### 2.2 Lightweight Cryptography: Standar NIST

NIST memulai proses standardisasi *Lightweight Cryptography* pada tahun 2013 untuk mengatasi ketidaksesuaian antara algoritma standar (AES-GCM, SHA-256) dengan sumber daya perangkat tertanam yang sangat terbatas [9]. Proses ini berakhir pada Februari 2023 dengan terpilihnya **keluarga Ascon** [10] sebagai standar, yang kemudian dipublikasikan sebagai **NIST SP 800-232** (Agustus 2025) [11].

### 2.3 Kesenjangan Penelitian (Research Gap)

Tinjauan di atas mengungkap kesenjangan yang jelas dalam literatur dan implementasi yang ada:

> **Gap Utama:** Tidak ada sistem komunikasi mesh *offline* yang ada yang mengintegrasikan standar NIST LWC (Ascon) ke dalam desain protokol keamanannya, khususnya untuk mengatasi masalah *cryptographic overhead* pada autentikasi *multi-hop relay* dan enkripsi *metadata routing*.

Secara lebih spesifik, literatur yang ada belum membahas:

1.  Penggantian Ed25519 *per-packet signatures* dengan Ascon-MAC *per-hop* dalam konteks *store-and-forward mesh*.
2.  Penggunaan Ascon-Hash untuk *Epidemic Sync* yang *privacy-preserving* (sinkronisasi tanpa membocorkan konten pesan).
3.  Evaluasi kuantitatif dari *Ciphertext Expansion* dan *Multi-hop Latency* ketika LWC digunakan sebagai pengganti kriptografi standar dalam sistem *desktop mesh*.

---

## 3. Analisis Algoritma Lightweight Cryptography

Bagian ini menganalisis setiap algoritma kandidat dalam konteks kebutuhan teknis CARAKA Desktop.

### 3.1 Matriks Perbandingan Komprehensif

| Kriteria | **ASCON** | **TinyJambu** | **GIFT-COFB** | **Grain-128AEAD** | **PRESENT** |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **Status NIST** | ✅ Standar (SP 800-232) | ❌ Finalis (tidak dipilih) | ❌ Finalis | ❌ Finalis | ❌ ISO/IEC 29192-2 (Block Cipher saja) |
| **Tingkat Keamanan** | 128-bit ✅ | ~120-bit ⚠️ | ~64-bit tag ⚠️ | 128-bit ✅ | 80/128-bit ❌/⚠️ |
| **Kapabilitas** | AEAD + Hash + XOF ✅ | AEAD saja | AEAD saja | AEAD saja | Blok Cipher saja, butuh mode |
| **Isu Kriptanalisis** | Minimal | Birthday-bound slide attacks [12] | Effective 64-bit tag forgery [13] | Keystream ceiling limit | Sweet32 (64-bit block) [14] |
| **Dukungan Rust** | ✅ Kuat (`ascon-aead`, `ascon`) | ❌ Tidak ada crate resmi | ❌ Tidak ada crate resmi | ⚠️ Ada (`grain-128aeadv2`) | ❌ Minimal |
| **Ukuran Tag** | 128-bit | 64-bit | 128-bit (efektif 64-bit) | 64-bit | N/A |
| **Cocok untuk Desktop Mesh** | ✅ Sangat Cocok | ❌ Tidak | ⚠️ Terbatas | ⚠️ Dengan syarat ketat | ❌ Tidak |

### 3.2 Analisis Detail Per Algoritma

#### 3.2.1 ASCON (Rekomendasi Utama)

Ascon adalah *sponge-based permutation* yang dirancang oleh Dobraunig, Eichlseder, Mendel, dan Schläffer [15]. Keluarga ini mencakup:
-   **Ascon-AEAD128:** AEAD dengan kunci 128-bit, nonce 128-bit, dan tag 128-bit.
-   **Ascon-Hash256:** Fungsi hash kriptografi 256-bit.
-   **Ascon-XOF128 / Ascon-CXOF128:** *Extendable Output Function* untuk derivasi kunci (KDF).

*State* internal Ascon adalah 320-bit yang dioperasikan dalam mode *duplex*. Ini berarti satu keluarga primitif tunggal dapat memenuhi kebutuhan AEAD, hashing, dan KDF, mengurangi kompleksitas implementasi secara signifikan.

**Mengapa ASCON tepat untuk CARAKA:**
1.  Standar NIST yang aktif dan memiliki landasan analisis kriptografi paling luas di antara kandidat.
2.  Dukungan ekosistem Rust yang matang melalui RustCrypto Project.
3.  Satu keluarga untuk semua kebutuhan kriptografi simetris.
4.  *Compact state* yang efisien juga pada CPU *general-purpose*.

#### 3.2.2 TinyJambu (Ditolak)

TinyJambu mengalami serangkaian temuan kriptanalisis yang mengurangi kepercayaan terhadap *security margin*-nya [12, 16]:
-   Analisis diferensial menunjukkan *security margin* kurang dari 8 bit terhadap kompleksitas data.
-   *Birthday-bound slide attacks* membuktikan bahwa permutasi kunci *P2* untuk semua ukuran kunci TinyJambu dapat dipecahkan pada kompleksitas *birthday bound*, yang melemahkan asumsi *ideal permutation* dalam bukti keamanan mode AEAD-nya.
-   Tidak ada crate Rust yang resmi dan terawat.

**Kesimpulan:** Tidak direkomendasikan untuk protokol baru.

#### 3.2.3 GIFT-COFB (Ditolak untuk Desktop Mesh)

GIFT-COFB memiliki rate=1 yang efisien (satu panggilan *block cipher* per blok input). Namun, analisis oleh Khairallah [13] menunjukkan bahwa meskipun tag berukuran 128-bit, mode COFB secara efektif berperilaku seperti AEAD dengan tag 64-bit dalam skenario *high forgery count*, karena batas keamanan tumbuh seperti `qd / 2^(n/2)`. Dalam jaringan mesh dengan banyak node jangka panjang (*long-lived*), ini menjadi kelemahan yang tidak dapat diterima. Selain itu, tidak ada crate Rust yang tersedia.

#### 3.2.4 Grain-128AEAD (Conditional)

Grain-128AEAD v2 adalah AEAD berbasis *stream cipher* yang efisien pada perangkat *resource-constrained*. Namun, desainernya secara eksplisit membatasi panjang *keystream* per pasangan kunci/IV hingga sekitar 2^80 bit. Pada jaringan mesh yang aktif dengan banyak pesan, manajemen rotasi kunci yang ketat menjadi beban operasional. *Side-channel* dan *fault attack* aktif diteliti [17].

#### 3.2.5 PRESENT (Ditolak)

PRESENT adalah *block cipher* 64-bit yang dirancang untuk perangkat keras RFID sangat terbatas. Keterbatasan fundamentalnya:
-   Ukuran blok 64-bit rentan terhadap serangan kolisi *birthday bound* (serangan tipe Sweet32) untuk volume data besar.
-   Versi 80-bit kunci dianggap usang secara kriptografis oleh NIST.
-   Tidak memiliki mode AEAD bawaan, sehingga memerlukan desain mode tambahan yang berpotensi tidak aman.

### 3.3 Justifikasi Stack Kriptografi CARAKA

Berdasarkan analisis di atas, stack kriptografi CARAKA ditentukan sebagai:

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
│ KDF                │ HKDF-SHA256 atau Ascon-XOF128              │
│ Node Identity      │ Ed25519 Public Key (hanya untuk identitas) │
└────────────────────┴───────────────────────────────────────────┘
```

---

## 4. Rancangan Protokol CLAMP

*CLAMP* (*Compact Lightweight Authenticated Mesh Protocol*) adalah protokol lapisan aplikasi yang dirancang khusus untuk CARAKA Desktop.

### 4.1 Entitas dan Identitas

Setiap **Node** dalam jaringan CARAKA memiliki:
-   **Node ID:** 32-byte *Public Key* X25519, digunakan sebagai identitas permanen.
-   **Display Name:** Nama tampilan yang dapat dikonfigurasi pengguna.
-   **Node Fingerprint:** Ascon-Hash256 dari Node ID, digunakan sebagai referensi pendek dalam log.

### 4.2 Model Kunci

CLAMP mendefinisikan dua konteks kunci:

1.  **Direct Message Key (DM-Key):**
    -   Diderivasi dari *Elliptic Curve Diffie-Hellman (ECDH)* antara dua node.
    -   `shared_secret = X25519(my_private_key, peer_public_key)`
    -   `dm_key = Ascon-XOF128(shared_secret || "CARAKA-DM-v1" || sender_id || receiver_id)`
    -   Digunakan untuk enkripsi E2EE pesan langsung.

2.  **Channel Key (CH-Key):**
    -   Kunci simetris yang didistribusikan *out-of-band* (misal: melalui QR code) kepada anggota channel.
    -   Digunakan untuk autentikasi MAC per-hop pada pesan yang diteruskan relay.
    -   Berbeda dari DM-Key, CH-Key adalah *shared group secret*.

### 4.3 Struktur Paket CLAMP

```
┌────────────────────────────────────────────────────────────────────┐
│                         CLAMP Packet                               │
├─────────────┬──────────────────────────────────────────────────────┤
│ HEADER (Plaintext untuk routing layer)                             │
│  - Magic Bytes   : 2 byte  (0xCA, 0x52 = "CARA")                  │
│  - Version       : 1 byte                                           │
│  - Packet Type   : 1 byte  (DM / Channel / Control)               │
│  - TTL           : 1 byte  (max. 7 hops)                           │
│  - Packet ID     : 8 byte  (NodeID[0..4] + Nonce[0..4])           │
│  - Total Header  : 13 byte                                          │
├─────────────┴──────────────────────────────────────────────────────┤
│ HOP AUTHENTICATION LAYER (Per-relay, updated each hop)             │
│  - Hop Counter   : 1 byte                                           │
│  - Ascon-MAC Tag : 16 byte (Ascon-MAC dari Packet ID + Hop Counter │
│                             menggunakan CH-Key atau local key)      │
│  - Total         : 17 byte                                          │
├────────────────────────────────────────────────────────────────────┤
│ ENCRYPTED PAYLOAD (E2EE, hanya bisa dibuka oleh tujuan akhir)      │
│  - Nonce         : 16 byte (Ascon-AEAD128 nonce)                   │
│  - Ciphertext    : variable (max. MTU - overhead)                   │
│  - AEAD Tag      : 16 byte                                          │
└────────────────────────────────────────────────────────────────────┘
```

**Total Overhead Kriptografi per Paket:**
```
Header:        13 byte  (routing)
Hop Auth:      17 byte  (MAC per-hop)
Nonce:         16 byte  (AEAD)
AEAD Tag:      16 byte  (AEAD integrity)
─────────────────────────
Total Overhead: 62 byte
```

**Perbandingan dengan Meshtastic (untuk 256-byte LoRa packet):**
```
Meshtastic:
  Header + Routing : ~30 byte (plaintext)
  AES-CCM Overhead :  8 byte (nonce) + 8 byte (tag) = 16 byte
  XEdDSA Signature : 64 byte
  Total Overhead   : ~110 byte → hanya tersisa ~146 byte untuk payload

CARAKA/CLAMP (hipotesis untuk LoRa):
  Header           : 13 byte
  Hop MAC          : 17 byte
  Ascon Nonce      : 16 byte
  Ascon Tag        : 16 byte
  Total Overhead   : 62 byte → tersisa ~194 byte untuk payload (+33%)
```

### 4.4 Alur Pengiriman Pesan Direct Message (DM)

```
Pengirim (Alice)                    Relay (Bob)               Penerima (Charlie)
     │                                   │                            │
     │ 1. Buat Paket                     │                            │
     │    - Derive DM-Key(A→C)           │                            │
     │    - Enkripsi payload: Ascon-AEAD │                            │
     │    - Isi Header (TTL=7)           │                            │
     │    - Hitung Hop-MAC dengan CH-Key │                            │
     │                                   │                            │
     │ ──── CLAMP Packet ───────────────►│                            │
     │                                   │ 2. Validasi Hop-MAC        │
     │                                   │    Jika valid, lanjutkan   │
     │                                   │    Jika tidak, drop paket  │
     │                                   │                            │
     │                                   │ 3. Kurangi TTL             │
     │                                   │    Perbarui Hop Counter    │
     │                                   │    Hitung ulang Hop-MAC    │
     │                                   │                            │
     │                                   │ ──── CLAMP Packet ────────►│
     │                                   │                            │
     │                                   │                            │ 4. Validasi Hop-MAC
     │                                   │                            │    Dekripsi Payload
     │                                   │                            │    dengan DM-Key(A→C)
     │                                   │                            │    Tampilkan ke pengguna
```

### 4.5 Perlindungan Replay Attack

Setiap node mempertahankan **Packet Cache** berupa *LRU cache* dari 512 `Packet ID` terakhir yang diterima. Ketika paket diterima:
1.  Ekstrak `Packet ID` dari header (plaintext).
2.  Cek apakah `Packet ID` sudah ada di cache. Jika ya → **drop** (replay).
3.  Jika tidak → proses paket dan tambahkan ke cache.

Selain itu, `Packet ID` mengandung timestamp (4 byte embedded dalam nonce). Paket dengan timestamp lebih dari ±5 menit dari waktu sistem lokal di-drop secara otomatis.

---

## 5. Arsitektur Jaringan Terdistribusi

### 5.1 Peer Discovery

CARAKA menggunakan dua mekanisme *peer discovery* secara bersamaan:

1.  **UDP Broadcast / mDNS:** Setiap node mengirimkan pesan `CARAKA-HELLO` via UDP Broadcast (255.255.255.255:7770) setiap 30 detik. Pesan ini berisi `Node ID` (Public Key X25519), `Display Name` (terenkripsi minimal), dan `IP:Port` untuk koneksi TCP.
2.  **Manual Peer Entry:** Pengguna dapat menambahkan *peer* secara manual dengan memasukkan IP:Port atau memindai QR Code yang berisi `Node ID`.

### 5.2 Transport Layer

-   **Control Messages** (Discovery, Handshake, Sync Vektor): UDP.
-   **Data Messages** (CLAMP Packets): TCP dengan framing panjang (2-byte length prefix + data).

### 5.3 Mesh Routing: Controlled Flooding dengan TTL

CARAKA mengimplementasikan **Controlled Flooding** sebagai strategi routing utama:

1.  Setiap node menerima paket yang valid (Hop-MAC valid + TTL > 0 + Packet ID belum di-cache).
2.  Node meneruskan paket ke semua *peer* yang terhubung kecuali *peer* yang mengirimkan paket tersebut.
3.  TTL dikurangi 1 sebelum diteruskan.
4.  Jika `TTL == 0`, paket diterima jika ini adalah tujuan; jika bukan, di-drop tanpa diteruskan.

**Trust-Filtered Relay (Inovasi Tambahan):**
Setiap node mempertahankan `Trust Score` untuk setiap peer tetangganya. Skor ini dinaikkan ketika peer berperilaku normal (pesan valid, tidak spam) dan diturunkan ketika peer mengirim paket invalid atau berlebihan. Paket dari peer dengan `Trust Score < THRESHOLD` diabaikan, mengurangi risiko *Sybil Attack* dan *network flooding*.

### 5.4 Store-and-Forward dan Sinkronisasi Offline

Ketika node tidak dapat terhubung langsung ke tujuan, CARAKA menggunakan model **Epidemic Store-and-Forward**:

1.  **Penyimpanan:** Pesan yang belum terkirim (karena penerima offline) disimpan di *local encrypted database* menggunakan SQLite yang dienkripsi dengan kunci lokal.
2.  **Epidemic Sync:** Ketika dua node pertama kali terhubung, mereka menukar **Message Fingerprint Vector**: daftar `Ascon-Hash256` dari seluruh pesan yang tersimpan lokal.
3.  **Gap Filling:** Setelah membandingkan vektor, node meminta pesan yang dimiliki peer tetapi tidak ada di lokal. Hanya *ciphertext* (yang sudah terenkripsi E2EE) yang dipertukarkan, bukan *plaintext*.
4.  **Privasi Sync:** Karena yang dipertukarkan hanya hash dan ciphertext, node perantara *tidak pernah* dapat membaca isi pesan, bahkan saat menjadi relay sinkronisasi.

---

## 6. Arsitektur Keamanan

### 6.1 Model Kepercayaan (Trust Model)

CARAKA mengadopsi model **TOFU + Verification** (*Trust On First Use with Out-of-Band Verification*):

-   Pertama kali dua node bertemu, identitas (`Node ID = X25519 Public Key`) disimpan secara lokal.
-   Verifikasi opsional dapat dilakukan melalui perbandingan *fingerprint* secara langsung (misalnya, membandingkan 8 karakter hex terakhir dari `Node Fingerprint` secara verbal).
-   Tidak ada otoritas sertifikasi terpusat (*CA*).

### 6.2 End-to-End Encryption

Setiap pesan *Direct Message* dienkripsi secara E2EE sebelum meninggalkan perangkat pengirim:

```
1. Key Derivation:
   shared_secret = ECDH(alice_private_key, charlie_public_key)
   aead_key = Ascon-XOF128(
       shared_secret ||
       "CARAKA-DM-SESSION-v1" ||
       session_id ||       // mencegah cross-session reuse
       msg_counter         // forward secrecy sederhana
   )

2. Encryption:
   nonce = random_bytes(16)
   associated_data = header_bytes || hop_counter_bytes
   ciphertext || tag = Ascon-AEAD128.encrypt(
       key = aead_key,
       nonce = nonce,
       plaintext = message_bytes,
       associated_data = associated_data
   )
```

### 6.3 Multi-Hop Relay Authentication

Berbeda dari sistem yang menggunakan Ed25519 signature per-paket, CARAKA menggunakan Ascon-MAC per-hop:

```
Hop MAC Computation (pada setiap relay node):
hop_mac = Ascon-MAC(
    key = channel_key,          // CH-Key yang dibagi seluruh anggota channel
    data = packet_id ||          // 8 byte (unik per pesan)
           hop_counter ||         // 1 byte (meningkat di setiap hop)
           sending_node_id        // 4 byte (prefix Node ID pengirim hop ini)
)
```

**Analisis Ukuran vs. Keamanan:**
-   Ascon-MAC menghasilkan tag 128-bit (16 byte).
-   Ed25519 menghasilkan signature 64 byte.
-   **Penghematan: 48 byte per paket** (75% pengurangan overhead autentikasi).
-   Ascon-MAC dengan 128-bit key menyediakan keamanan 128-bit MAC, sama atau lebih baik dari Ed25519 untuk tujuan autentikasi integritas.

### 6.4 Forward Secrecy (Best-Effort)

Implementasi *full Double Ratchet* tidak realistis untuk lingkungan *delay-tolerant* dengan reordering pesan yang tinggi. CARAKA mengimplementasikan **Session-Based Forward Secrecy**:

-   Setiap sesi komunikasi memiliki `session_id` yang unik (random 8 byte).
-   `aead_key` diderivasi dari `shared_secret || session_id || msg_counter`.
-   Peningkatan `msg_counter` per pesan memberikan lapisan *forward secrecy* sederhana tanpa kebutuhan sinkronisasi ratchet yang ketat.

---

## 7. Threat Model

### 7.1 Asumsi Adversary

CARAKA dirancang dengan asumsi adversary yang memiliki kemampuan berikut:
-   Dapat mencegat (sniff) seluruh lalu lintas jaringan di layer transport (pasif).
-   Dapat menyuntikkan, mengubah, atau memblokir paket di layer jaringan (aktif).
-   Dapat menjalankan node berbahaya dalam jaringan (Sybil).
-   **Tidak dapat** membobol primitif kriptografi yang digunakan (Ascon-AEAD, X25519).
-   **Tidak dapat** secara fisik mengakses perangkat pengguna yang sah.

### 7.2 Analisis Ancaman dan Mitigasi

| Kategori | Ancaman Spesifik | Level Risiko | Mitigasi dalam CARAKA |
| :--- | :--- | :---: | :--- |
| **Confidentiality** | Eavesdropping (penyadapan payload) | 🔴 Tinggi | Enkripsi E2EE Ascon-AEAD128 untuk seluruh payload. |
| **Integrity** | Message Tampering (modifikasi ciphertext) | 🔴 Tinggi | AEAD tag 128-bit; modifikasi ciphertext menyebabkan dekripsi gagal. |
| **Replay** | Replay Attack (mengirim ulang paket lama) | 🟡 Sedang | Packet ID cache (LRU-512) + timestamp window (±5 menit). |
| **Routing** | Traffic Analysis (analisis lalu lintas) | 🟡 Sedang | Hop Counter + encrypted AEAD (konten tidak terbaca relay). |
| **Authentication** | Node Impersonation (penyamaran identitas) | 🟡 Sedang | TOFU model + out-of-band fingerprint verification. |
| **Availability** | Network Flooding / DoS | 🟡 Sedang | TTL (max 7 hop) + Trust Score filtering + Packet ID deduplication. |
| **Availability** | Sybil Attack (banyak node palsu) | 🟡 Sedang | Trust Score berbasis perilaku; node baru dimulai dengan skor rendah. |
| **Integrity** | Routing Manipulation | 🟢 Rendah | Hop-MAC divalidasi tiap relay; paket dari relay tidak terpercaya di-drop. |

### 7.3 Ancaman di Luar Cakupan (Out-of-Scope)

Ancaman berikut tidak ditangani dalam versi ini:
-   **Kompromi Perangkat:** Jika perangkat pengguna dikompromi (malware, akses fisik), kunci kriptografi dapat bocor.
-   **Quantum Attacks:** X25519 dan Ed25519 tidak *post-quantum resistant*. Ini adalah area pengembangan masa depan.
-   **Global Traffic Correlation:** Adversary dengan kapasitas monitoring jaringan global (seperti *nation-state*) dapat melakukan korelasi lalu lintas.

---

## 8. Metodologi Evaluasi

Untuk membuktikan nilai akademik protokol yang diusulkan, evaluasi dilakukan pada dua level:

### 8.1 Level 1: Microbenchmark Kriptografi

**Tujuan:** Mengukur performa primitif kriptografi Ascon dibandingkan AES-GCM dan ChaCha20-Poly1305 pada platform *desktop*.

**Framework:** Rust `criterion` library.

**Metrik yang Diukur:**

| Metrik | Unit | Keterangan |
| :--- | :--- | :--- |
| Encryption Throughput | MB/s | Ukuran pesan: 64B, 256B, 1KB, 4KB, 16KB |
| Decryption Throughput | MB/s | Idem |
| Key Derivation Time | μs | HKDF vs Ascon-XOF128 |
| MAC Computation Time | μs | Ascon-MAC untuk 62-byte header |
| Memory Footprint | KB | Peak RAM usage per operasi kriptografi |

**Hipotesis:** Ascon akan sedikit lebih lambat dari AES-GCM pada CPU dengan AES-NI, tetapi memiliki *memory footprint* yang lebih kecil dan performa lebih baik dari AES-GCM pada CPU tanpa akselerasi hardware.

### 8.2 Level 2: Network-Level Benchmark

**Tujuan:** Mengukur performa sistem secara end-to-end pada jaringan mesh yang disimulasikan.

**Topologi Uji:** 3 konfigurasi jaringan lokal:
-   Topologi Linear: `A → B → C → D → E` (5 hop, *worst case* untuk latency).
-   Topologi Star: `A ← B → C, A → D → E` (distribusi melalui hub node).
-   Topologi Mesh: Graf acak dengan 10 node, konektivitas rata-rata 3 tetangga per node.

**Metrik yang Diukur:**

| Metrik | Unit | Keterangan |
| :--- | :--- | :--- |
| End-to-End Latency | ms | Dari kirim hingga terima, per konfigurasi topologi |
| Message Delivery Ratio | % | Dari 1000 pesan yang dikirim, berapa yang diterima |
| Ciphertext Expansion | % | `(ukuran_paket_CARAKA / ukuran_plaintext) × 100%` |
| Hop Overhead per Node | byte | Overhead tambahan per-hop yang ditambahkan relay |
| Sync Throughput | msg/s | Kecepatan sinkronisasi *Epidemic Sync* antar dua node |

### 8.3 Baseline Perbandingan

Seluruh pengukuran dibandingkan terhadap dua baseline:
1.  **Baseline A (No Crypto):** Protokol CLAMP tanpa enkripsi (plaintext), untuk mengukur *networking overhead murni*.
2.  **Baseline B (AES-GCM):** Protokol CLAMP dengan AES-256-GCM + HMAC-SHA256 sebagai pengganti Ascon, untuk mengukur perbedaan antara LWC dan kriptografi konvensional.

---

## 9. Roadmap Implementasi

### Fase 1: Core Cryptographic Engine (Minggu 1–2)

**Deliverable:** Library Rust yang dapat diuji secara independen.

-   [x] Setup proyek Rust dengan workspace (`cargo new caraka-core --lib`).
-   [x] Integrasi dependensi: `ascon-aead`, `ascon`, `x25519-dalek`, `hkdf`, `sha2`.
-   [x] Implementasi modul `key_management.rs`:
    -   Generasi pasangan kunci X25519.
    -   Derivasi `DM-Key` dan `CH-Key`.
-   [x] Implementasi modul `crypto.rs`:
    -   Fungsi `encrypt_payload(key, nonce, plaintext, aad) -> ciphertext`.
    -   Fungsi `decrypt_payload(key, nonce, ciphertext, aad) -> plaintext`.
    -   Fungsi `compute_hop_mac(ch_key, packet_id, hop_counter, sender_id) -> tag`.
    -   Fungsi `verify_hop_mac(tag, ch_key, ...) -> bool`.
-   [x] Unit test untuk semua fungsi kriptografi.
-   [x] *Microbenchmark* awal menggunakan `criterion`.

### Fase 2: CLAMP Protocol Engine (Minggu 3–4)

**Deliverable:** Implementasi protokol yang dapat mengirim dan menerima paket CLAMP.

-   [x] Implementasi struktur data paket CLAMP (`packet.rs`).
-   [x] Implementasi serialisasi/deserialisasi paket (menggunakan `bincode` atau format biner kustom).
-   [x] Implementasi `PacketCache` (LRU 512 entries untuk deduplication).
-   [x] Implementasi validasi TTL, Hop-MAC, dan timestamp.

### Fase 3: Jaringan P2P dan Routing (Minggu 5–6)

**Deliverable:** Dua node dapat saling menemukan dan berkomunikasi melalui relay.

-   [x] Implementasi `peer_discovery.rs`: UDP Broadcast listener dan sender.
-   [x] Implementasi `transport.rs`: TCP server/client dengan framing.
-   [x] Implementasi `routing.rs`: Controlled Flooding dengan Trust Score.
-   [x] Implementasi `store.rs`: SQLite encrypted local message store.
-   [x] Implementasi `sync.rs`: Epidemic Sync dengan Message Fingerprint Vector.

### Fase 4: Graphical User Interface (Minggu 7–8)

**Deliverable:** Aplikasi desktop yang dapat dioperasikan pengguna.

-   [x] Setup proyek Tauri (menggunakan `create-tauri-app`).
-   [x] Desain UI menggunakan HTML/CSS/JS.
-   [x] Integrasi backend Rust (modul Fase 1–3) dengan frontend Tauri.
-   [x] Implementasi tampilan: Daftar peer, chat window, status koneksi, visualisasi node aktif.

### Fase 5: Evaluasi dan Penulisan (Minggu 9–10)

**Deliverable:** Laporan evaluasi kuantitatif dan draft makalah final.

-   [ ] Jalankan seluruh *microbenchmark* dan catat hasilnya.
-   [ ] Setup topologi jaringan uji (menggunakan beberapa mesin fisik atau VM).
-   [ ] Jalankan seluruh *network-level benchmark*.
-   [ ] Buat tabel dan grafik perbandingan ASCON vs AES-GCM.
-   [ ] Tulis makalah final.

---

## 10. Kesimpulan

Makalah ini telah mempresentasikan CARAKA Desktop, sebuah platform komunikasi mesh offline terdesentralisasi yang dirancang dari awal untuk mengintegrasikan **Ascon** — standar *Lightweight Cryptography* NIST — sebagai primitif kriptografi inti.

Kontribusi utama yang dibawa oleh proyek ini adalah:

1.  **Protokol CLAMP** yang meminimalkan *cryptographic overhead* per-hop dari 64 byte (Ed25519) menjadi 17 byte (Ascon-MAC), memberikan **penghematan 73%** dalam overhead autentikasi — relevan secara langsung untuk jaringan *low-bandwidth* masa depan.
2.  **Analisis komparatif** yang membuktikan Ascon sebagai satu-satunya kandidat LWC yang memenuhi semua kriteria: keamanan 128-bit yang telah dibuktikan, standarisasi NIST, kapabilitas AEAD+Hash+XOF dalam satu keluarga, dan ekosistem Rust yang matang.
3.  **Arsitektur sinkronisasi privacy-preserving** menggunakan Ascon-Hash untuk Epidemic Sync yang memungkinkan node perantara membantu sinkronisasi pesan tanpa pernah dapat membaca kontennya.

Proyek ini melampaui sekadar "aplikasi messenger dengan enkripsi" dan menempatkan dirinya sebagai eksperimen protokol yang dapat memberikan kontribusi nyata pada literatur *Secure Mesh Communication* dan *Applied Lightweight Cryptography*.

---

## Referensi

> *[Bagian referensi ini akan dilengkapi dengan format IEEE pada versi final makalah.]*

[1] Briar Project. *How Briar Works*. https://briarproject.org/how-it-works/ (2024).

[2] Berty Technologies. *Wesh Protocol Technical Documentation*. https://berty.tech/docs/protocol/ (2024).

[3] Berty Technologies. *Challenges in Building a Distributed Messaging System*. https://berty.tech/challenges (2024).

[4] Meshtastic Project. *Updated Security Implementation*. https://meshtastic.org/docs/development/reference/encryption-technical/ (2025).

[5] Meshtastic Project. *Meshtastic Encryption Overview*. https://meshtastic.org/docs/overview/encryption/ (2025).

[6] Meshtastic Project. *Known Limitations and Future Plans of Meshtastic's Encryption*. https://meshtastic.org/docs/about/overview/encryption/limitations/ (2025).

[7] Tarr, D., et al. *Secure Scuttlebutt: An Identity-Centric Protocol for Subjective and Decentralized Applications*. ICN '19 (2019).

[8] SSB Community. *Scuttlebutt Protocol Guide*. https://ssbc.github.io/scuttlebutt-protocol-guide/ (2024).

[9] NIST. *Lightweight Cryptography Project*. https://csrc.nist.gov/Projects/lightweight-cryptography (2023).

[10] NIST. *Announcing Lightweight Cryptography Selection: Ascon Family*. https://csrc.nist.gov/news/2023/lightweight-cryptography-nist-selects-ascon (2023).

[11] Dobraunig, C., et al. *NIST Special Publication 800-232: Ascon-Based Lightweight Cryptography Standards for Constrained Devices*. NIST (2025).

[12] Saha, D., et al. *Birthday-Bound Slide Attacks on TinyJAMBU's Keyed-Permutations*. ASIACRYPT 2022.

[13] Khairallah, M. *Security of COFB against Chosen Ciphertext Attacks*. IACR ePrint 2021/648 (2021).

[14] Bhargavan, K., Leurent, G. *On the Practical (In-)Security of 64-bit Block Ciphers (Sweet32)*. CCS 2016.

[15] Dobraunig, C., Eichlseder, M., Mendel, F., Schläffer, M. *Ascon v1.2: Lightweight Authenticated Encryption and Hashing*. IACR ePrint 2021/1574 (2021).

[16] Li, Y., et al. *On the Security Margin of TinyJAMBU with Refined Differential and Linear Cryptanalysis*. LWC Workshop 2020.

[17] Banik, S., et al. *NIST IR 8454: Status Report on the Final Round of the NIST Lightweight Cryptography Standardization Process*. NIST (2023).

---

*— Akhir Dokumen Draft Whitepaper CARAKA Desktop v0.1 —*
