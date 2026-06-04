# CARAKA Desktop: Research & Architecture Report

> **Secure Offline Mesh Communication Powered by Lightweight Cryptography**

## 1. Executive Summary

Proyek **CARAKA Desktop** bertujuan membangun sistem komunikasi desentralisasi tanpa internet yang mengimplementasikan kriptografi ringan (Lightweight Cryptography/LWC). Berdasarkan analisis state-of-the-art (Meshtastic, Berty, Briar, SSB) dan tinjauan standar NIST LWC terbaru, laporan ini merumuskan bahwa menggunakan kriptografi konvensional (AES-GCM / RSA / Ed25519) pada jaringan mesh menghasilkan *overhead* yang sangat tinggi pada level routing dan autentikasi multi-hop. 

Pendekatan terbaik untuk mendapatkan **nilai akademik dan novelty** yang tinggi adalah dengan mengintegrasikan **ASCON** (pemenang standar NIST LWC) sebagai core *Authenticated Encryption with Associated Data (AEAD)* dan *Hashing*, dipadukan dengan desain arsitektur routing yang *metadata-hiding*. Meskipun aplikasi ini berjalan di Desktop (yang secara komputasi kuat), penggunaan ASCON akan membuktikan skalabilitas protokol untuk *heterogeneous mesh networks* (dimana desktop bertindak sebagai *backbone* bagi *constrained devices* seperti IoT/LoRa di masa depan).

---

## 2. Research Gap Analysis

Sistem komunikasi mesh/offline yang ada saat ini menghadapi tantangan besar pada **Trade-off antara Keamanan vs Overhead Jaringan**.

*   **Mengapa gap ini penting:** Pada komunikasi *multi-hop*, setiap byte tambahan (seperti *header routing* atau *cryptographic signature*) mengurangi kapasitas *payload* secara drastis (contoh: LoRa dibatasi 256 bytes).
*   **Mengapa belum dieksplorasi:** Proyek seperti Meshtastic atau Berty menggunakan kriptografi standar (AES-CTR/CCM, X25519, Ed25519). Signature Ed25519 memakan 64 bytes per paket. Akibatnya, banyak sistem mesh memilih membiarkan *routing header* dalam bentuk *plaintext* (metadata terekspos) agar paket masih bisa diteruskan (relayed).
*   **Kontribusi Proyek:** CARAKA Desktop dapat mengisi gap ini dengan merancang protokol *secure store-and-forward* yang menggunakan primitives dari algoritma Lightweight Cryptography (ASCON-AEAD & ASCON-MAC). Hal ini akan mendemonstrasikan bagaimana LWC dapat meminimalkan ukuran *header* enkripsi per-hop dan memungkinkan *metadata-hiding routing* (seperti *onion routing* versi ringan).

---

## 3. State-of-the-Art Review

Analisis terhadap sistem yang ada saat ini:

| Sistem | Transport & Routing | Security & Crypto | Kelebihan | Kelemahan & Peluang Kontribusi |
| :--- | :--- | :--- | :--- | :--- |
| **Briar** | Tor (online), Bluetooth/Wi-Fi (offline). *Social graph routing*. | AES, Signal-like PFS, Tor Onion. | Privasi tinggi, metadata terlindungi di *social layer*. | *Path routing* terbatas pada kontak langsung. Sangat lambat jika tidak online. |
| **Berty** | IPFS (Wesh), BLE. *Gossip-protocol*. | X25519 & Ed25519 di seluruh stack. | *Offline-first*, desentralisasi penuh. | *Overhead* tinggi dari Ed25519 untuk setiap pesan. Tidak dioptimasi untuk *low-bandwidth*. |
| **Meshtastic** | LoRa PHY. *Flooding-mesh*. | AES-256-CTR (Channel), AES-CCM (DM). | Berjalan baik di radio *low-power*. | *Routing header plaintext*. Signature 64B terlalu besar. |
| **Scuttlebutt** | LAN / Wi-Fi Sync. *Epidemic Tree*. | Ed25519 (Append-only logs). | Tahan banting terhadap partisi jaringan. | Eksposur metadata yang sangat besar; jejak permanen. |

> [!TIP]
> **Research Opportunity:** Sistem saat ini terjebak antara "Privasi Metadata Penuh tetapi Lambat/Online" (Briar) atau "Cepat/Offline tetapi Metadata Terekspos" (Meshtastic/Berty). CARAKA bisa masuk di tengah: *Offline Mesh* dengan perlindungan metadata menggunakan *LWC primitives*.

---

## 4. Novelty Recommendation

Novelty yang paling kuat dan menjanjikan secara akademik untuk proyek CARAKA adalah:
**"LWC-Aware Secure Onion Routing for Offline Mesh Networks"**

Fokus pada kombinasi area berikut:
1.  **Multi-Hop Security:** Mengganti *heavy public-key signatures* (Ed25519) dengan *lightweight symmetric MACs* (ASCON-MAC) untuk verifikasi integritas pesan di tingkat *relay/hop*.
2.  **Cryptographic Performance Evaluation:** Membandingkan kinerja protokol mesh (kecepatan, overhead memori, dan rasio latensi) ketika di-*back* oleh ASCON (LWC) versus AES-GCM (Standard). 

*Pendekatan ini menjauhkan CARAKA dari sekadar "aplikasi chat" menjadi sebuah eksperimen protokol jaringan desentralisasi.*

---

## 5. Cryptography Research (Algoritma Review)

Berdasarkan literatur dan spesifikasi NIST:

1.  **ASCON (NIST Standard - Winner):**
    *   **Security:** ~128-bit security. Melewati analisis paling ketat dari kompetisi CAESAR & NIST LWC.
    *   **Suitability:** Sangat cocok karena mendukung AEAD dan XOF (Hash) dalam satu *primitive* kecil.
    *   **Rust Ecosystem:** Sangat kuat (`ascon-aead`, di-maintain oleh RustCrypto).
2.  **TinyJambu:**
    *   **Security:** Memiliki isu *security margin* yang tipis. Analisis terbaru menunjukkan kerentanan *birthday-bound slide attacks*.
    *   **Rust Ecosystem:** Lemah/Tidak ada crate resmi.
3.  **GIFT-COFB:**
    *   **Security:** AEAD rate=1, tetapi memiliki batas keamanan efektif 64-bit tag dalam skenario *high-forgery*, kurang ideal untuk jaringan dengan banyak partisipan yang *long-lived*.
4.  **Grain-128AEAD:**
    *   **Security:** Stream-cipher yang baik, tetapi butuh batasan *keystream length* yang ketat (maks ~2^80 bit) per IV. Rawan jika IV (nonce) di-reuse.
5.  **PRESENT:**
    *   **Security:** Block cipher 64-bit. Keamanan 80-bit usang (obsolete). Rentan terhadap serangan kolisi data (Sweet32).

> [!IMPORTANT]
> **Rekomendasi Utama:** Gunakan **ASCON**. Ini adalah standar industri masa depan untuk LWC. Untuk *Key Exchange*, gunakan **X25519** (Elliptic Curve Diffie-Hellman) karena LWC belum menstandardisasi skema asimetrik (*Public-Key*), dan X25519 adalah standar yang ringan dan aman.

---

## 6. Recommended Security Architecture

1.  **End-to-End Encryption (E2EE):**
    *   Setiap payload pesan dienkripsi secara E2EE menggunakan **ASCON-AEAD128**.
2.  **Key Exchange & Node Identity:**
    *   Identity node berupa *Public Key* dari **X25519**.
    *   Saat dua node ingin berkomunikasi *Direct Message*, mereka melakukan *Key Agreement* ECDH untuk mendapatkan *Shared Secret*, yang kemudian dimasukkan ke *Key Derivation Function (KDF)* seperti HKDF-SHA256 atau ASCON-XOF untuk menghasilkan *Symmetric Key* ASCON.
3.  **Multi-Hop / Relay Authentication (Inovasi):**
    *   Daripada setiap node me-*relay* pesan menandatangani (sign) keseluruhan paket dengan Ed25519, payload yang telah di E2EE dibungkus (wrapped) dengan ASCON-MAC per-hop menggunakan *Channel Key* / *Group Key* terdistribusi.
4.  **Replay Protection:**
    *   Gunakan kombinasi *Session ID* dan *Monotonic Counter / Timestamp*. Paket dengan *Counter* lama atau *Session ID* kadaluarsa akan di-drop oleh node *relay* (mencegah *Broadcast Storm Attack*).

---

## 7. Distributed Systems Architecture

1.  **Peer Discovery:** 
    *   Menggunakan UDP Broadcast / Multicast (mDNS) di jaringan LAN lokal.
2.  **Mesh Routing Model:** 
    *   Gunakan **Controlled Flooding** dengan *Time-To-Live (TTL)*.
    *   Sebagai tambahan novelty, setiap node menyimpan *Trust Score* dari *peer* tetangganya. Pesan hanya di-*relay* dari/ke *peer* dengan *Trust Score* yang valid (menangkal *Sybil / Spam*).
3.  **Store-and-Forward / Offline Synchronization:**
    *   Ketika partisi jaringan terputus (offline), pesan disimpan di *local database* terenkripsi.
    *   Saat terhubung dengan *peer* baru, lakukan **Epidemic Sync**: bertukar vektor *Message Hash* (menggunakan ASCON-Hash) untuk mensinkronisasi log pesan tanpa membocorkan konten.

---

## 8. Threat Model

| Ancaman (*Threat*) | Mitigasi |
| :--- | :--- |
| **Eavesdropping (Penyadapan)** | Payload dienkripsi End-to-End dengan ASCON-AEAD. |
| **Message Tampering** | Autentikasi tag 128-bit oleh ASCON mencegah modifikasi *ciphertext*. |
| **Replay Attack** | Implementasi *Nonce* unik (kombinasi Node ID + Counter + Timestamp) dan validasi di level *relay*. |
| **Routing / Metadata Analysis** | Mengenkripsi *Routing Header* secara per-hop dengan *Channel/Local Key*. |
| **Sybil Attack / Network Flooding** | Pembatasan *Rate-limit* di level aplikasi dan validasi *Trust Score* berbasis *Proof-of-Work* ringan atau persetujuan peer. |

---

## 9. Evaluation Methodology

Untuk memenuhi standar makalah implementasi kriptografi, evaluasi harus kuantitatif dan empiris:

1.  **Micro-benchmarks (Algoritma):**
    *   Ukur *Encryption/Decryption Time* (dalam milidetik/mikrodetik).
    *   Ukur *Throughput* (MB/s).
    *   Ukur *Memory Usage* (Peak RAM footprint) dari ASCON vs AES-GCM (menggunakan Rust `criterion`).
2.  **Macro-benchmarks (Sistem/Jaringan):**
    *   **Ciphertext Expansion:** Bandingkan ukuran total paket (Header + Payload + Tag) ASCON vs Protokol konvensional.
    *   **Multi-hop Latency:** Waktu yang dibutuhkan pesan melewati 1, 3, dan 5 *hop* jaringan terisolasi.
    *   **Network Overhead:** Bandwidth total yang digunakan untuk *Syncing* 1000 pesan (menguji efisiensi arsitektur *gossip*).

---

## 10. Technical Risks and Mitigation

| Risiko Teknis | Mitigasi yang Direkomendasikan |
| :--- | :--- |
| Implementasi ASCON tidak aman (Side-channel) | Jangan menulis implementasi kriptografi dari nol. Gunakan *crate* resmi `ascon-aead` yang telah direview oleh komunitas Rust. |
| *Broadcast Storm* menghancurkan koneksi LAN | Implementasi TTL (*Time-To-Live*) maksimal 5 hop, dan simpan *cache* Hash dari 500 pesan terakhir untuk mencegah me-*relay* pesan yang sama. |
| Kompleksitas sinkronisasi *offline* tinggi | Batasi fitur awal. Mulai dengan sinkronisasi 1-on-1 (*Direct Messaging*) sebelum mencoba *Group Messaging / Channel*. |

---

## 11. Development Roadmap (Untuk Mahasiswa)

*   **Phase 1: Core Cryptography (Minggu 1-2)**
    *   Setup proyek Rust (`cargo`). Implementasi modul Kriptografi menggunakan `ascon-aead` dan `x25519-dalek`. Pembuatan *unit test*.
*   **Phase 2: P2P Network Engine (Minggu 3-4)**
    *   Implementasi UDP Broadcast untuk *Peer Discovery*. Pembuatan sistem *TCP/UDP Socket* untuk transmisi *ciphertext*.
*   **Phase 3: Mesh & Routing Logic (Minggu 5-6)**
    *   Logika *Flooding*, pencegahan *Replay Attack*, dan implementasi lokal *database* (SQLite/Sled) untuk *Store-and-Forward*.
*   **Phase 4: GUI & Integration (Minggu 7-8)**
    *   Membangun UI Desktop (Tauri atau Iced) dan integrasi dengan *core backend*.
*   **Phase 5: Evaluation & Paper Writing (Minggu 9-10)**
    *   Menjalankan *benchmark* (Criterion), mencatat matrik, dan menyusun laporan akhir / makalah.

---

## 12. Final Recommendation

Untuk mendapatkan nilai tertinggi dari aspek *Engineering*, *Cryptography*, dan *Novelty*:

Bangunlah **CARAKA Desktop** menggunakan **Rust**. Gunakan **Tauri** (HTML/CSS/JS) untuk UI yang cantik dan modern, sementara seluruh *logic routing* dan kriptografi berada di backend Rust. 

Jadikan arsitekturnya difokuskan pada pembandingan: **"Bagaimana jika aplikasi komunikasi desktop menggunakan algoritma IoT (ASCON) untuk mengamankan jaringan mesh desentralisasi?"**
Anda akan mendapatkan aplikasi desktop yang berjalan sangat cepat, memiliki keamanan *state-of-the-art*, dan membuktikan bahwa arsitektur protokol ini siap (*future-proof*) untuk diekspansi ke perangkat IoT (seperti LoRa / ESP32) di fase riset selanjutnya.
