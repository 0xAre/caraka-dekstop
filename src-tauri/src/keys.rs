// src-tauri/src/keys.rs
//
// Modul manajemen kunci kriptografi CARAKA Desktop.
// Mengimplementasikan:
//   - X25519 keypair: identitas permanen setiap node
//   - ECDH (Diffie-Hellman): shared secret antar dua node
//   - DM-Key derivation: kunci AEAD per-sesi, berubah tiap pesan
//   - Channel MAC-Key: kunci autentikasi relay, dibagikan ke channel
//   - Fingerprint: 8-char hex untuk verifikasi identitas out-of-band
//
// PROPERTI KEAMANAN:
//   - Private key TIDAK PERNAH keluar dari device
//   - Semua tipe kunci implement Zeroize (hapus dari memori saat di-drop)
//   - ECDH bersifat commutative: X25519(a,B) == X25519(b,A)
//   - DM-Key berbeda untuk setiap pesan (msg_counter increment)

use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};
use crate::crypto::{AeadKey, MacKey};

// ─── Tipe Kunci ─────────────────────────────────────────────────────────────

/// Private key X25519 node — identitas permanen.
/// TIDAK PERNAH dikirim ke jaringan.
/// Otomatis di-zero dari memori saat di-drop (ZeroizeOnDrop).
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct NodePrivateKey(pub [u8; 32]);

impl std::fmt::Debug for NodePrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodePrivateKey([REDACTED - NEVER LOG PRIVATE KEYS])")
    }
}

/// Public key X25519 node — ini adalah Node ID yang disebarkan ke jaringan.
/// Setiap node diidentifikasi oleh public key-nya (64-char hex).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct NodePublicKey(pub [u8; 32]);

impl NodePublicKey {
    /// Encode Node ID sebagai hex string (64 karakter).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Decode Node ID dari hex string.
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(NodePublicKey(arr))
    }
}

impl std::fmt::Display for NodePublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex()[..16]) // Tampilkan 16 char pertama saja
    }
}

// ─── Identitas Node ─────────────────────────────────────────────────────────

/// Generate keypair X25519 baru menggunakan OsRng (cryptographically secure).
///
/// Dipanggil SEKALI saat node pertama kali dijalankan.
/// Hasil disimpan ke key store SQLite untuk persistensi.
pub fn generate_keypair() -> (NodePrivateKey, NodePublicKey) {
    use rand::rngs::OsRng;
    let private = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&private);
    (
        NodePrivateKey(private.to_bytes()),
        NodePublicKey(public.to_bytes()),
    )
}

/// Derive public key dari private key.
///
/// Digunakan saat load keypair dari database — kita hanya simpan private key,
/// public key selalu dapat di-derive kembali.
pub fn public_key_from_private(private_key: &NodePrivateKey) -> NodePublicKey {
    let private = StaticSecret::from(private_key.0);
    let public = PublicKey::from(&private);
    NodePublicKey(public.to_bytes())
}

/// Load keypair dari penyimpanan lokal, atau generate baru jika belum ada.
///
/// Implementasi penuh: baca dari SQLite key store.
/// Stub saat ini: generate fresh (tanpa persistensi).
/// TODO Fase 5: implementasi baca dari `local_keys` table di SQLite.
pub fn load_or_generate_identity() -> Result<(NodePrivateKey, NodePublicKey), anyhow::Error> {
    // TODO (Fase 5): coba baca dari SQLite key store dulu
    // if let Ok(kp) = load_from_keystore() { return Ok(kp); }
    Ok(generate_keypair())
}

/// Hitung fingerprint 8-karakter untuk verifikasi identitas out-of-band.
///
/// Fingerprint digunakan untuk:
///   - Verifikasi verbal ("node kamu fingerprintnya ABCD1234?")
///   - QR code verification di UI
///   - Deteksi Man-in-the-Middle dalam TOFU model
///
/// Format: hex string 8 karakter (4 byte pertama dari Hash256(public_key))
pub fn fingerprint(public_key: &NodePublicKey) -> String {
    let hash = crate::crypto::hash256(&public_key.0);
    hex::encode(&hash[0..4]) // 8 hex chars
}

// ─── ECDH Key Exchange ───────────────────────────────────────────────────────

/// Hitung Shared Secret menggunakan X25519 ECDH.
///
/// Sifat commutative (kunci tidak tergantung siapa yang menginisiasi):
///   X25519(alice_private, bob_public) == X25519(bob_private, alice_public)
///
/// Shared Secret 32 byte ini adalah INPUT untuk derive_dm_key() —
/// jangan gunakan langsung sebagai kunci enkripsi!
pub fn ecdh(my_private: &NodePrivateKey, peer_public: &NodePublicKey) -> [u8; 32] {
    let private = StaticSecret::from(my_private.0);
    let public = PublicKey::from(peer_public.0);
    let shared = private.diffie_hellman(&public);
    shared.to_bytes()
}

// ─── Key Derivation ──────────────────────────────────────────────────────────

/// Turunkan DM-Key (AeadKey 128-bit) untuk enkripsi Direct Message.
///
/// Kunci BERUBAH setiap pesan karena msg_counter di-increment.
/// Ini mengimplementasikan simple forward secrecy berbasis sesi:
/// kompromi satu kunci tidak membuka pesan sebelum/sesudahnya.
///
/// Context string berbeda untuk sender dan receiver WAJIB berbeda
/// untuk mencegah key confusion (sender tidak bisa dekripsi miliknya sendiri).
///
/// Derivation:
///   aead_key = HKDF-SHA256(
///     IKM  = shared_secret (dari ECDH),
///     info = "CARAKA-DM-v1" || sender_id || receiver_id || session_id || msg_counter
///   )[0..16]
pub fn derive_dm_key(
    shared_secret: &[u8; 32],
    sender_id: &NodePublicKey,
    receiver_id: &NodePublicKey,
    session_id: &[u8; 8],
    msg_counter: u64,
) -> AeadKey {
    // Build context: label + ID pengirim + ID penerima + session + counter
    // Sender dan receiver selalu diurutkan sama: (sender_id, receiver_id)
    // sehingga SEND key Alice == RECV key Bob otomatis via ECDH commutativity
    let mut context = Vec::with_capacity(12 + 32 + 32 + 8 + 8);
    context.extend_from_slice(b"CARAKA-DM-v1");             // 12 byte
    context.extend_from_slice(&sender_id.0);                // 32 byte
    context.extend_from_slice(&receiver_id.0);              // 32 byte
    context.extend_from_slice(session_id);                  // 8 byte
    context.extend_from_slice(&msg_counter.to_le_bytes());  // 8 byte

    let mut key_bytes = [0u8; 16];
    crate::crypto::xof_derive(shared_secret, &context, &mut key_bytes);
    AeadKey(key_bytes)
}

/// Turunkan session_id (8 byte) secara DETERMINISTIK dari ECDH shared_secret.
///
/// [FIX KRITIS] Sebelumnya session_id di-generate acak (OsRng) secara LOKAL oleh
/// masing-masing node dan TIDAK PERNAH dipertukarkan lewat wire protocol.
/// Akibatnya session_id Alice != session_id Bob untuk percakapan yang sama,
/// sehingga derive_dm_key() menghasilkan AEAD key yang berbeda di kedua sisi —
/// SEMUA pesan DM/File gagal didekripsi oleh lawan bicara (LAN maupun Tor),
/// walaupun koneksi transport-nya sendiri berhasil.
///
/// X25519 ECDH bersifat komutatif: shared_secret_Alice == shared_secret_Bob.
/// Dengan menurunkan session_id dari shared_secret (bukan acak), kedua node
/// independen SELALU mendapat session_id yang identik tanpa perlu pertukaran
/// pesan tambahan.
pub fn derive_session_id(shared_secret: &[u8; 32]) -> [u8; 8] {
    let mut id = [0u8; 8];
    crate::crypto::xof_derive(shared_secret, b"CARAKA-SESSION-ID-v1", &mut id);
    id
}

/// Alias untuk derive_dm_key dengan parameter is_sender untuk clarity di commands.rs.
///
/// Ketika is_sender=true: sender=my_id, receiver=peer_id  (Alice kirim ke Bob)
/// Ketika is_sender=false: sender=peer_id, receiver=my_id (Bob kirim ke Alice)
pub fn derive_dm_key_directional(
    shared_secret: &[u8; 32],
    my_id: &NodePublicKey,
    peer_id: &NodePublicKey,
    session_id: &[u8; 8],
    msg_counter: u64,
    is_sender: bool,
) -> AeadKey {
    if is_sender {
        derive_dm_key(shared_secret, my_id, peer_id, session_id, msg_counter)
    } else {
        derive_dm_key(shared_secret, peer_id, my_id, session_id, msg_counter)
    }
}

/// Turunkan Channel MAC-Key untuk autentikasi per-hop relay.
///
/// Kunci ini DIBAGIKAN ke semua anggota channel (via QR code / verbal / side-channel).
/// BUKAN untuk enkripsi payload — hanya untuk relay authentication.
///
/// Setiap channel memiliki MAC-Key yang unik (dibedakan oleh channel_id).
pub fn derive_channel_mac_key(master_secret: &[u8], channel_id: &[u8; 8]) -> MacKey {
    let mut context = Vec::with_capacity(16 + 8);
    context.extend_from_slice(b"CARAKA-CH-MAC-v1");
    context.extend_from_slice(channel_id);

    let mut key_bytes = [0u8; 16];
    crate::crypto::xof_derive(master_secret, &context, &mut key_bytes);
    MacKey(key_bytes)
}

// ─── Unit Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Keypair Tests ────────────────────────────────────────────────────────

    #[test]
    fn test_generate_keypair_not_zero() {
        let (priv_key, pub_key) = generate_keypair();
        assert_ne!(priv_key.0, [0u8; 32], "Private key tidak boleh semua nol");
        assert_ne!(pub_key.0, [0u8; 32], "Public key tidak boleh semua nol");
    }

    #[test]
    fn test_generate_keypair_unique() {
        let (_, pub1) = generate_keypair();
        let (_, pub2) = generate_keypair();
        assert_ne!(pub1.0, pub2.0, "Dua keypair tidak boleh identik");
    }

    #[test]
    fn test_public_key_hex_roundtrip() {
        let (_, pub_key) = generate_keypair();
        let hex_str = pub_key.to_hex();
        let recovered = NodePublicKey::from_hex(&hex_str).expect("Parsing hex harus berhasil");
        assert_eq!(pub_key, recovered, "Hex roundtrip harus identik");
    }

    #[test]
    fn test_public_key_hex_length() {
        let (_, pub_key) = generate_keypair();
        assert_eq!(pub_key.to_hex().len(), 64, "Hex Node ID harus 64 karakter");
    }

    #[test]
    fn test_fingerprint_length() {
        let (_, pub_key) = generate_keypair();
        let fp = fingerprint(&pub_key);
        assert_eq!(fp.len(), 8, "Fingerprint harus 8 karakter hex");
    }

    #[test]
    fn test_fingerprint_deterministic() {
        let (_, pub_key) = generate_keypair();
        let fp1 = fingerprint(&pub_key);
        let fp2 = fingerprint(&pub_key);
        assert_eq!(fp1, fp2, "Fingerprint harus deterministik");
    }

    // ── ECDH Tests ───────────────────────────────────────────────────────────

    #[test]
    fn test_ecdh_commutativity() {
        let (alice_priv, alice_pub) = generate_keypair();
        let (bob_priv, bob_pub) = generate_keypair();

        let alice_shared = ecdh(&alice_priv, &bob_pub);
        let bob_shared   = ecdh(&bob_priv, &alice_pub);

        assert_eq!(alice_shared, bob_shared,
            "ECDH harus commutative: X25519(a,B) == X25519(b,A)");
    }

    #[test]
    fn test_ecdh_different_pairs() {
        let (alice_priv, _alice_pub) = generate_keypair();
        let (_bob_priv,   bob_pub)   = generate_keypair();
        let (_charlie_priv, charlie_pub) = generate_keypair();

        let alice_bob     = ecdh(&alice_priv, &bob_pub);
        let alice_charlie = ecdh(&alice_priv, &charlie_pub);

        assert_ne!(alice_bob, alice_charlie,
            "Shared secret dengan peer berbeda harus berbeda");
    }

    #[test]
    fn test_ecdh_not_zero() {
        let (alice_priv, _) = generate_keypair();
        let (_, bob_pub)    = generate_keypair();
        let shared = ecdh(&alice_priv, &bob_pub);
        assert_ne!(shared, [0u8; 32], "Shared secret tidak boleh semua nol");
    }

    // ── Key Derivation Tests ─────────────────────────────────────────────────

    #[test]
    fn test_dm_key_symmetry() {
        // Properti utama: alice SEND key == bob RECV key (dan sebaliknya)
        let (alice_priv, alice_pub) = generate_keypair();
        let (bob_priv, bob_pub)     = generate_keypair();
        let session_id = [1u8; 8];
        let msg_counter = 42u64;

        // Shared secret dari kedua sisi (harus sama karena ECDH commutative)
        let shared_alice = ecdh(&alice_priv, &bob_pub);
        let shared_bob   = ecdh(&bob_priv, &alice_pub);
        assert_eq!(shared_alice, shared_bob); // sanity check

        // Alice SEND key (Alice -> Bob) == Bob RECV key (Alice -> Bob)
        let alice_send = derive_dm_key(
            &shared_alice, &alice_pub, &bob_pub, &session_id, msg_counter
        );
        let bob_recv = derive_dm_key(
            &shared_bob, &alice_pub, &bob_pub, &session_id, msg_counter
        );
        assert_eq!(alice_send.0, bob_recv.0,
            "Alice SEND key harus identik dengan Bob RECV key");

        // Bob SEND key (Bob -> Alice) == Alice RECV key (Bob -> Alice)
        let bob_send = derive_dm_key(
            &shared_bob, &bob_pub, &alice_pub, &session_id, msg_counter
        );
        let alice_recv = derive_dm_key(
            &shared_alice, &bob_pub, &alice_pub, &session_id, msg_counter
        );
        assert_eq!(bob_send.0, alice_recv.0,
            "Bob SEND key harus identik dengan Alice RECV key");
    }

    #[test]
    fn test_dm_key_changes_per_message() {
        let (alice_priv, alice_pub) = generate_keypair();
        let (_, bob_pub)           = generate_keypair();
        let session_id = [0u8; 8];
        let shared = ecdh(&alice_priv, &bob_pub);

        let key_msg0 = derive_dm_key(&shared, &alice_pub, &bob_pub, &session_id, 0);
        let key_msg1 = derive_dm_key(&shared, &alice_pub, &bob_pub, &session_id, 1);
        let key_msg2 = derive_dm_key(&shared, &alice_pub, &bob_pub, &session_id, 2);

        assert_ne!(key_msg0.0, key_msg1.0, "Kunci tiap pesan harus berbeda");
        assert_ne!(key_msg1.0, key_msg2.0, "Kunci tiap pesan harus berbeda");
    }

    #[test]
    fn test_send_recv_keys_are_different() {
        let (alice_priv, alice_pub) = generate_keypair();
        let (_, bob_pub)           = generate_keypair();
        let shared = ecdh(&alice_priv, &bob_pub);
        let session_id = [0u8; 8];

        let send_key = derive_dm_key(&shared, &alice_pub, &bob_pub, &session_id, 0);
        let recv_key = derive_dm_key(&shared, &bob_pub, &alice_pub, &session_id, 0);

        assert_ne!(send_key.0, recv_key.0,
            "SEND key dan RECV key untuk pesan yang sama harus berbeda");
    }

    #[test]
    fn test_channel_mac_key_derivation() {
        let master = b"master_secret_32bytes___________";
        let channel_id = [0xCAu8; 8];

        let key1 = derive_channel_mac_key(master, &channel_id);
        let key2 = derive_channel_mac_key(master, &channel_id);

        assert_eq!(key1.0, key2.0, "Channel MAC key harus deterministik");
    }

    #[test]
    fn test_different_channels_different_keys() {
        let master = b"master_secret";
        let ch1 = [0x01u8; 8];
        let ch2 = [0x02u8; 8];

        let key1 = derive_channel_mac_key(master, &ch1);
        let key2 = derive_channel_mac_key(master, &ch2);

        assert_ne!(key1.0, key2.0,
            "Channel berbeda harus menghasilkan MAC key berbeda");
    }

    // ── Integration Test: Full E2E Crypto Pipeline ───────────────────────────

    #[test]
    fn test_full_e2e_pipeline() {
        // Simulasi: Alice kirim DM ke Bob, Bob dekripsi.
        //
        // [REGRESSION TEST] session_id SENGAJA diturunkan SECARA TERPISAH oleh
        // Alice dan Bob dari shared_secret masing-masing (BUKAN dibagi lewat
        // variabel yang sama seperti sebelumnya) — ini mensimulasikan dua node
        // independen yang tidak pernah bertukar session_id lewat wire protocol.
        // Sebelum fix derive_session_id() ada, kedua sisi men-generate
        // session_id ACAK secara lokal sehingga TIDAK PERNAH cocok, dan test
        // lama (memakai satu variabel session_id yang dipakai bersama) tidak
        // pernah menangkap bug ini karena secara tidak sengaja "curang".
        let (alice_priv, alice_pub) = generate_keypair();
        let (bob_priv, bob_pub)     = generate_keypair();
        let msg_counter = 0u64;
        let message = b"Halo Bob! Ini pesan terenkripsi dari Alice via CARAKA.";

        // 1. Alice: ECDH + derive session_id + send key (semua dari sisi Alice)
        let shared_alice = ecdh(&alice_priv, &bob_pub);
        let session_id = derive_session_id(&shared_alice);
        let alice_send_key = derive_dm_key(
            &shared_alice, &alice_pub, &bob_pub, &session_id, msg_counter
        );

        // 2. Alice: Enkripsi
        let nonce = crate::crypto::generate_nonce();
        let aad   = b"CLAMP-HEADER-SIMULASI";
        let (ciphertext, tag) = crate::crypto::encrypt(
            &alice_send_key, &nonce, message, aad
        ).expect("Alice enkripsi harus berhasil");

        // 3. Bob: ECDH + derive session_id SENDIRI (independen dari Alice) + recv key.
        // X25519 komutatif → shared_bob == shared_alice → session_id_bob HARUS
        // == session_id yang dipakai Alice tanpa pertukaran pesan apa pun.
        let shared_bob = ecdh(&bob_priv, &alice_pub);
        let session_id_bob = derive_session_id(&shared_bob);
        assert_eq!(
            session_id_bob, session_id,
            "session_id Bob harus identik dengan Alice tanpa pertukaran wire — \
             kalau ini gagal, semua DM antar node independen tidak akan pernah terdekripsi"
        );
        let bob_recv_key = derive_dm_key(
            &shared_bob, &alice_pub, &bob_pub, &session_id_bob, msg_counter
        );

        // 4. Bob: Dekripsi
        let plaintext = crate::crypto::decrypt(
            &bob_recv_key, &nonce, &ciphertext, &tag, aad
        ).expect("Bob dekripsi harus berhasil");

        assert_eq!(plaintext.as_slice(), message,
            "Plaintext yang didekripsi Bob harus identik dengan pesan Alice");

        // 5. Pastikan Bob tidak bisa dekripsi dengan kunci yang salah
        let (_, charlie_pub) = generate_keypair();
        let shared_charlie = ecdh(&bob_priv, &charlie_pub);
        let wrong_key = derive_dm_key(
            &shared_charlie, &charlie_pub, &bob_pub, &session_id, msg_counter
        );
        assert!(
            crate::crypto::decrypt(&wrong_key, &nonce, &ciphertext, &tag, aad).is_err(),
            "Dekripsi dengan kunci salah harus gagal"
        );
    }
}
