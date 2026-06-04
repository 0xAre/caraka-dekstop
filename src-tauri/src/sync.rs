// src-tauri/src/sync.rs
// Fase 5 — Epidemic Sync: Sinkronisasi pesan antar node yang reconnect
//
// Prinsip:
//   - Dua node bertukar VECTOR of fingerprints (hash dari ciphertext)
//   - Bukan konten pesan — node relay tidak bisa membaca isi
//   - Node yang punya pesan yang tidak dimiliki peer, mengirimkan ciphertext



// ─── Fingerprint Vector ────────────────────────────────────────────────────

/// Hitung set perbedaan: fingerprints yang ada di local tapi tidak di peer_fps.
/// Return: list fingerprint yang dimiliki kita tapi peer tidak punya.
pub fn compute_missing_for_peer(
    local_fps: &[[u8; 16]],
    peer_fps: &[[u8; 16]],
) -> Vec<[u8; 16]> {
    let peer_set: std::collections::HashSet<[u8; 16]> =
        peer_fps.iter().copied().collect();

    local_fps
        .iter()
        .filter(|fp| !peer_set.contains(*fp))
        .copied()
        .collect()
}

/// Hitung fingerprint dari ciphertext untuk epidemic sync.
pub fn compute_fingerprint(ciphertext: &[u8]) -> [u8; 16] {
    let hash = crate::crypto::hash256(ciphertext);
    let mut fp = [0u8; 16];
    fp.copy_from_slice(&hash[0..16]);
    fp
}

// ─── Sync Request/Response Builder ────────────────────────────────────────

/// Build payload untuk SYNC_REQ packet.
///
/// Format:
///   [4 byte LE] count of fingerprints
///   [16 byte × count] fingerprints
pub fn build_sync_req_payload(
    my_node_id: &[u8; 32],
    local_fps: &[[u8; 16]],
) -> Vec<u8> {
    let mut payload = Vec::new();
    // Header: node_id untuk identifikasi
    payload.extend_from_slice(my_node_id);                        // 32 byte
    payload.extend_from_slice(&(local_fps.len() as u32).to_le_bytes()); // 4 byte
    for fp in local_fps {
        payload.extend_from_slice(fp);                             // 16 byte each
    }
    payload
}

/// Parse payload SYNC_REQ menjadi (node_id, fingerprints).
pub fn parse_sync_req_payload(data: &[u8]) -> Option<([u8; 32], Vec<[u8; 16]>)> {
    if data.len() < 36 {
        return None;
    }

    let mut node_id = [0u8; 32];
    node_id.copy_from_slice(&data[0..32]);

    let count = u32::from_le_bytes(data[32..36].try_into().ok()?) as usize;

    if data.len() < 36 + count * 16 {
        return None;
    }

    let mut fps = Vec::with_capacity(count);
    for i in 0..count {
        let start = 36 + i * 16;
        let mut fp = [0u8; 16];
        fp.copy_from_slice(&data[start..start + 16]);
        fps.push(fp);
    }

    Some((node_id, fps))
}

// ─── Unit Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_missing_for_peer() {
        let fp1 = [1u8; 16];
        let fp2 = [2u8; 16];
        let fp3 = [3u8; 16];

        let local = vec![fp1, fp2, fp3];
        let peer  = vec![fp1, fp2];

        let missing = compute_missing_for_peer(&local, &peer);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0], fp3);
    }

    #[test]
    fn test_compute_missing_empty() {
        let fp1 = [1u8; 16];
        let local = vec![fp1];
        let peer  = vec![fp1];

        let missing = compute_missing_for_peer(&local, &peer);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_sync_req_roundtrip() {
        let node_id = [0xABu8; 32];
        let fps = vec![[1u8; 16], [2u8; 16], [3u8; 16]];

        let payload = build_sync_req_payload(&node_id, &fps);
        let (parsed_id, parsed_fps) = parse_sync_req_payload(&payload).unwrap();

        assert_eq!(parsed_id, node_id);
        assert_eq!(parsed_fps.len(), 3);
        assert_eq!(parsed_fps[0], [1u8; 16]);
        assert_eq!(parsed_fps[2], [3u8; 16]);
    }

    #[test]
    fn test_fingerprint_consistency() {
        let data = b"test encrypted payload";
        let fp1 = compute_fingerprint(data);
        let fp2 = compute_fingerprint(data);
        assert_eq!(fp1, fp2, "Fingerprint harus deterministik");
    }

    #[test]
    fn test_different_payloads_different_fingerprints() {
        let fp1 = compute_fingerprint(b"payload one");
        let fp2 = compute_fingerprint(b"payload two");
        assert_ne!(fp1, fp2);
    }
}
