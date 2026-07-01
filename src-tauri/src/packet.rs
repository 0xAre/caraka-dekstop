// src-tauri/src/packet.rs
// Fase 2 — Definisi Format Paket CLAMP (Compact Lightweight Authenticated Mesh Protocol)

use rand::rngs::OsRng;
use rand::RngCore;
use thiserror::Error;

// ─── Error Types ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum PacketError {
    #[error("Invalid magic bytes — bukan paket CLAMP")]
    InvalidMagic,
    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(u8),
    #[error("Packet terlalu pendek: minimal {expected} byte, dapat {got}")]
    TooShort { expected: usize, got: usize },
    #[error("Unknown packet type: {0}")]
    UnknownPacketType(u8),
}

// ─── Konstanta Protokol ────────────────────────────────────────────────────

/// Magic bytes "CA" "R" = 0xCA 0x52 (identifikasi paket CLAMP)
pub const MAGIC: [u8; 2] = [0xCA, 0x52];
pub const PROTOCOL_VERSION: u8 = 0x01;
pub const TTL_MAX: u8 = 7;

pub const HEADER_SIZE: usize = 13;
pub const HOP_AUTH_SIZE: usize = 17;
pub const NONCE_SIZE: usize = 16;
pub const TAG_SIZE: usize = 16;
/// Total fixed overhead per paket: 62 byte
pub const FIXED_OVERHEAD: usize = HEADER_SIZE + HOP_AUTH_SIZE + NONCE_SIZE + TAG_SIZE;

// ─── Tipe Paket ────────────────────────────────────────────────────────────

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    /// Direct Message — E2EE antara dua node
    Dm = 0x01,
    /// Channel Message — broadcast ke channel group
    Channel = 0x02,
    /// Sync Request — minta fingerprint vector dari peer
    SyncReq = 0x03,
    /// Sync Response — kirim fingerprint vector ke requester
    SyncResp = 0x04,
    /// Hello/Handshake — kenalkan diri ke peer baru (TCP)
    Hello = 0x05,
    /// Sync Data — kirim ciphertext yang diminta peer
    SyncData = 0x06,
    /// Broadcast darurat — pesan publik mesh flooding tanpa E2EE
    /// Diteruskan ke semua peer sampai TTL habis atau duplicate
    Broadcast = 0x07,
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
            0x07 => Ok(PacketType::Broadcast),
            other => Err(PacketError::UnknownPacketType(other)),
        }
    }
}

// ─── Struct Paket ──────────────────────────────────────────────────────────

/// CLAMP Header — 13 byte, plaintext (dibaca semua relay untuk routing)
///
/// Layout:
///   [0..2]  magic       = 0xCA 0x52
///   [2]     version     = 0x01
///   [3]     packet_type
///   [4]     ttl         (0–7, dikurangi setiap relay)
///   [5..13] packet_id   (origin_node_id[0..4] || OsRng[4..8])
#[derive(Debug, Clone)]
pub struct ClampHeader {
    pub magic: [u8; 2],
    pub version: u8,
    pub packet_type: PacketType,
    pub ttl: u8,
    pub packet_id: [u8; 8],
}

/// HopAuth — 17 byte, diperbarui setiap relay
///
/// Layout:
///   [0]     hop_counter (0 di origin, +1 tiap relay)
///   [1..17] mac_tag     (Ascon-MAC dari packet_id||hop_counter||relay_node_id[0..4])
#[derive(Debug, Clone)]
pub struct HopAuth {
    pub hop_counter: u8,
    pub mac_tag: [u8; 16],
}

/// Paket CLAMP lengkap — format biner untuk transmisi via TCP
///
/// Total size: FIXED_OVERHEAD + ciphertext.len()
///   = 62 + len(ciphertext) byte
#[derive(Debug, Clone)]
pub struct ClampPacket {
    pub header: ClampHeader,
    pub hop_auth: HopAuth,
    pub nonce: [u8; 16],
    pub ciphertext: Vec<u8>,
    pub aead_tag: [u8; 16],
}

// ─── Inner Payload (plaintext sebelum enkripsi) ────────────────────────────

/// Inner payload untuk DM — ini yang dienkripsi menjadi ciphertext
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DmInnerPayload {
    /// Node ID pengirim (32 byte hex)
    pub sender_id: String,
    /// Node ID penerima (32 byte hex)
    pub recipient_id: String,
    /// Teks pesan
    pub text: String,
    /// Unix timestamp (seconds)
    pub timestamp: u64,
    /// Session ID untuk forward secrecy (8 byte hex)
    pub session_id: String,
    /// Message counter untuk key derivation
    pub msg_counter: u64,
    /// ID pesan yang dibalas (opsional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    /// Teks pesan yang dibalas untuk tampilan quote (opsional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_text: Option<String>,
}

/// Payload untuk Broadcast darurat — TIDAK dienkripsi (plaintext mesh flooding)
/// Ditujukan untuk semua node di jaringan. Gunakan hanya untuk pesan darurat publik.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BroadcastPayload {
    /// Node ID pengirim (32 byte hex)
    pub sender_id: String,
    /// Nama tampilan pengirim
    pub sender_name: String,
    /// Teks pesan darurat
    pub text: String,
    /// Unix timestamp (seconds)
    pub timestamp: u64,
    /// UUID v4 unik — untuk deduplikasi di sisi penerima
    pub message_id: String,
}

// ─── Implementasi ClampPacket ──────────────────────────────────────────────

impl ClampPacket {
    /// Generate Packet ID baru: origin_node_id[0..4] || OsRng[4..8]
    pub fn generate_packet_id(origin_node_id: &[u8; 32]) -> [u8; 8] {
        let mut id = [0u8; 8];
        id[0..4].copy_from_slice(&origin_node_id[0..4]);
        OsRng.fill_bytes(&mut id[4..8]);
        id
    }

    /// Serialize header ke bytes (13 byte tepat).
    /// Digunakan sebagai Associated Data (AAD) untuk Ascon-AEAD128 —
    /// header diautentikasi tapi tidak dienkripsi.
    pub fn header_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..2].copy_from_slice(&self.header.magic);
        buf[2] = self.header.version;
        buf[3] = self.header.packet_type as u8;
        buf[4] = self.header.ttl;
        buf[5..13].copy_from_slice(&self.header.packet_id);
        buf
    }

    /// Encode seluruh paket ke byte stream untuk transmisi via TCP.
    ///
    /// Format output:
    ///   [0..13]   Header (13B)
    ///   [13]      hop_counter (1B)
    ///   [14..30]  mac_tag (16B)
    ///   [30..46]  nonce (16B)
    ///   [46..46+N] ciphertext (NB)
    ///   [46+N..] aead_tag (16B)
    pub fn encode(&self) -> Vec<u8> {
        let total = FIXED_OVERHEAD + self.ciphertext.len();
        let mut buf = Vec::with_capacity(total);
        // Header (13B)
        buf.extend_from_slice(&self.header_bytes());
        // HopAuth (17B)
        buf.push(self.hop_auth.hop_counter);
        buf.extend_from_slice(&self.hop_auth.mac_tag);
        // Nonce (16B)
        buf.extend_from_slice(&self.nonce);
        // Ciphertext (variabel)
        buf.extend_from_slice(&self.ciphertext);
        // AEAD Tag (16B)
        buf.extend_from_slice(&self.aead_tag);
        buf
    }

    /// Decode byte stream menjadi ClampPacket.
    ///
    /// Validasi yang dilakukan:
    ///   1. Cek magic bytes
    ///   2. Cek protocol version
    ///   3. Cek minimum length
    ///   4. Parse packet type (gagal jika tidak dikenal)
    pub fn decode(bytes: &[u8]) -> Result<Self, PacketError> {
        // Minimal length check
        if bytes.len() < FIXED_OVERHEAD {
            return Err(PacketError::TooShort {
                expected: FIXED_OVERHEAD,
                got: bytes.len(),
            });
        }

        // Magic bytes validation
        if bytes[0..2] != MAGIC {
            return Err(PacketError::InvalidMagic);
        }

        // Version validation
        let version = bytes[2];
        if version != PROTOCOL_VERSION {
            return Err(PacketError::UnsupportedVersion(version));
        }

        // Parse header fields
        let packet_type = PacketType::try_from(bytes[3])?;
        let ttl = bytes[4];
        let mut packet_id = [0u8; 8];
        packet_id.copy_from_slice(&bytes[5..13]);

        // Parse HopAuth
        let hop_counter = bytes[13];
        let mut mac_tag = [0u8; 16];
        mac_tag.copy_from_slice(&bytes[14..30]);

        // Parse nonce
        let mut nonce = [0u8; 16];
        nonce.copy_from_slice(&bytes[30..46]);

        // Parse ciphertext dan AEAD tag
        // Cek bahwa ada setidaknya 16 byte untuk tag setelah nonce
        let total = bytes.len();
        if total < 46 + TAG_SIZE {
            return Err(PacketError::TooShort {
                expected: 46 + TAG_SIZE,
                got: total,
            });
        }
        let tag_start = total - TAG_SIZE;
        let ciphertext = bytes[46..tag_start].to_vec();
        let mut aead_tag = [0u8; 16];
        aead_tag.copy_from_slice(&bytes[tag_start..total]);

        Ok(ClampPacket {
            header: ClampHeader {
                magic: MAGIC,
                version,
                packet_type,
                ttl,
                packet_id,
            },
            hop_auth: HopAuth { hop_counter, mac_tag },
            nonce,
            ciphertext,
            aead_tag,
        })
    }

    /// Buat paket Hello untuk handshake TCP awal.
    /// Payload berisi JSON: {"nodeId": "...", "displayName": "..."}
    pub fn build_hello(
        my_node_id: &[u8; 32],
        display_name: &str,
    ) -> Self {
        let payload = serde_json::json!({
            "nodeId": hex::encode(my_node_id),
            "displayName": display_name
        })
        .to_string();

        let packet_id = Self::generate_packet_id(my_node_id);
        ClampPacket {
            header: ClampHeader {
                magic: MAGIC,
                version: PROTOCOL_VERSION,
                packet_type: PacketType::Hello,
                ttl: 1, // Hello tidak perlu di-relay
                packet_id,
            },
            hop_auth: HopAuth {
                hop_counter: 0,
                mac_tag: [0u8; 16], // Hello tidak pakai MAC (plaintext handshake)
            },
            nonce: [0u8; 16],
            ciphertext: payload.into_bytes(),
            aead_tag: [0u8; 16],
        }
    }

    /// Buat paket Broadcast untuk pesan darurat mesh.
    ///
    /// Payload berisi JSON BroadcastPayload tanpa enkripsi.
    /// TTL = 5 agar bisa menjangkau node jauh tapi tidak menyebabkan broadcast storm.
    pub fn build_broadcast(
        my_node_id: &[u8; 32],
        payload: &BroadcastPayload,
    ) -> Self {
        let payload_bytes = serde_json::to_vec(payload).unwrap_or_default();
        let packet_id = Self::generate_packet_id(my_node_id);
        ClampPacket {
            header: ClampHeader {
                magic: MAGIC,
                version: PROTOCOL_VERSION,
                packet_type: PacketType::Broadcast,
                ttl: 5, // Maks 5 lompatan — cukup untuk jaringan mesh kecil-menengah
                packet_id,
            },
            hop_auth: HopAuth {
                hop_counter: 0,
                mac_tag: [0u8; 16], // Broadcast tidak pakai Hop-MAC
            },
            nonce: [0u8; 16],
            ciphertext: payload_bytes,
            aead_tag: [0u8; 16],
        }
    }
}

// ─── Unit Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_packet() -> ClampPacket {
        let node_id = [1u8; 32];
        let packet_id = ClampPacket::generate_packet_id(&node_id);
        ClampPacket {
            header: ClampHeader {
                magic: MAGIC,
                version: PROTOCOL_VERSION,
                packet_type: PacketType::Dm,
                ttl: TTL_MAX,
                packet_id,
            },
            hop_auth: HopAuth {
                hop_counter: 0,
                mac_tag: [0xABu8; 16],
            },
            nonce: [0x12u8; 16],
            ciphertext: b"Hello CARAKA encrypted payload".to_vec(),
            aead_tag: [0xCDu8; 16],
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = make_test_packet();
        let encoded = original.encode();
        let decoded = ClampPacket::decode(&encoded).expect("decode gagal");

        assert_eq!(decoded.header.magic, MAGIC);
        assert_eq!(decoded.header.version, PROTOCOL_VERSION);
        assert_eq!(decoded.header.packet_type, PacketType::Dm);
        assert_eq!(decoded.header.ttl, TTL_MAX);
        assert_eq!(decoded.header.packet_id, original.header.packet_id);
        assert_eq!(decoded.hop_auth.hop_counter, 0);
        assert_eq!(decoded.hop_auth.mac_tag, original.hop_auth.mac_tag);
        assert_eq!(decoded.nonce, original.nonce);
        assert_eq!(decoded.ciphertext, original.ciphertext);
        assert_eq!(decoded.aead_tag, original.aead_tag);
    }

    #[test]
    fn test_encoded_size_correct() {
        let pkt = make_test_packet();
        let encoded = pkt.encode();
        assert_eq!(
            encoded.len(),
            FIXED_OVERHEAD + pkt.ciphertext.len(),
            "Encoded size harus FIXED_OVERHEAD + ciphertext.len()"
        );
    }

    #[test]
    fn test_header_bytes_is_13() {
        let pkt = make_test_packet();
        let hb = pkt.header_bytes();
        assert_eq!(hb.len(), HEADER_SIZE);
        assert_eq!(&hb[0..2], &MAGIC);
        assert_eq!(hb[2], PROTOCOL_VERSION);
        assert_eq!(hb[3], PacketType::Dm as u8);
    }

    #[test]
    fn test_decode_fails_on_invalid_magic() {
        let pkt = make_test_packet();
        let mut encoded = pkt.encode();
        encoded[0] = 0xFF; // Corrupt magic
        assert!(matches!(ClampPacket::decode(&encoded), Err(PacketError::InvalidMagic)));
    }

    #[test]
    fn test_decode_fails_on_wrong_version() {
        let pkt = make_test_packet();
        let mut encoded = pkt.encode();
        encoded[2] = 0xFF; // Corrupt version
        assert!(matches!(
            ClampPacket::decode(&encoded),
            Err(PacketError::UnsupportedVersion(0xFF))
        ));
    }

    #[test]
    fn test_decode_fails_on_too_short() {
        let short = vec![0u8; 10];
        assert!(matches!(
            ClampPacket::decode(&short),
            Err(PacketError::TooShort { .. })
        ));
    }

    #[test]
    fn test_decode_fails_on_unknown_packet_type() {
        let pkt = make_test_packet();
        let mut encoded = pkt.encode();
        encoded[3] = 0xFF; // Invalid packet type
        assert!(matches!(
            ClampPacket::decode(&encoded),
            Err(PacketError::UnknownPacketType(0xFF))
        ));
    }

    #[test]
    fn test_packet_id_has_origin_prefix() {
        let node_id = [0xABu8; 32];
        let packet_id = ClampPacket::generate_packet_id(&node_id);
        // Prefix 4 byte pertama harus sama dengan node_id[0..4]
        assert_eq!(&packet_id[0..4], &node_id[0..4]);
    }

    #[test]
    fn test_all_packet_types_roundtrip() {
        let types = [
            PacketType::Dm,
            PacketType::Channel,
            PacketType::SyncReq,
            PacketType::SyncResp,
            PacketType::Hello,
            PacketType::SyncData,
        ];
        for pt in types {
            let v = pt as u8;
            let decoded = PacketType::try_from(v).unwrap();
            assert_eq!(decoded as u8, v);
        }
    }
}
