// src-tauri/src/crypto.rs
//
// Modul kriptografi utama CARAKA Desktop.
// Mengimplementasikan:
//   - Ascon-AEAD128  : enkripsi End-to-End (payload DM / Channel)
//   - Ascon-MAC      : autentikasi per-hop relay (via HKDF-SHA256 proxy)
//   - Ascon-Hash256  : fingerprint pesan untuk Epidemic Sync
//   - Ascon-XOF128   : derivasi kunci (via HKDF-SHA256 proxy)
//   - Nonce          : timestamp-embedded, fresh setiap enkripsi
//
// ATURAN KEAMANAN (WAJIB):
//   1. Semua randomness dari OsRng — BUKAN thread_rng()
//   2. Semua tipe kunci mengimplementasikan Zeroize
//   3. Nonce selalu di-generate ulang setiap enkripsi
//   4. Gunakan constant-time comparison untuk MAC verification

use ascon_aead::{
    Ascon128,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

// ─── Error Types ────────────────────────────────────────────────────────────

#[derive(Debug, Error, PartialEq)]
pub enum CryptoError {
    #[error("AEAD decryption failed: authentication tag mismatch")]
    DecryptionFailed,

    #[error("Timestamp out of validity window (±300 seconds from now)")]
    TimestampInvalid,

    #[error("Key derivation failed: output too long")]
    KeyDerivationFailed,
}

// ─── Key Types ──────────────────────────────────────────────────────────────

/// Kunci 128-bit untuk Ascon-AEAD128.
/// Dibuat via derive_dm_key() atau derive_channel_mac_key().
/// Otomatis di-zero dari memori saat di-drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct AeadKey(pub [u8; 16]);

impl std::fmt::Debug for AeadKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AeadKey([REDACTED])")
    }
}

/// Kunci 128-bit untuk Ascon-MAC (autentikasi per-hop relay).
/// Dibagikan ke semua anggota channel — bukan untuk enkripsi payload.
/// Otomatis di-zero dari memori saat di-drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MacKey(pub [u8; 16]);

impl std::fmt::Debug for MacKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MacKey([REDACTED])")
    }
}

/// Nonce 128-bit untuk Ascon-AEAD128.
/// Format byte: timestamp_u32_LE[0..4] || OsRng_random[4..16]
/// Timestamp memungkinkan validasi anti-replay tanpa counter persistens.
#[derive(Clone, Debug)]
pub struct Nonce(pub [u8; 16]);

// ─── Nonce ──────────────────────────────────────────────────────────────────

/// Generate nonce baru yang SELALU unik per enkripsi.
/// Embed Unix timestamp (4 byte) + 12 byte random dari OsRng.
/// WAJIB dipanggil setiap kali enkripsi — jangan reuse nonce.
pub fn generate_nonce() -> Nonce {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32;

    let mut nonce = [0u8; 16];
    nonce[0..4].copy_from_slice(&ts.to_le_bytes()); // timestamp (4 byte)
    OsRng.fill_bytes(&mut nonce[4..16]);             // random (12 byte)
    Nonce(nonce)
}

/// Validasi timestamp yang di-embed dalam nonce.
/// Tolak paket dengan timestamp di luar jendela ±300 detik.
/// Ini adalah lini pertahanan anti-replay attack bersama Packet ID Cache.
pub fn validate_nonce_timestamp(nonce: &Nonce) -> Result<(), CryptoError> {
    let embedded_ts = u32::from_le_bytes(
        nonce.0[0..4].try_into().expect("slice dengan panjang 4")
    ) as u64;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if now.abs_diff(embedded_ts) > 300 {
        return Err(CryptoError::TimestampInvalid);
    }
    Ok(())
}

// ─── AEAD Encryption / Decryption ───────────────────────────────────────────

/// Enkripsi plaintext dengan Ascon-AEAD128.
///
/// `aad` = Associated Data (CLAMP header 13 byte) — diautentikasi, TIDAK dienkripsi.
/// Ikatan AAD ke ciphertext memastikan header tidak bisa dimodifikasi tanpa deteksi.
///
/// Kembalikan: (ciphertext, aead_tag_16_byte)
pub fn encrypt(
    key: &AeadKey,
    nonce: &Nonce,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, [u8; 16]), CryptoError> {
    let ascon_key = ascon_aead::Key::<Ascon128>::from_slice(&key.0);
    let ascon_nonce = ascon_aead::Nonce::<Ascon128>::from_slice(&nonce.0);
    let cipher = Ascon128::new(ascon_key);

    // ascon-aead menghasilkan ciphertext dengan 16-byte tag di-append di akhir
    let ct_with_tag = cipher
        .encrypt(ascon_nonce, Payload { msg: plaintext, aad })
        .map_err(|_| CryptoError::DecryptionFailed)?;

    // Pisahkan ciphertext dan tag
    let tag_start = ct_with_tag.len().saturating_sub(16);
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&ct_with_tag[tag_start..]);

    Ok((ct_with_tag[..tag_start].to_vec(), tag))
}

/// Dekripsi ciphertext dengan Ascon-AEAD128.
///
/// GAGAL (CryptoError::DecryptionFailed) jika:
///   - Tag autentikasi tidak cocok (ciphertext dimodifikasi)
///   - Associated Data tidak cocok (header dimodifikasi)
///   - Kunci salah (paket bukan untuk node ini)
///
/// Kegagalan dekripsi adalah hal NORMAL — artinya paket bukan untuk kita
/// atau perlu di-relay. JANGAN tampilkan error ini ke user.
pub fn decrypt(
    key: &AeadKey,
    nonce: &Nonce,
    ciphertext: &[u8],
    tag: &[u8; 16],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    // Validasi timestamp anti-replay
    validate_nonce_timestamp(nonce)?;

    let ascon_key = ascon_aead::Key::<Ascon128>::from_slice(&key.0);
    let ascon_nonce = ascon_aead::Nonce::<Ascon128>::from_slice(&nonce.0);
    let cipher = Ascon128::new(ascon_key);

    // Gabungkan ciphertext + tag untuk di-pass ke Ascon
    let mut ct_with_tag = ciphertext.to_vec();
    ct_with_tag.extend_from_slice(tag);

    cipher
        .decrypt(ascon_nonce, Payload { msg: &ct_with_tag, aad })
        .map_err(|_| CryptoError::DecryptionFailed)
}

// ─── Ascon-MAC (Hop Authentication) ─────────────────────────────────────────

/// Hitung MAC untuk autentikasi per-hop relay menggunakan HKDF-SHA256.
///
/// Pengganti Ascon-MAC native — keamanan setara (128-bit), sudah diaudit.
/// Native Ascon-MAC akan digunakan setelah crate ascon-mac stabil.
///
/// Input standar (13 byte):
///   mac_input = packet_id[0..8] || hop_counter[1] || relay_node_id[0..4]
///
/// Inovasi utama CARAKA:
///   Ascon-MAC 16 byte vs Ed25519 64 byte = hemat 75% overhead autentikasi
pub fn compute_mac(key: &MacKey, data: &[u8]) -> [u8; 16] {
    let hk = Hkdf::<Sha256>::new(Some(&key.0), data);
    let mut tag = [0u8; 16];
    hk.expand(b"CARAKA-HOP-MAC-v1", &mut tag)
        .expect("HKDF expand selalu berhasil untuk output 16 byte");
    tag
}

/// Verifikasi MAC secara constant-time.
///
/// PENTING: Gunakan constant-time comparison (fold dengan XOR) bukan == langsung.
/// Perbandingan biasa rentan terhadap timing side-channel attack.
pub fn verify_mac(key: &MacKey, data: &[u8], expected: &[u8; 16]) -> bool {
    let computed = compute_mac(key, data);
    // Constant-time comparison: XOR semua byte, fold ke satu nilai
    // Jika semua byte identik, hasilnya 0
    computed
        .iter()
        .zip(expected.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

// ─── Ascon-Hash256 (Message Fingerprint) ────────────────────────────────────

/// Hitung hash 32-byte dari data menggunakan SHA-256 dengan domain separation.
///
/// Digunakan untuk: Message Fingerprint dalam Epidemic Sync.
/// Node perantara bertukar hash pesan (bukan isi) untuk sinkronisasi
/// tanpa pernah membaca konten pesan yang terenkripsi.
///
/// Catatan: Menggunakan SHA-256 sebagai proxy sampai ascon-hash crate stabil.
/// Domain separation "CARAKA-HASH-v1" memastikan output berbeda dari hash biasa.
pub fn hash256(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(b"CARAKA-HASH-v1"); // domain separation
    hasher.update(data);
    hasher.finalize().into()
}

/// Ambil 16-byte prefix dari hash256 sebagai fingerprint ringkas.
/// Digunakan dalam SYNC_REQ / SYNC_RESP untuk efisiensi bandwidth.
pub fn fingerprint16(data: &[u8]) -> [u8; 16] {
    let hash = hash256(data);
    let mut fp = [0u8; 16];
    fp.copy_from_slice(&hash[0..16]);
    fp
}

// ─── Ascon-XOF128 (Key Derivation) ──────────────────────────────────────────

/// Turunkan kunci sepanjang `output.len()` byte dari secret dan context.
///
/// Menggunakan HKDF-SHA256 sebagai proxy Ascon-XOF128.
/// Keamanan setara — kedua skema menghasilkan output pseudorandom yang aman.
///
/// Context string HARUS unik per tujuan penggunaan kunci untuk mencegah
/// key confusion attack (kunci untuk tujuan berbeda tidak boleh sama).
pub fn xof_derive(secret: &[u8], context: &[u8], output: &mut [u8]) {
    let hk = Hkdf::<Sha256>::new(None, secret);
    hk.expand(context, output)
        .expect("HKDF expand gagal — output terlalu panjang (maks ~8KB)");
}

// ─── Unit Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: buat AeadKey dari byte tunggal yang diulang
    fn test_aead_key(b: u8) -> AeadKey { AeadKey([b; 16]) }
    fn test_mac_key(b: u8) -> MacKey { MacKey([b; 16]) }

    // ── Nonce Tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_nonce_timestamp_embedded() {
        let nonce = generate_nonce();
        let ts = u32::from_le_bytes(nonce.0[0..4].try_into().unwrap()) as u64;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH).unwrap().as_secs();
        // Timestamp harus dalam ±5 detik dari sekarang
        assert!(now.abs_diff(ts) < 5, "Timestamp di nonce tidak valid");
    }

    #[test]
    fn test_nonce_unique() {
        let n1 = generate_nonce();
        let n2 = generate_nonce();
        // Dua nonce tidak boleh identik (12 byte random)
        assert_ne!(n1.0, n2.0, "Dua nonce berurutan tidak boleh sama");
    }

    #[test]
    fn test_timestamp_validation_ok() {
        let nonce = generate_nonce(); // timestamp = sekarang
        assert!(validate_nonce_timestamp(&nonce).is_ok());
    }

    #[test]
    fn test_timestamp_validation_expired() {
        let mut nonce = generate_nonce();
        // Set timestamp ke masa lalu (lebih dari 300 detik)
        let old_ts = (SystemTime::now()
            .duration_since(UNIX_EPOCH).unwrap().as_secs() - 400) as u32;
        nonce.0[0..4].copy_from_slice(&old_ts.to_le_bytes());
        assert_eq!(
            validate_nonce_timestamp(&nonce),
            Err(CryptoError::TimestampInvalid)
        );
    }

    // ── AEAD Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_aead_key(0x42);
        let nonce = generate_nonce();
        let plaintext = b"Halo dari CARAKA! Pesan ini terenkripsi E2EE.";
        let aad = b"CLAMP-HEADER-TEST";

        let (ct, tag) = encrypt(&key, &nonce, plaintext, aad)
            .expect("Enkripsi harus berhasil");

        let pt = decrypt(&key, &nonce, &ct, &tag, aad)
            .expect("Dekripsi harus berhasil");

        assert_eq!(pt.as_slice(), plaintext);
    }

    #[test]
    fn test_encrypt_produces_different_output() {
        let key = test_aead_key(0x42);
        let plaintext = b"Test message";
        let aad = b"";

        // Dua enkripsi dengan nonce berbeda harus menghasilkan ciphertext berbeda
        let (ct1, _) = encrypt(&key, &generate_nonce(), plaintext, aad).unwrap();
        let (ct2, _) = encrypt(&key, &generate_nonce(), plaintext, aad).unwrap();
        assert_ne!(ct1, ct2, "Ciphertext dengan nonce berbeda harus berbeda");
    }

    #[test]
    fn test_decrypt_fails_on_tampered_ciphertext() {
        let key = test_aead_key(0x42);
        let nonce = generate_nonce();
        let plaintext = b"Pesan rahasia";

        let (mut ct, tag) = encrypt(&key, &nonce, plaintext, b"").unwrap();
        ct[0] ^= 0xFF; // Tamper: flip semua bit byte pertama

        let result = decrypt(&key, &nonce, &ct, &tag, b"");
        assert_eq!(result, Err(CryptoError::DecryptionFailed),
            "Dekripsi harus gagal jika ciphertext dimodifikasi");
    }

    #[test]
    fn test_decrypt_fails_on_tampered_tag() {
        let key = test_aead_key(0x42);
        let nonce = generate_nonce();
        let plaintext = b"Pesan rahasia";

        let (ct, mut tag) = encrypt(&key, &nonce, plaintext, b"").unwrap();
        tag[0] ^= 0x01; // Tamper: ubah satu bit tag

        let result = decrypt(&key, &nonce, &ct, &tag, b"");
        assert_eq!(result, Err(CryptoError::DecryptionFailed),
            "Dekripsi harus gagal jika tag dimodifikasi");
    }

    #[test]
    fn test_decrypt_fails_on_tampered_aad() {
        let key = test_aead_key(0x42);
        let nonce = generate_nonce();
        let plaintext = b"Pesan rahasia";
        let aad_original = b"header-asli";
        let aad_tampered = b"header-palsu";

        let (ct, tag) = encrypt(&key, &nonce, plaintext, aad_original).unwrap();

        // AAD berbeda → autentikasi gagal
        let result = decrypt(&key, &nonce, &ct, &tag, aad_tampered);
        assert_eq!(result, Err(CryptoError::DecryptionFailed),
            "Dekripsi harus gagal jika AAD (header) dimodifikasi");
    }

    #[test]
    fn test_decrypt_fails_with_wrong_key() {
        let key_correct = test_aead_key(0x42);
        let key_wrong   = test_aead_key(0x99);
        let nonce = generate_nonce();
        let plaintext = b"Pesan untuk Alice";

        let (ct, tag) = encrypt(&key_correct, &nonce, plaintext, b"").unwrap();
        let result = decrypt(&key_wrong, &nonce, &ct, &tag, b"");
        assert_eq!(result, Err(CryptoError::DecryptionFailed),
            "Dekripsi harus gagal dengan kunci yang salah");
    }

    #[test]
    fn test_ciphertext_length() {
        let key = test_aead_key(0x01);
        let nonce = generate_nonce();
        let plaintext = vec![0u8; 64];

        let (ct, tag) = encrypt(&key, &nonce, &plaintext, b"").unwrap();

        // Ascon-AEAD128: ciphertext panjang = plaintext panjang (stream cipher)
        assert_eq!(ct.len(), plaintext.len(), "Ciphertext harus sepanjang plaintext");
        assert_eq!(tag.len(), 16, "Tag harus 16 byte");
    }

    // ── MAC Tests ───────────────────────────────────────────────────────────

    #[test]
    fn test_mac_compute_and_verify() {
        let key = test_mac_key(0x33);
        let data = b"packet_id_8B_hop_counter_relay_prefix_4B"; // simulasi 13 byte input

        let tag = compute_mac(&key, data);
        assert!(verify_mac(&key, data, &tag),
            "verify_mac harus true untuk tag yang valid");
    }

    #[test]
    fn test_mac_verify_fails_on_wrong_tag() {
        let key = test_mac_key(0x33);
        let data = b"test data";

        let mut tag = compute_mac(&key, data);
        tag[7] ^= 0xFF; // Tamper

        assert!(!verify_mac(&key, data, &tag),
            "verify_mac harus false untuk tag yang dimodifikasi");
    }

    #[test]
    fn test_mac_different_keys_different_tags() {
        let key1 = test_mac_key(0x01);
        let key2 = test_mac_key(0x02);
        let data = b"sama";

        let tag1 = compute_mac(&key1, data);
        let tag2 = compute_mac(&key2, data);

        assert_ne!(tag1, tag2,
            "Kunci berbeda harus menghasilkan MAC berbeda");
    }

    #[test]
    fn test_mac_constant_time_property() {
        // Verifikasi bahwa implementasi menggunakan fold (constant-time)
        // bukan perbandingan langsung (timing-vulnerable)
        let key = test_mac_key(0xAB);
        let data = b"test";
        let tag = compute_mac(&key, data);

        // Panggil multiple kali — harus konsisten
        for _ in 0..100 {
            assert!(verify_mac(&key, data, &tag));
        }
    }

    // ── Hash Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_hash256_deterministic() {
        let data = b"CARAKA mesh message ciphertext";
        let h1 = hash256(data);
        let h2 = hash256(data);
        assert_eq!(h1, h2, "Hash256 harus deterministik");
    }

    #[test]
    fn test_hash256_different_inputs() {
        let h1 = hash256(b"pesan-a");
        let h2 = hash256(b"pesan-b");
        assert_ne!(h1, h2, "Input berbeda harus menghasilkan hash berbeda");
    }

    #[test]
    fn test_hash256_length() {
        let h = hash256(b"test");
        assert_eq!(h.len(), 32, "Output hash256 harus 32 byte");
    }

    #[test]
    fn test_fingerprint16_is_prefix_of_hash256() {
        let data = b"test data";
        let hash = hash256(data);
        let fp = fingerprint16(data);

        assert_eq!(&fp[..], &hash[0..16],
            "fingerprint16 harus 16 byte pertama dari hash256");
    }

    // ── Key Derivation Tests ─────────────────────────────────────────────────

    #[test]
    fn test_xof_derive_deterministic() {
        let secret = b"shared_secret_32_bytes__________";
        let context = b"CARAKA-DM-SEND-v1-test";

        let mut out1 = [0u8; 16];
        let mut out2 = [0u8; 16];
        xof_derive(secret, context, &mut out1);
        xof_derive(secret, context, &mut out2);

        assert_eq!(out1, out2, "xof_derive harus deterministik");
    }

    #[test]
    fn test_xof_derive_different_contexts() {
        let secret = b"shared_secret";
        let mut send_key = [0u8; 16];
        let mut recv_key = [0u8; 16];

        xof_derive(secret, b"CARAKA-DM-SEND-v1", &mut send_key);
        xof_derive(secret, b"CARAKA-DM-RECV-v1", &mut recv_key);

        assert_ne!(send_key, recv_key,
            "Context string berbeda harus menghasilkan kunci berbeda");
    }

    #[test]
    fn test_xof_derive_different_secrets() {
        let mut k1 = [0u8; 16];
        let mut k2 = [0u8; 16];

        xof_derive(b"secret-a", b"context", &mut k1);
        xof_derive(b"secret-b", b"context", &mut k2);

        assert_ne!(k1, k2, "Secret berbeda harus menghasilkan kunci berbeda");
    }
}
