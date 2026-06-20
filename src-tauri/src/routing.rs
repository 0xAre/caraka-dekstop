// src-tauri/src/routing.rs
// Fase 3 — Routing Engine: Controlled Flooding + Replay Protection + Trust Score

use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::time::Instant;
use crate::crypto::{MacKey, verify_mac, compute_mac};
use crate::keys::NodePublicKey;
use crate::packet::ClampPacket;

// ─── Konstanta Routing ─────────────────────────────────────────────────────

/// Jumlah Packet ID yang di-cache untuk replay protection
const CACHE_SIZE: usize = 512;

/// Trust Score awal setiap peer baru (neutral)
const TRUST_INITIAL: f32 = 2.0;
/// Trust Score minimum untuk menerima paket dari peer
const TRUST_THRESHOLD: f32 = 0.5;
/// Kenaikan trust per paket valid yang diterima
const TRUST_VALID_DELTA: f32 = 0.01;
/// Penalti trust jika Hop-MAC invalid
const TRUST_INVALID_MAC_DELTA: f32 = -0.5;
/// Penalti trust jika peer melakukan rate-limit violation
pub const TRUST_RATE_LIMIT_DELTA: f32 = -1.0;
/// Batas trust score (min, max)
const TRUST_MIN: f32 = 0.0;
const TRUST_MAX: f32 = 5.0;

// ─── Token Bucket Rate Limiter ──────────────────────────────────────────────

/// Burst capacity: jumlah paket maksimum dalam burst singkat
const RATE_BURST: u32 = 200;
/// Refill rate: token baru per detik
const RATE_PER_SEC: u32 = 100;

/// Token bucket per peer untuk rate limiting.
struct TokenBucket {
    tokens: u32,
    last_refill: Instant,
}

impl TokenBucket {
    fn new() -> Self {
        TokenBucket { tokens: RATE_BURST, last_refill: Instant::now() }
    }

    /// Coba konsumsi 1 token. Return true jika diizinkan, false jika rate limit terlampaui.
    fn consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed_secs = now.duration_since(self.last_refill).as_secs_f32();
        let new_tokens = (elapsed_secs * RATE_PER_SEC as f32) as u32;
        if new_tokens > 0 {
            self.tokens = self.tokens.saturating_add(new_tokens).min(RATE_BURST);
            self.last_refill = now;
        }
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
}

// ─── Keputusan Routing ─────────────────────────────────────────────────────

/// Hasil keputusan routing setelah memproses paket masuk
#[derive(Debug)]
pub enum RoutingDecision {
    /// Kirim ke aplikasi (UI) — mungkin untuk node ini
    DeliverToApp,
    /// Teruskan ke semua peer lain (TTL sudah dikurangi, MAC diperbarui)
    Relay,
    /// Kirim ke app DAN relay (TTL > 0)
    DeliverAndRelay,
    /// Buang paket dengan alasan
    Drop(DropReason),
}

#[derive(Debug)]
pub enum DropReason {
    InvalidMagic,
    DuplicatePacket,
    InvalidHopMac,
    TtlExpired,
    TimestampInvalid,
    PeerUntrusted,
    RateLimitExceeded,
}

// ─── Router ────────────────────────────────────────────────────────────────

/// Routing engine utama CARAKA.
///
/// Diakses dari multiple tokio tasks via Arc<Mutex<Router>>.
pub struct Router {
    /// LRU cache dari 512 Packet ID terakhir (replay protection)
    packet_cache: LruCache<[u8; 8], ()>,
    /// Trust Score setiap peer yang dikenal
    trust_scores: HashMap<NodePublicKey, f32>,
    /// Token bucket per peer untuk rate limiting (SECURITY 3A)
    rate_limiters: HashMap<NodePublicKey, TokenBucket>,
    /// Channel MAC Key — untuk validasi Hop-MAC di setiap relay
    /// Default [0u8; 16] — akan dikonfigurasi user di Fase 6
    pub channel_mac_key: MacKey,
    /// Node ID milik node ini (untuk compute relay MAC)
    pub my_node_id: NodePublicKey,
}

impl Router {
    pub fn new(my_node_id: NodePublicKey) -> Self {
        Router {
            packet_cache: LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap()),
            trust_scores: HashMap::new(),
            rate_limiters: HashMap::new(),
            channel_mac_key: MacKey([0u8; 16]),
            my_node_id,
        }
    }

    /// Set channel MAC key (dipanggil saat user mengkonfigurasi channel)
    pub fn set_channel_key(&mut self, key: MacKey) {
        self.channel_mac_key = key;
    }

    /// Proses paket yang masuk dari peer.
    ///
    /// Pipeline:
    ///   1. Cek trust score sumber
    ///   2. Cek Packet ID di cache (replay protection)
    ///   3. Validasi Hop-MAC
    ///   4. Tambahkan ke cache
    ///   5. Update trust
    ///   6. Tentukan routing decision (deliver / relay / both)
    pub fn handle_incoming(
        &mut self,
        packet: &mut ClampPacket,
        source: &NodePublicKey,
    ) -> RoutingDecision {
        // 0. Rate limit check — token bucket per peer (SECURITY 3A)
        let allowed = self.rate_limiters
            .entry(source.clone())
            .or_insert_with(TokenBucket::new)
            .consume();
        if !allowed {
            self.update_trust(source, TRUST_RATE_LIMIT_DELTA);
            tracing::warn!(
                "Rate limit terlampaui dari peer {:?} — trust diturunkan, paket di-drop",
                &source.0[0..4]
            );
            return RoutingDecision::Drop(DropReason::RateLimitExceeded);
        }

        // 1. Cek trust score
        let trust = self.trust_scores.get(source).copied().unwrap_or(TRUST_INITIAL);
        if trust < TRUST_THRESHOLD {
            tracing::warn!(
                "Paket dari peer tidak dipercaya: {:?} (trust={:.2})",
                &source.0[0..4],
                trust
            );
            return RoutingDecision::Drop(DropReason::PeerUntrusted);
        }

        // 2. Replay protection — cek Packet ID
        if self.packet_cache.contains(&packet.header.packet_id) {
            tracing::debug!("Duplicate packet {:?} — di-drop", &packet.header.packet_id);
            return RoutingDecision::Drop(DropReason::DuplicatePacket);
        }

        // 3. Validasi Hop-MAC
        // Untuk Hello packets, skip MAC verification (plaintext handshake)
        if packet.header.packet_type != crate::packet::PacketType::Hello {
            if !self.verify_hop_mac(packet, source) {
                self.update_trust(source, TRUST_INVALID_MAC_DELTA);
                tracing::warn!(
                    "Hop-MAC invalid dari peer {:?} — trust diturunkan",
                    &source.0[0..4]
                );
                return RoutingDecision::Drop(DropReason::InvalidHopMac);
            }
        }

        // 4. Tambah ke packet cache (setelah validasi)
        self.packet_cache.put(packet.header.packet_id, ());

        // 5. Update trust positif
        self.update_trust(source, TRUST_VALID_DELTA);

        // 6. Tentukan routing decision
        // Selalu coba deliver ke app (mungkin untuk node ini)
        // Jika TTL > 0, juga relay ke peer lain
        let can_relay = packet.header.ttl > 0;

        if can_relay {
            // Update TTL dan Hop-MAC untuk relay
            packet.header.ttl -= 1;
            packet.hop_auth.hop_counter += 1;
            packet.hop_auth.mac_tag = self.compute_relay_mac(packet);

            RoutingDecision::DeliverAndRelay
        } else {
            RoutingDecision::DeliverToApp
        }
    }

    /// Proses paket yang dibuat oleh node ini (untuk broadcast ke semua peer).
    /// Tidak ada replay check — paket baru selalu diterima.
    pub fn register_outgoing(&mut self, packet: &ClampPacket) {
        // Tambahkan packet_id ke cache sehingga tidak diproses kembali
        // jika node menerima kembali paket miliknya sendiri
        self.packet_cache.put(packet.header.packet_id, ());
    }

    /// Cek apakah packet_id sudah pernah diterima (duplicate check untuk Broadcast).
    pub fn is_duplicate(&self, packet_id: &[u8; 8]) -> bool {
        self.packet_cache.contains(packet_id)
    }

    /// Register packet_id Broadcast ke cache tanpa proses routing penuh.
    /// Digunakan untuk paket Broadcast yang tidak perlu validasi Hop-MAC.
    pub fn register_broadcast(&mut self, packet_id: &[u8; 8]) {
        self.packet_cache.put(*packet_id, ());
    }

    // ─── MAC Operations ─────────────────────────────────────────────────

    /// Bangun input data untuk MAC computation:
    ///   packet_id (8B) || hop_counter (1B) || node_id_prefix (4B)
    pub(crate) fn build_mac_input(&self, packet_id: &[u8; 8], hop_counter: u8, node_id: &[u8; 32]) -> Vec<u8> {
        let mut input = Vec::with_capacity(13);
        input.extend_from_slice(packet_id);        // 8 byte
        input.push(hop_counter);                   // 1 byte
        input.extend_from_slice(&node_id[0..4]);   // 4 byte prefix
        input
    }

    /// Verifikasi Hop-MAC dari paket yang diterima.
    ///
    /// MAC di-compute menggunakan:
    ///   key = channel_mac_key
    ///   data = packet_id || hop_counter || source_node_id[0..4]
    fn verify_hop_mac(&self, packet: &ClampPacket, source: &NodePublicKey) -> bool {
        let mac_input = self.build_mac_input(
            &packet.header.packet_id,
            packet.hop_auth.hop_counter,
            &source.0,
        );
        verify_mac(&self.channel_mac_key, &mac_input, &packet.hop_auth.mac_tag)
    }

    /// Compute Hop-MAC baru untuk relay (menggunakan node ID kita sebagai sender).
    fn compute_relay_mac(&self, packet: &ClampPacket) -> [u8; 16] {
        let mac_input = self.build_mac_input(
            &packet.header.packet_id,
            packet.hop_auth.hop_counter,
            &self.my_node_id.0,
        );
        compute_mac(&self.channel_mac_key, &mac_input)
    }

    /// Compute initial Hop-MAC untuk paket yang dibuat node ini (origin).
    pub fn compute_origin_mac(&self, packet: &ClampPacket) -> [u8; 16] {
        let mac_input = self.build_mac_input(
            &packet.header.packet_id,
            0, // hop_counter = 0 saat origin
            &self.my_node_id.0,
        );
        compute_mac(&self.channel_mac_key, &mac_input)
    }

    // ─── Trust Management ──────────────────────────────────────────────

    /// Update trust score peer dengan delta, clamp ke [TRUST_MIN, TRUST_MAX].
    pub fn update_trust(&mut self, peer: &NodePublicKey, delta: f32) {
        let score = self.trust_scores
            .entry(peer.clone())
            .or_insert(TRUST_INITIAL);
        *score = (*score + delta).clamp(TRUST_MIN, TRUST_MAX);
    }

    /// Ambil trust score peer (default: TRUST_INITIAL jika belum dikenal).
    pub fn get_trust(&self, peer: &NodePublicKey) -> f32 {
        self.trust_scores.get(peer).copied().unwrap_or(TRUST_INITIAL)
    }

    /// Tambahkan peer baru dengan trust default.
    pub fn register_peer(&mut self, peer: &NodePublicKey) {
        self.trust_scores.entry(peer.clone()).or_insert(TRUST_INITIAL);
    }

    /// Ambil semua peer yang dikenal beserta trust score-nya.
    pub fn get_all_peers(&self) -> Vec<(NodePublicKey, f32)> {
        self.trust_scores
            .iter()
            .map(|(k, &v)| (k.clone(), v))
            .collect()
    }
}

// ─── Unit Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{generate_keypair, NodePublicKey};
    use crate::packet::{ClampPacket, ClampHeader, HopAuth, PacketType, MAGIC, PROTOCOL_VERSION, TTL_MAX};

    fn make_router() -> (Router, NodePublicKey) {
        let (_, pub_key) = generate_keypair();
        let router = Router::new(pub_key.clone());
        (router, pub_key)
    }

    /// Buat packet valid yang seolah-olah dikirim oleh `sender_id`.
    /// MAC dihitung menggunakan sender_id sebagai node_id input (bukan my_node_id router).
    fn make_packet_from_peer(router: &Router, sender_id: &NodePublicKey) -> ClampPacket {
        let packet_id = ClampPacket::generate_packet_id(&sender_id.0);
        let mut pkt = ClampPacket {
            header: ClampHeader {
                magic: MAGIC,
                version: PROTOCOL_VERSION,
                packet_type: PacketType::Dm,
                ttl: TTL_MAX,
                packet_id,
            },
            hop_auth: HopAuth {
                hop_counter: 0,
                mac_tag: [0u8; 16],
            },
            nonce: [0u8; 16],
            ciphertext: b"test payload".to_vec(),
            aead_tag: [0u8; 16],
        };

        // Compute MAC menggunakan sender_id (seperti yang dilakukan pengirim)
        let mac_input = router.build_mac_input(&packet_id, 0, &sender_id.0);
        pkt.hop_auth.mac_tag = compute_mac(&router.channel_mac_key, &mac_input);
        pkt
    }

    fn make_packet_with_router(router: &Router) -> ClampPacket {
        let node_id = router.my_node_id.0;
        let packet_id = ClampPacket::generate_packet_id(&node_id);
        let mut pkt = ClampPacket {
            header: ClampHeader {
                magic: MAGIC,
                version: PROTOCOL_VERSION,
                packet_type: PacketType::Dm,
                ttl: TTL_MAX,
                packet_id,
            },
            hop_auth: HopAuth {
                hop_counter: 0,
                mac_tag: [0u8; 16], // akan diisi
            },
            nonce: [0u8; 16],
            ciphertext: b"test payload".to_vec(),
            aead_tag: [0u8; 16],
        };
        // Compute valid MAC
        pkt.hop_auth.mac_tag = router.compute_origin_mac(&pkt);
        pkt
    }

    #[test]
    fn test_duplicate_packet_dropped() {
        let (mut router, _my_id) = make_router();
        let (_, peer_id) = generate_keypair();

        let mut pkt = make_packet_with_router(&router);

        // Register outgoing (simulate kita yang buat)
        router.register_outgoing(&pkt);

        // Coba proses paket dengan ID sama — harus drop sebagai duplicate
        let result = router.handle_incoming(&mut pkt, &peer_id);
        assert!(matches!(result, RoutingDecision::Drop(DropReason::DuplicatePacket)));
    }

    #[test]
    fn test_invalid_hop_mac_drops_and_penalizes_trust() {
        let (mut router, _) = make_router();
        let (_, peer_id) = generate_keypair();

        let mut pkt = make_packet_from_peer(&router, &peer_id);
        // Corrupt MAC
        pkt.hop_auth.mac_tag[0] ^= 0xFF;

        let trust_before = router.get_trust(&peer_id);
        let result = router.handle_incoming(&mut pkt, &peer_id);

        assert!(matches!(result, RoutingDecision::Drop(DropReason::InvalidHopMac)));
        // Trust harus turun
        let trust_after = router.get_trust(&peer_id);
        assert!(trust_after < trust_before);
    }

    #[test]
    fn test_ttl_decremented_on_relay() {
        let (mut router, _) = make_router();
        let (_, peer_id) = generate_keypair();

        // Buat packet yang valid dari peer_id
        let mut pkt = make_packet_from_peer(&router, &peer_id);
        let original_ttl = pkt.header.ttl;

        let result = router.handle_incoming(&mut pkt, &peer_id);

        // Harus relay (TTL > 0)
        assert!(matches!(result, RoutingDecision::DeliverAndRelay));
        assert_eq!(pkt.header.ttl, original_ttl - 1);
    }

    #[test]
    fn test_hop_counter_incremented_on_relay() {
        let (mut router, _) = make_router();
        let (_, peer_id) = generate_keypair();

        let mut pkt = make_packet_from_peer(&router, &peer_id);
        let result = router.handle_incoming(&mut pkt, &peer_id);

        assert!(matches!(result, RoutingDecision::DeliverAndRelay));
        assert_eq!(pkt.hop_auth.hop_counter, 1);
    }

    #[test]
    fn test_ttl_zero_delivers_but_no_relay() {
        let (mut router, _) = make_router();
        let (_, peer_id) = generate_keypair();

        let mut pkt = make_packet_from_peer(&router, &peer_id);
        pkt.header.ttl = 0; // TTL habis

        // Recompute MAC karena TTL tidak mempengaruhi MAC (MAC dihitung dari packet_id, hop_counter, node_id)
        // MAC tetap valid karena input MAC tidak bergantung TTL
        let mac_input = router.build_mac_input(&pkt.header.packet_id, 0, &peer_id.0);
        pkt.hop_auth.mac_tag = compute_mac(&router.channel_mac_key, &mac_input);

        let result = router.handle_incoming(&mut pkt, &peer_id);
        assert!(matches!(result, RoutingDecision::DeliverToApp));
    }

    #[test]
    fn test_trust_score_clamped() {
        let (mut router, _) = make_router();
        let (_, peer_id) = generate_keypair();

        // Update trust banyak kali
        for _ in 0..1000 {
            router.update_trust(&peer_id, 1.0);
        }
        assert_eq!(router.get_trust(&peer_id), TRUST_MAX);

        for _ in 0..1000 {
            router.update_trust(&peer_id, -1.0);
        }
        assert_eq!(router.get_trust(&peer_id), TRUST_MIN);
    }

    #[test]
    fn test_untrusted_peer_dropped() {
        let (mut router, _) = make_router();
        let (_, peer_id) = generate_keypair();

        // Set trust sangat rendah
        router.trust_scores.insert(peer_id.clone(), 0.1);

        let mut pkt = make_packet_from_peer(&router, &peer_id);
        let result = router.handle_incoming(&mut pkt, &peer_id);
        assert!(matches!(result, RoutingDecision::Drop(DropReason::PeerUntrusted)));
    }
}
