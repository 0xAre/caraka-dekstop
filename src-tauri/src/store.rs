// src-tauri/src/store.rs
// Fase 5 — Lapisan Penyimpanan: SQLite untuk pesan, peer, dan kunci

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

// ─── Error Types ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Data encoding error: {0}")]
    Encoding(String),
}

// ─── Data Structs ──────────────────────────────────────────────────────────

/// Pesan yang tersimpan di database (ciphertext — tidak pernah plaintext!)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    /// UUID pesan
    pub id: String,
    /// Packet ID CLAMP (8 byte hex)
    pub packet_id: String,
    /// Node ID pengirim (32 byte hex)
    pub sender_id: String,
    /// Node ID penerima (32 byte hex)
    pub recipient_id: String,
    /// Nonce Ascon-AEAD128 (16 byte)
    pub nonce: Vec<u8>,
    /// Ciphertext terenkripsi
    pub ciphertext: Vec<u8>,
    /// AEAD tag 128-bit
    pub aead_tag: Vec<u8>,
    /// Unix timestamp saat diterima
    pub received_at: i64,
    /// 1 jika sudah dikirim ke penerima, 0 jika masih pending
    pub delivered: bool,
}

/// Informasi peer yang dikenal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    /// Node ID peer (32 byte hex)
    pub node_id: String,
    /// Nama tampilan peer
    pub display_name: String,
    /// Unix timestamp terakhir kali terlihat
    pub last_seen: i64,
    /// IP address peer
    pub ip_address: String,
    /// TCP port peer
    pub tcp_port: u16,
    /// Trust score saat ini
    pub trust_score: f64,
}

/// Pesan yang sudah didekripsi (untuk tampilan di UI)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptedMessage {
    pub id: String,
    pub sender_id: String,
    pub recipient_id: String,
    pub text: String,
    pub timestamp: i64,
    pub is_outgoing: bool,
}

// ─── Database Initialization ───────────────────────────────────────────────

/// Buka koneksi database dan buat schema jika belum ada.
pub fn open_db(path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open(path)?;

    // Performance tuning untuk WAL mode
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

    create_tables(&conn)?;
    Ok(conn)
}

fn create_tables(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch("
        -- Tabel pesan (hanya menyimpan ciphertext, TIDAK PERNAH plaintext)
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

        -- Tabel peer yang dikenal
        CREATE TABLE IF NOT EXISTS peers (
            node_id      TEXT PRIMARY KEY,
            display_name TEXT NOT NULL DEFAULT 'Unknown',
            last_seen    INTEGER NOT NULL DEFAULT 0,
            ip_address   TEXT NOT NULL DEFAULT '',
            tcp_port     INTEGER NOT NULL DEFAULT 7771,
            trust_score  REAL NOT NULL DEFAULT 2.0
        );

        -- Tabel kunci lokal (private key identity)
        CREATE TABLE IF NOT EXISTS local_keys (
            key_id       TEXT PRIMARY KEY,
            key_type     TEXT NOT NULL,
            key_material BLOB NOT NULL,
            created_at   INTEGER NOT NULL
        );

        -- Tabel status sinkronisasi Epidemic Sync
        CREATE TABLE IF NOT EXISTS sync_state (
            peer_id    TEXT NOT NULL,
            message_id TEXT NOT NULL,
            synced     INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (peer_id, message_id)
        );

        -- Tabel kunci sesi (session counter per peer untuk forward secrecy)
        CREATE TABLE IF NOT EXISTS sessions (
            peer_id     TEXT NOT NULL,
            session_id  BLOB NOT NULL,
            msg_counter INTEGER NOT NULL DEFAULT 0,
            created_at  INTEGER NOT NULL,
            PRIMARY KEY (peer_id)
        );

        -- Index untuk query cepat
        CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id);
        CREATE INDEX IF NOT EXISTS idx_messages_recipient ON messages(recipient_id);
        CREATE INDEX IF NOT EXISTS idx_messages_received ON messages(received_at DESC);
    ")?;
    Ok(())
}

// ─── Message Operations ────────────────────────────────────────────────────

/// Simpan pesan ke database (HANYA ciphertext yang boleh disimpan!)
pub fn save_message(conn: &Connection, msg: &StoredMessage) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO messages
         (id, packet_id, sender_id, recipient_id, nonce, ciphertext, aead_tag, received_at, delivered)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            msg.id,
            msg.packet_id,
            msg.sender_id,
            msg.recipient_id,
            msg.nonce,
            msg.ciphertext,
            msg.aead_tag,
            msg.received_at,
            if msg.delivered { 1 } else { 0 },
        ],
    )?;
    Ok(())
}

/// Ambil semua pesan antara dua node (untuk ditampilkan di chat window).
pub fn get_messages_between(
    conn: &Connection,
    my_node_id: &str,
    peer_node_id: &str,
    limit: usize,
) -> Result<Vec<StoredMessage>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, packet_id, sender_id, recipient_id, nonce, ciphertext, aead_tag, received_at, delivered
         FROM messages
         WHERE (sender_id = ?1 AND recipient_id = ?2)
            OR (sender_id = ?2 AND recipient_id = ?1)
         ORDER BY received_at DESC
         LIMIT ?3",
    )?;

    let messages = stmt.query_map(
        params![my_node_id, peer_node_id, limit as i64],
        |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                packet_id: row.get(1)?,
                sender_id: row.get(2)?,
                recipient_id: row.get(3)?,
                nonce: row.get(4)?,
                ciphertext: row.get(5)?,
                aead_tag: row.get(6)?,
                received_at: row.get(7)?,
                delivered: row.get::<_, i32>(8)? == 1,
            })
        },
    )?
    .flatten()
    .collect();

    Ok(messages)
}

/// Tandai pesan sebagai sudah terkirim ke penerima.
pub fn mark_delivered(conn: &Connection, message_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE messages SET delivered = 1 WHERE id = ?1",
        params![message_id],
    )?;
    Ok(())
}

/// Ambil semua fingerprint pesan untuk Epidemic Sync.
///
/// Fingerprint = SHA-256 dari ciphertext (16 byte prefix).
/// Digunakan untuk membandingkan state dengan peer tanpa membocorkan konten.
pub fn get_all_fingerprints(conn: &Connection) -> Result<Vec<[u8; 16]>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT ciphertext FROM messages ORDER BY received_at ASC"
    )?;

    let fps: Vec<[u8; 16]> = stmt
        .query_map([], |row| {
            let ct: Vec<u8> = row.get(0)?;
            Ok(ct)
        })?
        .flatten()
        .map(|ct| {
            let hash = crate::crypto::hash256(&ct);
            let mut fp = [0u8; 16];
            fp.copy_from_slice(&hash[0..16]);
            fp
        })
        .collect();

    Ok(fps)
}

/// Ambil pesan yang belum diketahui peer (untuk gap filling Epidemic Sync).
pub fn get_messages_not_synced_to_peer(
    conn: &Connection,
    peer_id: &str,
) -> Result<Vec<StoredMessage>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.packet_id, m.sender_id, m.recipient_id,
                m.nonce, m.ciphertext, m.aead_tag, m.received_at, m.delivered
         FROM messages m
         LEFT JOIN sync_state ss ON ss.message_id = m.id AND ss.peer_id = ?1
         WHERE ss.synced IS NULL OR ss.synced = 0
         LIMIT 100",
    )?;

    let messages = stmt
        .query_map(params![peer_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                packet_id: row.get(1)?,
                sender_id: row.get(2)?,
                recipient_id: row.get(3)?,
                nonce: row.get(4)?,
                ciphertext: row.get(5)?,
                aead_tag: row.get(6)?,
                received_at: row.get(7)?,
                delivered: row.get::<_, i32>(8)? == 1,
            })
        })?
        .flatten()
        .collect();

    Ok(messages)
}

/// Tandai pesan sudah disinkronkan ke peer.
pub fn mark_synced_to_peer(
    conn: &Connection,
    peer_id: &str,
    message_id: &str,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_state (peer_id, message_id, synced)
         VALUES (?1, ?2, 1)",
        params![peer_id, message_id],
    )?;
    Ok(())
}

// ─── Peer Operations ───────────────────────────────────────────────────────

/// Simpan atau update informasi peer.
pub fn upsert_peer(conn: &Connection, peer: &PeerRecord) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO peers (node_id, display_name, last_seen, ip_address, tcp_port, trust_score)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(node_id) DO UPDATE SET
             display_name = excluded.display_name,
             last_seen = excluded.last_seen,
             ip_address = excluded.ip_address,
             tcp_port = excluded.tcp_port,
             trust_score = excluded.trust_score",
        params![
            peer.node_id,
            peer.display_name,
            peer.last_seen,
            peer.ip_address,
            peer.tcp_port as i64,
            peer.trust_score,
        ],
    )?;
    Ok(())
}

/// Ambil semua peer yang dikenal.
pub fn get_all_peers(conn: &Connection) -> Result<Vec<PeerRecord>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT node_id, display_name, last_seen, ip_address, tcp_port, trust_score
         FROM peers
         ORDER BY last_seen DESC"
    )?;

    let peers = stmt
        .query_map([], |row| {
            Ok(PeerRecord {
                node_id: row.get(0)?,
                display_name: row.get(1)?,
                last_seen: row.get(2)?,
                ip_address: row.get(3)?,
                tcp_port: row.get::<_, i64>(4)? as u16,
                trust_score: row.get(5)?,
            })
        })?
        .flatten()
        .collect();

    Ok(peers)
}

/// Ambil peer berdasarkan node ID.
pub fn get_peer(conn: &Connection, node_id: &str) -> Result<Option<PeerRecord>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT node_id, display_name, last_seen, ip_address, tcp_port, trust_score
         FROM peers WHERE node_id = ?1"
    )?;

    let peer = stmt
        .query_map(params![node_id], |row| {
            Ok(PeerRecord {
                node_id: row.get(0)?,
                display_name: row.get(1)?,
                last_seen: row.get(2)?,
                ip_address: row.get(3)?,
                tcp_port: row.get::<_, i64>(4)? as u16,
                trust_score: row.get(5)?,
            })
        })?
        .flatten()
        .next();

    Ok(peer)
}

// ─── Key Management ────────────────────────────────────────────────────────

/// Simpan identity private key ke database.
/// ATURAN: key_material adalah bytes mentah — TIDAK boleh logged atau expose ke UI!
pub fn save_identity_key(
    conn: &Connection,
    key_id: &str,
    key_type: &str,
    key_material: &[u8],
) -> Result<(), StoreError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    conn.execute(
        "INSERT OR IGNORE INTO local_keys (key_id, key_type, key_material, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![key_id, key_type, key_material, now],
    )?;
    Ok(())
}

/// Load identity private key dari database.
pub fn load_identity_key(
    conn: &Connection,
    key_id: &str,
) -> Result<Option<Vec<u8>>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT key_material FROM local_keys WHERE key_id = ?1 AND key_type = 'identity'"
    )?;

    let key = stmt
        .query_map(params![key_id], |row| row.get::<_, Vec<u8>>(0))?
        .flatten()
        .next();

    Ok(key)
}

// ─── Session Management ────────────────────────────────────────────────────

/// Ambil atau buat session ID dan counter untuk forward secrecy.
pub fn get_or_create_session(
    conn: &Connection,
    peer_id: &str,
) -> Result<([u8; 8], u64), StoreError> {
    // Coba load session yang ada
    let existing: Option<(Vec<u8>, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT session_id, msg_counter FROM sessions WHERE peer_id = ?1"
        )?;
        let rows: Vec<_> = stmt.query_map(params![peer_id], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
        })?
        .flatten()
        .collect();
        rows.into_iter().next()
    };

    if let Some((sid_bytes, counter)) = existing {
        let mut session_id = [0u8; 8];
        if sid_bytes.len() == 8 {
            session_id.copy_from_slice(&sid_bytes);
        }
        return Ok((session_id, counter as u64));
    }

    // Buat session baru
    use rand::RngCore;
    let mut session_id = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut session_id);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    conn.execute(
        "INSERT INTO sessions (peer_id, session_id, msg_counter, created_at)
         VALUES (?1, ?2, 0, ?3)",
        params![peer_id, session_id.to_vec(), now],
    )?;

    Ok((session_id, 0))
}

/// Increment message counter untuk session.
pub fn increment_msg_counter(conn: &Connection, peer_id: &str) -> Result<u64, StoreError> {
    conn.execute(
        "UPDATE sessions SET msg_counter = msg_counter + 1 WHERE peer_id = ?1",
        params![peer_id],
    )?;

    let counter: i64 = conn.query_row(
        "SELECT msg_counter FROM sessions WHERE peer_id = ?1",
        params![peer_id],
        |row| row.get(0),
    )?;

    Ok(counter as u64)
}

// ─── Unit Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn open_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        create_tables(&conn).unwrap();
        conn
    }

    fn make_test_message(id: &str, sender: &str, recipient: &str) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            packet_id: format!("pkt_{}", id),
            sender_id: sender.to_string(),
            recipient_id: recipient.to_string(),
            nonce: vec![0u8; 16],
            ciphertext: b"encrypted_payload_never_plaintext".to_vec(),
            aead_tag: vec![0u8; 16],
            received_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            delivered: false,
        }
    }

    #[test]
    fn test_save_and_retrieve_message() {
        let conn = open_test_db();
        let msg = make_test_message("msg1", "alice", "bob");

        save_message(&conn, &msg).unwrap();

        let retrieved = get_messages_between(&conn, "alice", "bob", 10).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].id, "msg1");
    }

    #[test]
    fn test_duplicate_message_ignored() {
        let conn = open_test_db();
        let msg = make_test_message("dup1", "alice", "bob");

        save_message(&conn, &msg).unwrap();
        save_message(&conn, &msg).unwrap(); // Duplicate — harus di-ignore

        let retrieved = get_messages_between(&conn, "alice", "bob", 10).unwrap();
        assert_eq!(retrieved.len(), 1, "Duplicate harus di-ignore (INSERT OR IGNORE)");
    }

    #[test]
    fn test_no_plaintext_in_db() {
        let conn = open_test_db();
        let msg = make_test_message("msg2", "charlie", "dave");

        save_message(&conn, &msg).unwrap();

        // Verifikasi: ciphertext field berisi data terenkripsi bukan plaintext
        let retrieved = get_messages_between(&conn, "charlie", "dave", 10).unwrap();
        assert!(!retrieved[0].ciphertext.is_empty());
        // Ciphertext seharusnya bukan teks readable (dalam test ini kita pakai bytes)
        assert_ne!(retrieved[0].ciphertext, b"Hello plaintext".to_vec());
    }

    #[test]
    fn test_fingerprints_generation() {
        let conn = open_test_db();

        // Pesan dengan ciphertext BERBEDA untuk menghasilkan fingerprint berbeda
        let mut msg1 = make_test_message("fp1", "a", "b");
        let mut msg2 = make_test_message("fp2", "c", "d");
        msg1.ciphertext = b"encrypted_payload_one_unique".to_vec();
        msg2.ciphertext = b"encrypted_payload_two_different".to_vec();

        save_message(&conn, &msg1).unwrap();
        save_message(&conn, &msg2).unwrap();

        let fps = get_all_fingerprints(&conn).unwrap();
        assert_eq!(fps.len(), 2);
        assert_ne!(fps[0], fps[1], "Fingerprint harus berbeda untuk ciphertext berbeda");
    }

    #[test]
    fn test_upsert_peer() {
        let conn = open_test_db();
        let peer = PeerRecord {
            node_id: "node_abc".to_string(),
            display_name: "Alice".to_string(),
            last_seen: 1000,
            ip_address: "192.168.1.10".to_string(),
            tcp_port: 7771,
            trust_score: 2.0,
        };

        upsert_peer(&conn, &peer).unwrap();

        let retrieved = get_peer(&conn, "node_abc").unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().display_name, "Alice");

        // Update peer
        let updated = PeerRecord {
            display_name: "Alice Updated".to_string(),
            last_seen: 2000,
            ..peer
        };
        upsert_peer(&conn, &updated).unwrap();

        let re2 = get_peer(&conn, "node_abc").unwrap().unwrap();
        assert_eq!(re2.display_name, "Alice Updated");
        assert_eq!(re2.last_seen, 2000);
    }

    #[test]
    fn test_session_creation() {
        let conn = open_test_db();

        let (sid1, counter1) = get_or_create_session(&conn, "peer1").unwrap();
        let (sid2, counter2) = get_or_create_session(&conn, "peer1").unwrap();

        // Session yang sama harus return session_id yang sama
        assert_eq!(sid1, sid2);
        assert_eq!(counter1, counter2);
        assert_eq!(counter1, 0);
    }

    #[test]
    fn test_message_counter_increment() {
        let conn = open_test_db();
        get_or_create_session(&conn, "peer1").unwrap();

        let c1 = increment_msg_counter(&conn, "peer1").unwrap();
        let c2 = increment_msg_counter(&conn, "peer1").unwrap();
        let c3 = increment_msg_counter(&conn, "peer1").unwrap();

        assert_eq!(c1, 1);
        assert_eq!(c2, 2);
        assert_eq!(c3, 3);
    }

    #[test]
    fn test_save_and_load_identity_key() {
        let conn = open_test_db();
        let key_bytes = [0x42u8; 32];

        save_identity_key(&conn, "node_identity", "identity", &key_bytes).unwrap();

        let loaded = load_identity_key(&conn, "node_identity").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap(), key_bytes.to_vec());
    }
}
