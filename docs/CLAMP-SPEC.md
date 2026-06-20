# CLAMP Protocol Specification

**CLAMP — Compact Lightweight Authenticated Mesh Protocol**  
Version: 1.0 (Protocol Version Byte: `0x01`)  
Status: Production  

---

## 1. Overview

CLAMP adalah protokol pesan P2P berbasis mesh yang dirancang untuk komunikasi darurat
saat infrastruktur internet tidak tersedia. Setiap node adalah relay sekaligus endpoint.

**Design Goals:**
- Minimal overhead: 62-byte fixed header per paket
- E2EE setiap DM: Ascon-128 + X25519 ECDH
- Relay tanpa kepercayaan: setiap hop diautentikasi via HopMAC
- Replay protection: LRU cache 512 packet ID + nonce window ±300 detik
- Forward secrecy: setiap pesan pakai counter-derived key via HKDF

---

## 2. Packet Format

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Magic[0]    |   Magic[1]    |   Version     |  Packet Type  |  Header
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|      TTL      |          Packet ID (8 bytes)                  |
+-+-+-+-+-+-+-+-+                                               +
|                        Packet ID (cont.)                      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Hop Counter  |           MAC Tag (16 bytes)                  |  HopAuth
+-+-+-+-+-+-+-+-+                                               +
|                         MAC Tag (cont.)                       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         Nonce (16 bytes)                      |  Nonce
+                                                               +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Ciphertext (variable)                      |  Payload
|                        ...                                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       AEAD Tag (16 bytes)                     |  Auth Tag
+                                                               +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 2.1 Field Sizes

| Field        | Size   | Notes                                      |
|--------------|--------|--------------------------------------------|
| Magic        | 2 B    | `0xCA 0x52` ("CAR" — CARAKA identifier)    |
| Version      | 1 B    | `0x01` saat ini                            |
| Packet Type  | 1 B    | Lihat §2.2                                 |
| TTL          | 1 B    | 0–7, dikurangi setiap relay                |
| Packet ID    | 8 B    | `origin[0..4] || OsRng[4..8]`             |
| Hop Counter  | 1 B    | 0 di origin, +1 setiap relay               |
| MAC Tag      | 16 B   | Ascon-MAC (lihat §4)                       |
| Nonce        | 16 B   | Random per pesan                           |
| Ciphertext   | N B    | Ascon-AEAD128 encrypted payload            |
| AEAD Tag     | 16 B   | Authentication tag dari Ascon-AEAD128      |
| **TOTAL**    | **62+N B** | Fixed overhead = 62 byte               |

### 2.2 Packet Types

| Value | Name       | Description                          | E2EE |
|-------|------------|--------------------------------------|------|
| `0x01` | DM        | Direct Message antar dua node        | Ya   |
| `0x02` | Channel   | Channel group message                | Ya   |
| `0x03` | SyncReq   | Epidemic sync: minta fingerprint list | Tidak |
| `0x04` | SyncResp  | Epidemic sync: kirim fingerprint list | Tidak |
| `0x05` | Hello     | Handshake TCP awal                   | Tidak |
| `0x06` | SyncData  | Epidemic sync: transfer pesan        | Tidak |
| `0x07` | Broadcast | Pesan darurat publik (mesh flooding) | Tidak |

---

## 3. Cryptographic Primitives

### 3.1 Enkripsi E2EE (DM)

Menggunakan **Ascon-AEAD128** (NIST SP 800-232, ISO/IEC 29192-6).

```
Encrypt(key, nonce, plaintext, aad) → (ciphertext, tag)
Decrypt(key, nonce, ciphertext, tag, aad) → plaintext | FAIL
```

- **Key**: 128-bit (16 byte), derived via HKDF
- **Nonce**: 128-bit (16 byte), random per pesan (OsRng)
- **AAD** (Associated Data): 13-byte CLAMP header (diautentikasi, tidak dienkripsi)

**PENTING**: AAD = header bytes ASLI saat enkripsi. Untuk dekripsi, AAD harus direkonstruksi
dari `[Magic(2B) || Version(1B) || PacketType::Dm(1B) || TTL_MAX(1B) || PacketID(8B)]`.
TTL dihardcode ke TTL_MAX (7) karena TTL berubah saat relay tetapi enkripsi terjadi di origin.

### 3.2 Key Exchange (X25519 ECDH)

```
shared_secret = X25519(my_private_key, peer_public_key)
```

Properti: X25519(a, B) == X25519(b, A) — commutative.  
Kedua sisi (pengirim dan penerima) menghasilkan shared_secret yang sama.

### 3.3 DM Key Derivation (HKDF-SHA256)

```
dm_key = HKDF-SHA256(
    ikm  = ECDH_shared_secret,
    salt = b"CARAKA-DM-v1",
    info = canonical_sender_id || canonical_receiver_id || session_id || msg_counter_le64,
    len  = 16
)
```

**Canonical ordering**: `sender_id` dan `receiver_id` ditentukan berdasarkan leksikografis
dari public key bytes agar konsisten di kedua sisi:
- Jika `my_pub_key < peer_pub_key`: sender = aku, receiver = peer
- Jika `my_pub_key > peer_pub_key`: sender = peer, receiver = aku

`session_id`: 8-byte random, disimpan di SQLite `sessions` table, unik per pasang peer.
`msg_counter`: u64 monotonically increasing, increment setiap DM terkirim.

### 3.4 HopMAC (Ascon-MAC)

```
mac_input = packet_id(8B) || hop_counter(1B) || relay_node_id[0..4](4B)
mac_tag   = Ascon-MAC(hop_mac_key, mac_input)
```

`hop_mac_key` di-derive dari private key via HKDF:
```
hop_mac_key = HKDF-SHA256(
    ikm  = my_private_key,
    salt = b"CARAKA-HOP-MAC-v1",
    info = b"hop-authentication",
    len  = 16
)
```

---

## 4. Routing & Relay

### 4.1 Controlled Flooding

CLAMP menggunakan controlled flooding dengan TTL untuk mencapai semua node dalam mesh:

1. Origin node set `TTL = TTL_MAX = 7`
2. Setiap relay node: `TTL -= 1`, `hop_counter += 1`
3. Jika `TTL == 0`: deliver ke app, jangan relay
4. Jika `TTL > 0`: deliver ke app DAN relay ke semua peer lain

### 4.2 Replay Protection

Setiap node menyimpan LRU cache dari 512 Packet ID terbaru.  
Paket dengan Packet ID yang sudah ada di cache akan di-drop (`DuplicatePacket`).

### 4.3 Trust Score

Setiap peer memiliki trust score `[0.0, 5.0]`:
- Initial: `2.0`
- Per valid packet: `+0.01`
- Per invalid HopMAC: `-0.5`
- Per rate limit violation: `-1.0`
- Threshold: paket dari peer dengan score `< 0.5` di-drop

### 4.4 Rate Limiting (Token Bucket)

Per peer: 200 token burst, 100 token/detik refill.  
Pelanggaran menurunkan trust score dan paket di-drop.

---

## 5. Transport Layer

CLAMP berjalan di atas TCP dengan framing 2-byte length prefix:

```
[2 byte LE length] [packet bytes]
```

- Port default: **7771** (TCP)
- Discovery: **7770** (UDP broadcast, subnet-scoped)
- Max packet size: 65535 byte
- Handshake: Hello packet pertama kali setelah TCP connect

### 5.1 Hello Handshake

```json
{"nodeId": "<hex 64 char>", "displayName": "<string>"}
```

Plaintext JSON (tidak dienkripsi). HopMAC = `[0u8; 16]` (tidak diautentikasi).
TTL = 1 (tidak perlu di-relay ke peer lain).

---

## 6. Epidemic Sync (Gap Filling)

Untuk memastikan pesan tersampaikan bahkan saat koneksi terputus-putus:

1. **SyncReq**: Node A kirim SyncReq ke Node B
2. **SyncResp**: Node B balas dengan list packet_id yang dimiliki
3. Node A bandingkan dengan database lokal, identifikasi yang hilang
4. Node A kirim event `sync_missing_detected` ke frontend
5. Frontend request SyncData untuk setiap pesan yang hilang

---

## 7. Inner DM Payload (JSON, dienkripsi)

```json
{
  "sender_id":   "<hex 64 char>",
  "recipient_id": "<hex 64 char>",
  "text":        "<plaintext pesan>",
  "timestamp":   1234567890,
  "session_id":  "<hex 16 char>",
  "msg_counter": 42
}
```

---

## 8. Broadcast Payload (JSON, plaintext)

```json
{
  "sender_id":   "<hex 64 char>",
  "sender_name": "<display name>",
  "text":        "[EVAC] Instruksi evakuasi...",
  "timestamp":   1234567890,
  "message_id":  "<uuid v4>"
}
```

Emergency type prefix (opsional): `[EVAC]`, `[STATUS]`, `[RESOURCE]`

---

## 9. Implementation Notes

### Backward Compatibility

- Versi protokol di header (`0x01`). Node dengan versi berbeda di-drop.
- CLAMP packet format changes HARUS backward-compatible.
- Penambahan field di inner JSON payload OK (dienkripsi, konsumen bisa ignore unknown fields).
- Penambahan Packet Type baru OK jika node lama akan skip (drop gracefully).

### Security Considerations

1. **Private key** disimpan di Windows Credential Manager (tidak di disk plaintext).
2. **Nonce reuse** dikecualikan karena 128-bit random dari OsRng (prob collision negligible).
3. **HopMAC** melindungi dari paket injection/replay di relay path.
4. **Trust score** memberikan rate limiting adaptif per peer.
5. **AAD binding** mencegah ciphertext dari satu paket digunakan di konteks paket lain.

---

*Dokumen ini mendeskripsikan implementasi CARAKA Desktop v0.1.0+*  
*Dibuat: 2026-06-21*
