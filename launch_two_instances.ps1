# launch_two_instances.ps1
# Script untuk test 2 instance CARAKA Desktop di satu laptop
# Jalankan dari: e:\Project APP\CARAKA-DEKSTOP\
#
# Instance A: port default (TCP 7771, UDP 7770)
# Instance B: port berbeda  (TCP 7772, UDP 7773) + AppData dir berbeda

$root = "e:\Project APP\CARAKA-DEKSTOP"

Write-Host "=== CARAKA Two-Instance Test Launcher ===" -ForegroundColor Cyan
Write-Host ""
Write-Host "Instance A: TCP=7771, UDP=7770 (AppData default)" -ForegroundColor Green
Write-Host "Instance B: TCP=7772, UDP=7773 (AppData=C:\tmp\caraka_b)" -ForegroundColor Yellow
Write-Host ""

# Buat dir untuk instance B
if (-not (Test-Path "C:\tmp\caraka_b")) {
    New-Item -ItemType Directory -Path "C:\tmp\caraka_b" | Out-Null
    Write-Host "Dibuat: C:\tmp\caraka_b" -ForegroundColor Gray
}

# Launch Instance A di terminal baru
Write-Host "Meluncurkan Instance A..." -ForegroundColor Green
Start-Process powershell -ArgumentList @(
    "-NoExit",
    "-Command",
    "cd '$root'; `$env:RUST_LOG='caraka_desktop=debug,warn'; `$env:CARAKA_TCP_PORT='7771'; `$env:CARAKA_UDP_PORT='7770'; Write-Host 'INSTANCE A (port 7771/7770)' -ForegroundColor Green; npm run tauri dev"
)

Start-Sleep -Seconds 3

# Launch Instance B di terminal lain
Write-Host "Meluncurkan Instance B (port berbeda)..." -ForegroundColor Yellow
Start-Process powershell -ArgumentList @(
    "-NoExit",
    "-Command",
    "cd '$root'; `$env:APPDATA='C:\tmp\caraka_b'; `$env:RUST_LOG='caraka_desktop=debug,warn'; `$env:CARAKA_TCP_PORT='7772'; `$env:CARAKA_UDP_PORT='7773'; Write-Host 'INSTANCE B (port 7772/7773)' -ForegroundColor Yellow; npm run tauri dev"
)

Write-Host ""
Write-Host "=== Kedua instance sedang dimulai... ===" -ForegroundColor Cyan
Write-Host "Tunggu ~1 menit untuk build selesai." -ForegroundColor Gray
Write-Host ""
Write-Host "CARA TEST CHAT LAN:" -ForegroundColor White
Write-Host "  1. Buat vault + nama di kedua instance" -ForegroundColor White
Write-Host "  2. Tunggu ~10 detik -- peer akan saling ditemukan via UDP" -ForegroundColor White
Write-Host "  3. Klik peer di list -> chat terbuka otomatis" -ForegroundColor White
Write-Host "  4. Ketik pesan -> Ctrl+Enter untuk kirim" -ForegroundColor White
Write-Host ""
Write-Host "CATATAN: UDP broadcast antar port berbeda di localhost bisa terbatas." -ForegroundColor DarkYellow
Write-Host "Jika peer tidak ditemukan otomatis, gunakan 'Add Peer Manual':" -ForegroundColor DarkYellow
Write-Host "  Instance A -> Add Peer -> 127.0.0.1:7772" -ForegroundColor DarkYellow
Write-Host "  Instance B -> Add Peer -> 127.0.0.1:7771" -ForegroundColor DarkYellow
