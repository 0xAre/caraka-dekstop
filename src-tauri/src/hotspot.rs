// src-tauri/src/hotspot.rs
// Emergency Mode — Windows Mobile Hotspot Manager
//
// Mengelola Windows Mobile Hotspot menggunakan `netsh wlan` command.
// Hotspot dibuat TANPA PASSWORD (open network) agar koneksi darurat lebih cepat.
//
// SSID : "CARAKA-Emergency"
// Auth : Open (tanpa password) — sengaja untuk kemudahan saat darurat
// Subnet: 192.168.137.x (default Windows Mobile Hotspot)
//
// Catatan Windows:
//   - netsh wlan hostednetwork tersedia di Windows 7/8/10/11
//   - Membutuhkan adapter WiFi yang mendukung "Virtual WiFi"
//   - Harus dijalankan sebagai Administrator untuk start/stop
//   - PowerShell MobileHotspot API (Windows 10+) lebih modern tapi lebih kompleks

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{info, warn, debug};


/// SSID jaringan darurat CARAKA
pub const EMERGENCY_SSID: &str = "CARAKA-Emergency";

/// Subnet Windows Mobile Hotspot (default tidak bisa diubah tanpa registry hack)
pub const HOTSPOT_SUBNET: &str = "192.168.137";

/// Port discovery di Emergency Mode (sama dengan normal)
pub const EMERGENCY_DISCOVERY_BROADCAST: &str = "192.168.137.255";

/// Status hotspot
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotspotStatus {
    pub is_active: bool,
    pub ssid: String,
    pub mode: String,         // "host" atau "client" atau "none"
    pub subnet: String,
    pub client_count: u32,
}

impl Default for HotspotStatus {
    fn default() -> Self {
        HotspotStatus {
            is_active: false,
            ssid: String::new(),
            mode: "none".to_string(),
            subnet: String::new(),
            client_count: 0,
        }
    }
}

/// Aktifkan Windows Mobile Hotspot via netsh.
///
/// Langkah:
///   1. Set hosted network: SSID = CARAKA-Emergency, mode = open (tanpa password)
///   2. Start hosted network
///   3. Return status
///
/// Note: Membutuhkan hak Administrator di Windows.
pub async fn start_emergency_hotspot() -> Result<HotspotStatus, String> {
    info!("Mengaktifkan hotspot darurat: SSID={}", EMERGENCY_SSID);

    // Step 1: Configure hosted network (open, tanpa password)
    let setup = Command::new("netsh")
        .args([
            "wlan", "set", "hostednetwork",
            "mode=allow",
            &format!("ssid={}", EMERGENCY_SSID),
            "key=",           // Kosong = open network tanpa password
            "keyusage=persistent",
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW — jangan tampil cmd window
        .output()
        .await
        .map_err(|e| format!("Gagal jalankan netsh setup: {}", e))?;

    if !setup.status.success() {
        // Coba tanpa key parameter (beberapa versi Windows tidak support key=kosong)
        let setup2 = Command::new("netsh")
            .args([
                "wlan", "set", "hostednetwork",
                "mode=allow",
                &format!("ssid={}", EMERGENCY_SSID),
            ])
            .creation_flags(0x08000000)
            .output()
            .await
            .map_err(|e| format!("Gagal jalankan netsh setup2: {}", e))?;

        let stderr = String::from_utf8_lossy(&setup2.stderr);
        debug!("netsh setup output: {}", stderr);
    }

    // Step 2: Start hosted network
    let start = Command::new("netsh")
        .args(["wlan", "start", "hostednetwork"])
        .creation_flags(0x08000000)
        .output()
        .await
        .map_err(|e| format!("Gagal start hostednetwork: {}", e))?;

    let start_out = String::from_utf8_lossy(&start.stdout);
    let start_err = String::from_utf8_lossy(&start.stderr);
    debug!("netsh start output: {} | err: {}", start_out, start_err);

    if start.status.success() || start_out.contains("started") || start_out.to_lowercase().contains("berhasil") {
        info!("✅ Hotspot darurat '{}' berhasil diaktifkan", EMERGENCY_SSID);
        Ok(HotspotStatus {
            is_active: true,
            ssid: EMERGENCY_SSID.to_string(),
            mode: "host".to_string(),
            subnet: HOTSPOT_SUBNET.to_string(),
            client_count: 0,
        })
    } else {
        // Coba PowerShell method sebagai fallback (Windows 10+)
        start_hotspot_via_powershell().await
    }
}

/// Matikan hotspot darurat.
pub async fn stop_emergency_hotspot() -> Result<(), String> {
    info!("Mematikan hotspot darurat");

    let output = Command::new("netsh")
        .args(["wlan", "stop", "hostednetwork"])
        .creation_flags(0x08000000)
        .output()
        .await
        .map_err(|e| format!("Gagal stop hostednetwork: {}", e))?;

    debug!("netsh stop: {}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

/// Cek status hotspot saat ini.
pub async fn get_hotspot_status() -> HotspotStatus {
    let output = Command::new("netsh")
        .args(["wlan", "show", "hostednetwork"])
        .creation_flags(0x08000000)
        .output()
        .await;

    match output {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            parse_hostednetwork_status(&text)
        }
        Err(_) => HotspotStatus::default(),
    }
}

/// Parse output `netsh wlan show hostednetwork` untuk cek status.
fn parse_hostednetwork_status(output: &str) -> HotspotStatus {
    let lower = output.to_lowercase();
    let is_started = lower.contains("started") || lower.contains("dimulai");

    // Coba extract SSID dari output
    let ssid = output
        .lines()
        .find(|l| l.to_lowercase().contains("ssid"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Coba extract jumlah client
    let client_count = output
        .lines()
        .find(|l| l.to_lowercase().contains("client") || l.to_lowercase().contains("station"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);

    HotspotStatus {
        is_active: is_started,
        ssid: if is_started { ssid } else { String::new() },
        mode: if is_started { "host".to_string() } else { "none".to_string() },
        subnet: if is_started { HOTSPOT_SUBNET.to_string() } else { String::new() },
        client_count,
    }
}

/// Fallback: Aktifkan hotspot via PowerShell (Windows 10+).
///
/// Menggunakan WinRT API via PowerShell untuk Mobile Hotspot yang lebih modern.
async fn start_hotspot_via_powershell() -> Result<HotspotStatus, String> {
    info!("Mencoba aktifkan hotspot via PowerShell (Windows 10+ Mobile Hotspot)");

    // Script PowerShell untuk enable Mobile Hotspot
    let ps_script = r#"
$connectionProfile = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]::GetInternetConnectionProfile()
if ($connectionProfile -eq $null) {
    Write-Output "NO_INTERNET_PROFILE"
    exit 1
}
$tetheringManager = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking.NetworkOperators,ContentType=WindowsRuntime]::CreateFromConnectionProfile($connectionProfile)
$config = $tetheringManager.GetCurrentAccessPointConfiguration()
$config.Ssid = "CARAKA-Emergency"
$config.Passphrase = ""
$tetheringManager.ConfigureAccessPointAsync($config).GetAwaiter().GetResult()
$tetheringManager.StartTetheringAsync().GetAwaiter().GetResult()
Write-Output "HOTSPOT_STARTED"
"#;

    let output = Command::new("powershell")
        .args(["-NonInteractive", "-Command", ps_script])
        .creation_flags(0x08000000)
        .output()
        .await
        .map_err(|e| format!("Gagal jalankan PowerShell: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    debug!("PS hotspot output: {} | err: {}", stdout, stderr);

    if stdout.contains("HOTSPOT_STARTED") {
        info!("✅ Hotspot darurat aktif via PowerShell");
        Ok(HotspotStatus {
            is_active: true,
            ssid: EMERGENCY_SSID.to_string(),
            mode: "host".to_string(),
            subnet: HOTSPOT_SUBNET.to_string(),
            client_count: 0,
        })
    } else {
        warn!("Gagal aktifkan hotspot. Minta user untuk buka Settings → Mobile Hotspot secara manual");
        // Buka Settings Mobile Hotspot secara manual sebagai last resort
        let _ = Command::new("ms-settings:network-mobilehotspot")
            .spawn();

        Err(format!(
            "Gagal otomatis aktifkan hotspot. \
             Silakan buka Settings → Network → Mobile Hotspot, \
             atur SSID ke '{}' tanpa password.",
            EMERGENCY_SSID
        ))
    }
}

/// Scan subnet hotspot untuk menemukan peer CARAKA.
///
/// Ketika device konek ke hotspot CARAKA-Emergency (subnet 192.168.137.x),
/// fungsi ini mencoba koneksi TCP ke setiap IP di subnet tersebut.
/// Ini sebagai alternatif UDP broadcast yang mungkin tidak bekerja di
/// beberapa konfigurasi hotspot.
///
/// Return: list IP yang berhasil direspons (kemungkinan peer CARAKA)
pub async fn scan_hotspot_subnet(
    tcp_port: u16,
    timeout_ms: u64,
) -> Vec<String> {
    info!("Scan subnet hotspot {}.*", HOTSPOT_SUBNET);

    let mut found = Vec::new();

    // Scan .1 sampai .254 (skip .0 dan .255)
    // Jalankan scan secara concurrent dengan batasan
    let semaphore = Arc::new(tokio::sync::Semaphore::new(20)); // Max 20 concurrent
    let mut handles = Vec::new();

    for last_octet in 1u8..=254 {
        let ip = format!("{}.{}", HOTSPOT_SUBNET, last_octet);
        let sem = semaphore.clone();
        let port = tcp_port;
        let timeout = timeout_ms;

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.ok()?;
            let addr = format!("{}:{}", ip, port);
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(timeout),
                tokio::net::TcpStream::connect(&addr),
            ).await;

            match result {
                Ok(Ok(_)) => {
                    debug!("Port terbuka di {}", addr);
                    Some(ip)
                }
                _ => None,
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        if let Ok(Some(ip)) = handle.await {
            found.push(ip);
        }
    }

    info!("Scan selesai: {} peer CARAKA ditemukan di subnet {}", found.len(), HOTSPOT_SUBNET);
    found
}
