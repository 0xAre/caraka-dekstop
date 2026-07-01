// src-tauri/src/file_transfer.rs
// F2 — File Transfer helpers: baca file dari disk, deteksi MIME, simpan file diterima.

use std::path::{Path, PathBuf};
use tauri::Manager;

/// Batas ukuran file yang boleh dikirim: 5 MB
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Deteksi MIME type sederhana berdasarkan ekstensi file.
pub fn mime_from_filename(filename: &str) -> &'static str {
    let ext = filename
        .rsplit('.')
        .next()
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png"          => "image/png",
        "gif"          => "image/gif",
        "webp"         => "image/webp",
        "svg"          => "image/svg+xml",
        "bmp"          => "image/bmp",
        "pdf"          => "application/pdf",
        "txt"          => "text/plain",
        "md"           => "text/markdown",
        "json"         => "application/json",
        "zip"          => "application/zip",
        "gz"           => "application/gzip",
        "mp3"          => "audio/mpeg",
        "mp4"          => "video/mp4",
        "wav"          => "audio/wav",
        "doc" | "docx" => "application/msword",
        _              => "application/octet-stream",
    }
}

/// Return true jika MIME adalah gambar yang bisa dipreview browser.
pub fn is_previewable_image(mime: &str) -> bool {
    matches!(
        mime,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/svg+xml" | "image/bmp"
    )
}

/// Baca file dari path, return (bytes, filename, mime_type).
/// Gagal jika ukuran melebihi MAX_FILE_BYTES.
pub fn read_file_for_transfer(path_str: &str) -> Result<(Vec<u8>, String, String), String> {
    let path = Path::new(path_str);

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Gagal baca metadata file: {e}"))?;

    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "File terlalu besar ({} MB). Maksimum {} MB.",
            metadata.len() / (1024 * 1024),
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }

    let bytes = std::fs::read(path)
        .map_err(|e| format!("Gagal baca file: {e}"))?;

    let mime = mime_from_filename(&filename).to_string();

    Ok((bytes, filename, mime))
}

/// Simpan file yang diterima ke folder Downloads/CARAKA/.
/// Return path file yang disimpan.
pub fn save_received_file(
    app_handle: &tauri::AppHandle,
    filename: &str,
    data: &[u8],
) -> Result<PathBuf, String> {
    let save_dir = get_save_dir(app_handle);

    // Sanitize filename: hapus path traversal characters
    let safe_name: String = filename
        .chars()
        .filter(|&c| c != '/' && c != '\\' && c != '\0')
        .collect();
    let safe_name = if safe_name.is_empty() { "file".to_string() } else { safe_name };

    // Jika sudah ada, tambahkan suffix unik agar tidak overwrite
    let mut dest = save_dir.join(&safe_name);
    if dest.exists() {
        let stem = Path::new(&safe_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = Path::new(&safe_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        dest = save_dir.join(format!("{stem}_{ts}{ext}"));
    }

    std::fs::write(&dest, data)
        .map_err(|e| format!("Gagal simpan file: {e}"))?;

    Ok(dest)
}

/// Dapatkan direktori penyimpanan file yang diterima.
pub fn get_save_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    let base = app_handle
        .path()
        .home_dir()
        .map(|h| h.join("Downloads").join("CARAKA"))
        .unwrap_or_else(|_| PathBuf::from("CARAKA-Downloads"));

    let _ = std::fs::create_dir_all(&base);
    base
}
