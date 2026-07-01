// src-tauri/src/vault.rs
//
// Argon2id-protected vault untuk penyimpanan private key CARAKA Desktop.
//
// Format vault file (80 bytes total):
//   [0..16]  — Argon2id salt (16 byte, random)
//   [16..32] — Ascon nonce  (16 byte, random)
//   [32..80] — Ascon-AEAD128 ciphertext + tag (32 byte key + 16 byte tag)
//
// KDF: Argon2id (m=32768 KiB, t=2, p=1) → 16-byte Ascon key
// AEAD: Ascon-AEAD128 dengan AAD = VAULT_MAGIC

use argon2::{Argon2, Algorithm, Params, Version};
use ascon_aead::{
    Ascon128,
    aead::{Aead, KeyInit, Payload},
};
use rand::{rngs::OsRng, RngCore};
use std::path::{Path, PathBuf};

const VAULT_MAGIC: &[u8] = b"CARAKA-VAULT-V1\x00";
const ARGON2_M_COST: u32 = 32768; // 32 MiB
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;
const VAULT_SIZE: usize = 80; // 16 salt + 16 nonce + 32 ct + 16 tag

pub fn vault_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("vault.key")
}

pub fn vault_exists(app_data_dir: &Path) -> bool {
    vault_path(app_data_dir).exists()
}

/// Derive 16-byte Ascon key dari passphrase + salt menggunakan Argon2id.
fn derive_key(passphrase: &str, salt: &[u8; 16]) -> Result<[u8; 16], String> {
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(16))
        .map_err(|e| format!("Argon2 params error: {}", e))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 16];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Argon2 KDF error: {}", e))?;
    Ok(key)
}

/// Buat vault baru, enkripsi private_key_bytes dengan passphrase.
///
/// Dipanggil saat first run (buat password baru) atau saat migrate dari keyring.
pub fn create_vault(
    app_data_dir: &Path,
    passphrase: &str,
    private_key_bytes: &[u8; 32],
) -> Result<(), String> {
    // 1. Random salt
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);

    // 2. Random nonce (tidak pakai timestamp — vault bukan stream cipher)
    let mut nonce_bytes = [0u8; 16];
    OsRng.fill_bytes(&mut nonce_bytes);

    // 3. Derive key
    let key_bytes = derive_key(passphrase, &salt)?;

    // 4. Enkripsi dengan Ascon-AEAD128
    let ascon_key = ascon_aead::Key::<Ascon128>::from_slice(&key_bytes);
    let ascon_nonce = ascon_aead::Nonce::<Ascon128>::from_slice(&nonce_bytes);
    let cipher = Ascon128::new(ascon_key);
    let ct_with_tag = cipher
        .encrypt(
            ascon_nonce,
            Payload {
                msg: private_key_bytes,
                aad: VAULT_MAGIC,
            },
        )
        .map_err(|_| "Gagal enkripsi vault".to_string())?;

    // 5. Tulis vault file: [salt][nonce][ct+tag]
    let mut data = Vec::with_capacity(VAULT_SIZE);
    data.extend_from_slice(&salt);
    data.extend_from_slice(&nonce_bytes);
    data.extend_from_slice(&ct_with_tag);

    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| format!("Gagal buat direktori data: {}", e))?;
    std::fs::write(vault_path(app_data_dir), &data)
        .map_err(|e| format!("Gagal tulis vault: {}", e))?;

    Ok(())
}

/// Buka vault dengan passphrase, kembalikan private key 32 byte.
///
/// Return Err("Password salah") jika AEAD gagal (tag mismatch).
pub fn unlock_vault(app_data_dir: &Path, passphrase: &str) -> Result<[u8; 32], String> {
    let data = std::fs::read(vault_path(app_data_dir))
        .map_err(|_| "File vault tidak ditemukan. Buat password baru.".to_string())?;

    if data.len() < VAULT_SIZE {
        return Err("File vault rusak atau tidak lengkap.".to_string());
    }

    let salt: [u8; 16] = data[0..16].try_into().unwrap();
    let nonce_bytes: [u8; 16] = data[16..32].try_into().unwrap();
    let ct_with_tag = &data[32..VAULT_SIZE]; // 32 ct + 16 tag = 48 bytes

    let key_bytes = derive_key(passphrase, &salt)?;

    let ascon_key = ascon_aead::Key::<Ascon128>::from_slice(&key_bytes);
    let ascon_nonce = ascon_aead::Nonce::<Ascon128>::from_slice(&nonce_bytes);
    let cipher = Ascon128::new(ascon_key);

    let plaintext = cipher
        .decrypt(
            ascon_nonce,
            Payload {
                msg: ct_with_tag,
                aad: VAULT_MAGIC,
            },
        )
        .map_err(|_| "Password salah.".to_string())?;

    if plaintext.len() != 32 {
        return Err("File vault rusak: ukuran key tidak valid.".to_string());
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&plaintext);
    Ok(key)
}
