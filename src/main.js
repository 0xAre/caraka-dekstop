// ═══════════════════════════════════════════════════════════════════════════
// CARAKA Desktop — Main Frontend Logic (REVAMPED SPA)
// ═══════════════════════════════════════════════════════════════════════════
;(function() {
'use strict';

// ── 1. Tauri IPC Wrappers (lazy-init agar aman di module scope) ────────────

// Jangan destructure window.__TAURI__ di top-level module!
// Ini akan crash jika Tauri belum inject global saat module diparsing.
let _invoke = null;
let _listen  = null;

async function ipc(command, args = {}) {
  if (!_invoke) throw new Error('Tauri IPC tidak tersedia');
  try {
    return await _invoke(command, args);
  } catch (err) {
    console.error(`[IPC] ${command} error:`, err);
    throw err;
  }
}

async function on(event, handler) {
  if (!_listen) return () => {};
  return _listen(event, handler);
}


// ── 2. Global State ────────────────────────────────────────────────────────

const state = {
  // Node identity
  myNodeId:     '',
  myNodeIdShort:'',
  myName:       '',
  myFingerprint:'',
  myLocalIp:    '',

  // Peers: Map<nodeId, PeerInfo>
  // PeerInfo: { nodeId, displayName, ip, port, status, fingerprint }
  // status: 'discovered' | 'connecting' | 'connected' | 'failed' | 'disconnected'
  peers: new Map(),

  // Active chat
  activePeerId: null,
  activeConnTimeout: null,

  // Messages: Map<peerId, Array<MessageObj>>
  messages: new Map(),

  // Broadcast messages array
  broadcastMessages: [],
  broadcastLastSent: 0, // rate limiting

  // Current view/panel
  currentPanel: 'home',

  // Reply state — { id, text } | null
  replyTo: null,

  // Seen broadcast message IDs (deduplikasi di UI)
  seenBroadcasts: new Set(),
};

// ── 3. View Router ─────────────────────────────────────────────────────────

function navigateTo(panelName, peerId = null) {
  // SECURITY 3D FIX: cancel radar animation saat pindah panel agar tidak leak memory
  if (panelName !== 'home' && radarAnimFrame) {
    cancelAnimationFrame(radarAnimFrame);
    radarAnimFrame = null;
  }

  // Hide semua panel
  document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
  // Activate target
  const panel = document.getElementById(`panel-${panelName}`);
  if (panel) panel.classList.add('active');

  // Nav button states
  document.querySelectorAll('.nav-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.panel === panelName);
  });

  state.currentPanel = panelName;

  // Panel-specific actions
  if (panelName === 'home') {
    renderRadar();
    // Peer panel: sembunyikan
    document.getElementById('peer-panel').classList.remove('visible');
  }

  if (panelName === 'broadcast') {
    document.getElementById('peer-panel').classList.remove('visible');
    document.getElementById('broadcast-badge').classList.add('hidden');
  }

  if (panelName === 'chats') {
    document.getElementById('peer-panel').classList.add('visible');
    if (peerId) openChat(peerId);
  }
}

// ── 4. Onboarding Flow ─────────────────────────────────────────────────────

let currentSlide = 0;
const TOTAL_SLIDES = 3;

let _onbListenersAttached = false;

function initOnboarding(nodeData) {
  // Isi data node di slide 3
  const nodeIdEl = document.getElementById('onb-node-id');
  const fpEl = document.getElementById('onb-fingerprint');
  if (nodeIdEl) nodeIdEl.textContent = nodeData.nodeId ? nodeData.nodeId.substring(0, 16) + '...' : '—';
  if (fpEl) fpEl.textContent = nodeData.fingerprint || '—';

  // Ambil IP lokal
  ipc('get_local_ip').then(ip => {
    const el = document.getElementById('onb-local-ip');
    if (el) el.textContent = ip || '—';
  }).catch(() => {});

  // Prefill name input jika sudah ada
  const nameInput = document.getElementById('name-input');
  if (nameInput) {
    if (nodeData.displayName && nodeData.displayName !== 'User') {
      nameInput.value = nodeData.displayName;
      updateNamePreview(nodeData.displayName);
    }
    nameInput.addEventListener('input', () => {
      updateNamePreview(nameInput.value);
    });
  }

  // Attach event listeners hanya sekali
  if (!_onbListenersAttached) {
    _onbListenersAttached = true;
    const nextBtn = document.getElementById('onb-next');
    const prevBtn = document.getElementById('onb-prev');
    if (nextBtn) nextBtn.addEventListener('click', handleOnbNext);
    if (prevBtn) prevBtn.addEventListener('click', handleOnbPrev);
  }

  // Force slide 0 tanpa animasi transisi
  const container = document.querySelector('.onboarding-slides');
  if (container) {
    container.style.transition = 'none';
    container.style.transform = 'translateX(0%)';
    // Re-enable transition setelah paint
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        container.style.transition = '';
      });
    });
  }
  showSlide(0);
}

function updateNamePreview(name) {
  const preview = document.getElementById('name-preview');
  if (!preview) return;
  const trimmed = name.trim();
  preview.textContent = trimmed || '\u2014';
}

function showSlide(idx) {
  currentSlide = idx;
  const container = document.querySelector('.onboarding-slides');
  if (container) container.style.transform = `translateX(-${idx * 100}%)`;

  // Dots
  document.querySelectorAll('.step-dot').forEach((dot, i) => {
    dot.classList.toggle('active', i === idx);
  });

  // Prev/Next buttons
  const prevBtn = document.getElementById('onb-prev');
  const nextBtn = document.getElementById('onb-next');
  if (prevBtn) prevBtn.style.visibility = idx === 0 ? 'hidden' : 'visible';
  if (nextBtn) nextBtn.textContent = idx === TOTAL_SLIDES - 1 ? 'Mulai →' : 'Lanjut →';
}

async function handleOnbNext() {
  if (currentSlide === 1) {
    // Validasi nama
    const name = document.getElementById('name-input').value.trim();
    if (!name) {
      showToast('Nama tidak boleh kosong', 'error');
      document.getElementById('name-input').focus();
      return;
    }
    // Simpan nama
    try {
      await ipc('set_display_name', { name });
      state.myName = name;
      updateAllNameDisplays();
    } catch (err) {
      showToast('Gagal menyimpan nama', 'error');
      return;
    }
  }

  if (currentSlide < TOTAL_SLIDES - 1) {
    showSlide(currentSlide + 1);
  } else {
    // Selesai onboarding
    finishOnboarding();
  }
}

function handleOnbPrev() {
  if (currentSlide > 0) showSlide(currentSlide - 1);
}

function finishOnboarding() {
  const onbView = document.getElementById('view-onboarding');
  onbView.style.opacity = '0';
  setTimeout(() => {
    onbView.classList.remove('active');
    showAppView();
  }, 300);
}

// ── 5. Home / Radar View ───────────────────────────────────────────────────────

let _appStartTime = Date.now();

function setupHomeQuickActions() {
  const qaBroadcast = document.getElementById('qa-broadcast');
  const qaChat = document.getElementById('qa-chat');
  const qaQr = document.getElementById('qa-qr');

  if (qaBroadcast) qaBroadcast.addEventListener('click', () => navigateTo('broadcast'));
  if (qaChat) qaChat.addEventListener('click', () => navigateTo('chats'));
  if (qaQr) qaQr.addEventListener('click', () => {
    const qrBtn = document.getElementById('qr-btn');
    if (qrBtn) qrBtn.click();
  });
}

// ── Notification Feed ──────────────────────────────────────────────────────
const MAX_NOTIFS = 50;

function pushNotif(type, icon, text) {
  const feed = document.getElementById('notif-feed');
  const empty = document.getElementById('notif-empty');
  if (!feed) return;

  if (empty) empty.remove();

  const now = new Date();
  const timeStr = now.toLocaleTimeString('id-ID', { hour: '2-digit', minute: '2-digit', second: '2-digit' });

  const item = document.createElement('div');
  item.className = `notif-item type-${type}`;
  item.innerHTML = `
    <div class="notif-icon">${icon}</div>
    <div class="notif-body">
      <div class="notif-text">${text}</div>
      <div class="notif-time">${timeStr}</div>
    </div>`;

  // Insert at top
  feed.insertBefore(item, feed.firstChild);

  // Limit
  const items = feed.querySelectorAll('.notif-item');
  if (items.length > MAX_NOTIFS) {
    items[items.length - 1].remove();
  }
}

function setupNotifFeed() {
  const clearBtn = document.getElementById('notif-clear-btn');
  if (clearBtn) {
    clearBtn.addEventListener('click', () => {
      const feed = document.getElementById('notif-feed');
      if (!feed) return;
      feed.innerHTML = `<div class="notif-empty" id="notif-empty">Belum ada aktivitas &mdash; sistem sedang memantau jaringan mesh</div>`;
    });
  }

  // Notif awal: node siap
  pushNotif('system', '⚡', `Node <strong>${state.myName || 'kamu'}</strong> aktif di jaringan mesh`);
  if (state.myLocalIp) {
    pushNotif('system', '🌐', `IP lokal terdeteksi: <strong>${state.myLocalIp}</strong>`);
  }
}

function updateUptime() {
  const el = document.getElementById('status-uptime');
  if (!el) return;
  const elapsed = Math.floor((Date.now() - _appStartTime) / 1000);
  if (elapsed < 60) {
    el.textContent = `${elapsed}s`;
  } else if (elapsed < 3600) {
    const m = Math.floor(elapsed / 60);
    const s = elapsed % 60;
    el.textContent = `${m}m ${s}s`;
  } else {
    const h = Math.floor(elapsed / 3600);
    const m = Math.floor((elapsed % 3600) / 60);
    el.textContent = `${h}h ${m}m`;
  }
}

// Update uptime setiap 10 detik
setInterval(updateUptime, 10000);

// ── Tor Status Polling ─────────────────────────────────────────────────────
// Poll get_tor_status setiap 10 detik, update chip + onion card di home

async function pollTorStatus() {
  const chipText = document.getElementById('tor-status-text');
  const chipIcon = document.getElementById('tor-status-icon');
  const onionCard = document.getElementById('card-onion');
  const onionAddr = document.getElementById('card-onion-addr');
  if (!chipText) return;

  try {
    const s = await ipc('get_tor_status');
    if (s.status === 'ready') {
      chipText.textContent = 'Online';
      if (chipIcon) {
        chipIcon.style.background = 'linear-gradient(135deg,#34d399,#059669)';
        chipIcon.textContent = '🧅';
      }
      if (s.onionAddress && s.onionAddress !== state.onionAddress) {
        state.onionAddress = s.onionAddress;
        if (onionAddr) onionAddr.textContent = s.onionAddress;
        if (onionCard) onionCard.style.display = '';
        // Update juga di modal jika terbuka
        const modalOnion = document.getElementById('my-onion-in-modal');
        if (modalOnion) modalOnion.textContent = s.onionAddress;
      }
    } else if (s.status === 'bootstrapping') {
      chipText.textContent = 'Bootstrapping...';
      if (chipIcon) chipIcon.style.background = 'linear-gradient(135deg,#f59e0b,#d97706)';
    } else if (s.status === 'failed') {
      chipText.textContent = 'Gagal';
      if (chipIcon) chipIcon.style.background = 'linear-gradient(135deg,#ef4444,#dc2626)';
    }
    // status 'unavailable' = vault belum unlock, skip
  } catch (_) {}
}

// Poll setiap 10 detik
setInterval(pollTorStatus, 10000);



let radarAnimFrame = null;
let radarAngle = 0;

// Posisi peer di radar — deterministic berdasarkan nodeId hash
function getPeerRadarPos(nodeId, canvasSize = 320) {
  const center = canvasSize / 2;
  const maxR   = center * 0.82;

  // Simple hash dari nodeId untuk posisi konsisten
  let hash1 = 0, hash2 = 0;
  for (let i = 0; i < nodeId.length; i++) {
    const c = nodeId.charCodeAt(i);
    hash1 = ((hash1 << 5) - hash1 + c) | 0;
    if (i % 2 === 0) hash2 = ((hash2 << 3) + c) | 0;
  }

  const angle  = ((Math.abs(hash1) % 360) * Math.PI) / 180;
  const radius = (Math.abs(hash2) % 60 + 20) / 100 * maxR; // 20%-80% dari maxR

  return {
    x: center + Math.cos(angle) * radius,
    y: center + Math.sin(angle) * radius,
    angle,
    radius
  };
}

function renderRadar() {
  const canvas = document.getElementById('radar-canvas');
  if (!canvas) return;

  // Use container size for responsive rendering
  const container = document.getElementById('radar-container');
  const rect = container.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const displaySize = Math.round(Math.min(rect.width, rect.height));

  // Set canvas resolution to match display
  canvas.width = displaySize * dpr;
  canvas.height = displaySize * dpr;
  canvas.style.width = displaySize + 'px';
  canvas.style.height = displaySize + 'px';

  const ctx  = canvas.getContext('2d');
  ctx.scale(dpr, dpr);
  const size = displaySize;
  const cx   = size / 2;
  const cy   = size / 2;
  const maxR = cx * 0.88;

  if (radarAnimFrame) cancelAnimationFrame(radarAnimFrame);

  function draw() {
    ctx.clearRect(0, 0, size, size);

    // Background
    ctx.fillStyle = '#070e0e';
    ctx.fillRect(0, 0, size, size);

    // ── HUD Corner Decorators ──
    const cornerLen = 24;
    const cornerOff = 8;
    ctx.strokeStyle = 'rgba(32, 201, 216, 0.35)';
    ctx.lineWidth = 1.5;
    // Top-left
    ctx.beginPath();
    ctx.moveTo(cornerOff, cornerOff + cornerLen); ctx.lineTo(cornerOff, cornerOff); ctx.lineTo(cornerOff + cornerLen, cornerOff);
    ctx.stroke();
    // Top-right
    ctx.beginPath();
    ctx.moveTo(size - cornerOff - cornerLen, cornerOff); ctx.lineTo(size - cornerOff, cornerOff); ctx.lineTo(size - cornerOff, cornerOff + cornerLen);
    ctx.stroke();
    // Bottom-left
    ctx.beginPath();
    ctx.moveTo(cornerOff, size - cornerOff - cornerLen); ctx.lineTo(cornerOff, size - cornerOff); ctx.lineTo(cornerOff + cornerLen, size - cornerOff);
    ctx.stroke();
    // Bottom-right
    ctx.beginPath();
    ctx.moveTo(size - cornerOff - cornerLen, size - cornerOff); ctx.lineTo(size - cornerOff, size - cornerOff); ctx.lineTo(size - cornerOff, size - cornerOff - cornerLen);
    ctx.stroke();

    // ── Lingkaran konsentris + ring labels ──
    const rings = [
      { r: 0.25, label: '25%' },
      { r: 0.50, label: '50%' },
      { r: 0.75, label: '75%' },
      { r: 1.00, label: '' },
    ];
    rings.forEach(ring => {
      const radius = maxR * ring.r;
      ctx.beginPath();
      ctx.arc(cx, cy, radius, 0, Math.PI * 2);
      ctx.strokeStyle = `rgba(32, 201, 216, ${0.06 + ring.r * 0.05})`;
      ctx.lineWidth = 0.8;
      ctx.stroke();

      // Ring label
      if (ring.label) {
        ctx.font = '9px "JetBrains Mono", monospace';
        ctx.fillStyle = 'rgba(32, 201, 216, 0.3)';
        ctx.textAlign = 'left';
        ctx.fillText(ring.label, cx + radius + 4, cy - 2);
      }
    });

    // ── Cross-hair ──
    ctx.strokeStyle = 'rgba(32, 201, 216, 0.06)';
    ctx.lineWidth = 0.5;
    ctx.setLineDash([4, 6]);
    ctx.beginPath(); ctx.moveTo(cx, cy - maxR); ctx.lineTo(cx, cy + maxR); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(cx - maxR, cy); ctx.lineTo(cx + maxR, cy); ctx.stroke();
    ctx.setLineDash([]);

    // ── Sweep arc (glow trail) ──
    const sweepLen = Math.PI * 0.5;
    for (let i = 0; i < 20; i++) {
      const alpha = (1 - i / 20) * 0.15;
      const a = radarAngle - (i * sweepLen) / 20;
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.arc(cx, cy, maxR, a, a + sweepLen / 20);
      ctx.closePath();
      ctx.fillStyle = `rgba(32, 201, 216, ${alpha})`;
      ctx.fill();
    }

    // Leading edge (bright line)
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.lineTo(
      cx + Math.cos(radarAngle) * maxR,
      cy + Math.sin(radarAngle) * maxR
    );
    ctx.strokeStyle = 'rgba(32, 201, 216, 0.75)';
    ctx.lineWidth = 1.5;
    ctx.stroke();

    // ── FITUR 4D: Topology Edges (garis dari center ke peer yang terhubung) ──
    state.peers.forEach((peer, nodeId) => {
      if (peer.status !== 'connected') return;
      const pos = getPeerRadarPos(nodeId, size);
      const gradient = ctx.createLinearGradient(cx, cy, pos.x, pos.y);
      gradient.addColorStop(0, 'rgba(32, 201, 216, 0.4)');
      gradient.addColorStop(1, 'rgba(29, 184, 122, 0.1)');
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.lineTo(pos.x, pos.y);
      ctx.strokeStyle = gradient;
      ctx.lineWidth = 0.8;
      ctx.setLineDash([3, 5]);
      ctx.stroke();
      ctx.setLineDash([]);
    });

    // ── Peer nodes ──
    state.peers.forEach((peer, nodeId) => {
      const pos = getPeerRadarPos(nodeId, size);
      const isOnline = peer.status === 'connected';
      const isConnecting = peer.status === 'connecting';

      const dotR = 7;
      ctx.beginPath();
      ctx.arc(pos.x, pos.y, dotR, 0, Math.PI * 2);

      if (isOnline) {
        ctx.fillStyle = '#1db87a';
        ctx.shadowColor = '#1db87a';
        ctx.shadowBlur = 12;
      } else if (isConnecting) {
        ctx.fillStyle = '#e5a832';
        ctx.shadowColor = '#e5a832';
        ctx.shadowBlur = 10;
      } else {
        ctx.fillStyle = 'rgba(74, 158, 191, 0.5)';
        ctx.shadowBlur = 0;
      }
      ctx.fill();
      ctx.shadowBlur = 0;

      // Outer ring for online
      if (isOnline) {
        ctx.beginPath();
        ctx.arc(pos.x, pos.y, dotR + 4, 0, Math.PI * 2);
        ctx.strokeStyle = 'rgba(29, 184, 122, 0.3)';
        ctx.lineWidth = 1;
        ctx.stroke();
      }

      // Peer name label
      const name = peer.displayName || nodeId.substring(0, 6);
      ctx.font = '10px "Space Grotesk", sans-serif';
      ctx.fillStyle = isOnline ? 'rgba(238, 244, 244, 0.8)' : 'rgba(125, 168, 168, 0.6)';
      ctx.textAlign = 'center';
      ctx.fillText(name, pos.x, pos.y + dotR + 14);
    });

    // ── Center dot (self) ──
    ctx.beginPath();
    ctx.arc(cx, cy, 8, 0, Math.PI * 2);
    ctx.fillStyle = '#20C9D8';
    ctx.shadowColor = '#20C9D8';
    ctx.shadowBlur = 18;
    ctx.fill();
    ctx.shadowBlur = 0;

    // "YOU" label
    ctx.font = 'bold 8px "JetBrains Mono", monospace';
    ctx.fillStyle = 'rgba(32, 201, 216, 0.6)';
    ctx.textAlign = 'center';
    ctx.fillText('YOU', cx, cy + 22);

    radarAngle = (radarAngle + 0.02) % (Math.PI * 2);
    radarAnimFrame = requestAnimationFrame(draw);
  }

  draw();
}


// Radar hover tooltip
function setupRadarInteraction() {
  const canvas  = document.getElementById('radar-canvas');
  const tooltip = document.getElementById('radar-tooltip');

  canvas.addEventListener('mousemove', (e) => {
    const rect   = canvas.getBoundingClientRect();
    const size   = Math.min(rect.width, rect.height);
    const mx     = e.clientX - rect.left;
    const my     = e.clientY - rect.top;

    let found = null;
    state.peers.forEach((peer, nodeId) => {
      const pos = getPeerRadarPos(nodeId, size);
      const dist = Math.hypot(mx - pos.x, my - pos.y);
      if (dist < 16) found = { peer, nodeId, pos };
    });

    if (found) {
      tooltip.style.left   = found.pos.x + 'px';
      tooltip.style.top    = (found.pos.y - 4) + 'px';
      tooltip.textContent  = found.peer.displayName || `Node ${found.nodeId.substring(0, 8)}`;
      tooltip.classList.remove('hidden');
      canvas.style.cursor  = 'pointer';
    } else {
      tooltip.classList.add('hidden');
      canvas.style.cursor  = 'crosshair';
    }
  });

  canvas.addEventListener('mouseleave', () => {
    tooltip.classList.add('hidden');
  });

  // Click pada peer di radar → buka chat
  canvas.addEventListener('click', (e) => {
    const rect   = canvas.getBoundingClientRect();
    const size   = Math.min(rect.width, rect.height);
    const mx     = e.clientX - rect.left;
    const my     = e.clientY - rect.top;

    let found = null;
    state.peers.forEach((peer, nodeId) => {
      const pos  = getPeerRadarPos(nodeId, size);
      const dist = Math.hypot(mx - pos.x, my - pos.y);
      if (dist < 18) found = { peer, nodeId };
    });

    if (found) {
      navigateTo('chats', found.nodeId);
    }
  });
}

// ── 6. Broadcast View ──────────────────────────────────────────────────────

function addBroadcastBubble(data, isMine = false) {
  // Deduplikasi
  if (state.seenBroadcasts.has(data.messageId)) return;
  state.seenBroadcasts.add(data.messageId);

  const container = document.getElementById('broadcast-messages');

  // Hapus empty state jika ada
  const empty = container.querySelector('.empty-state');
  if (empty) empty.remove();

  const avatarColor = getAvatarColor(data.senderId || data.senderName);
  const initial     = (data.senderName || '?')[0].toUpperCase();
  const timeStr     = formatTimestamp(data.timestamp);
  const hopInfo     = data.hopCount > 0 ? `📡 via ${data.hopCount} hop` : '📡 langsung';

  const bubble = document.createElement('div');
  bubble.className = `broadcast-bubble ${isMine ? 'mine' : ''}`;
  bubble.innerHTML = `
    <div class="broadcast-avatar" style="background:${avatarColor}">${initial}</div>
    <div class="broadcast-content">
      <div class="broadcast-sender">${escapeHtml(data.senderName)}</div>
      <div class="broadcast-text-bubble">${escapeHtml(data.text)}</div>
      <div class="broadcast-meta">
        <span>${timeStr}</span>
        <span class="hop-badge">${hopInfo}</span>
      </div>
    </div>
  `;
  container.appendChild(bubble);
  container.scrollTop = container.scrollHeight;

  // Badge di nav jika tidak sedang di broadcast panel
  if (state.currentPanel !== 'broadcast' && !isMine) {
    document.getElementById('broadcast-badge').classList.remove('hidden');
  }
}

function initBroadcastView() {
  const textarea = document.getElementById('broadcast-input');
  const sendBtn  = document.getElementById('broadcast-send-btn');
  const charCnt  = document.getElementById('broadcast-char-count');

  // FITUR 4E: Emergency type selector
  let selectedEmergencyType = 'INFO';
  document.querySelectorAll('.broadcast-type-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('.broadcast-type-btn').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      selectedEmergencyType = btn.dataset.type;
      // Update placeholder based on type
      const placeholders = {
        INFO:     'Tulis pesan informasi untuk seluruh jaringan mesh...',
        EVAC:     'Instruksi evakuasi: lokasi, rute, titik kumpul...',
        STATUS:   'Laporan status: kondisi, lokasi, jumlah orang...',
        RESOURCE: 'Permintaan sumber daya: air, makanan, medis, lokasi...',
      };
      textarea.placeholder = placeholders[selectedEmergencyType] || placeholders.INFO;
    });
  });

  textarea.addEventListener('input', () => {
    charCnt.textContent = textarea.value.length;
    sendBtn.disabled    = textarea.value.trim().length === 0;
  });

  sendBtn.addEventListener('click', async () => {
    const text = textarea.value.trim();
    if (!text) return;

    // Rate limiting: 3 detik
    const now = Date.now();
    if (now - state.broadcastLastSent < 3000) {
      showToast('Tunggu beberapa detik sebelum broadcast lagi', 'warning');
      return;
    }

    // Prefix pesan dengan tipe darurat
    const typePrefix = selectedEmergencyType !== 'INFO' ? `[${selectedEmergencyType}] ` : '';
    const fullText = typePrefix + text;

    // Konfirmasi
    const typeLabel = { INFO: 'informasi', EVAC: 'evakuasi DARURAT', STATUS: 'status', RESOURCE: 'permintaan sumber daya' }[selectedEmergencyType] || 'darurat';
    if (!confirm(`Pesan ${typeLabel} ini akan terlihat oleh SEMUA peer di jaringan. Lanjutkan?`)) return;

    sendBtn.disabled = true;
    sendBtn.textContent = '📡 Mengirim...';

    try {
      await ipc('send_broadcast', { text: fullText });
      textarea.value = '';
      charCnt.textContent = '0';
      state.broadcastLastSent = Date.now();
    } catch (err) {
      showToast('Gagal mengirim broadcast: ' + err, 'error');
    } finally {
      setTimeout(() => {
        sendBtn.disabled    = false;
        sendBtn.textContent = '📡 Broadcast';
      }, 3000);
    }
  });
}

// ── 7. Private Chat View ───────────────────────────────────────────────────

function openChat(peerId) {
  const peer = state.peers.get(peerId);
  if (!peer) {
    showToast('Peer tidak ditemukan', 'error');
    return;
  }

  state.activePeerId = peerId;

  // Show active-chat, hide no-chat
  document.getElementById('no-chat-selected').style.display = 'none';
  const activeChat = document.getElementById('active-chat');
  activeChat.style.display = 'flex';

  // Update peer list active
  document.querySelectorAll('.peer-item').forEach(el => {
    el.classList.toggle('active', el.dataset.peerId === peerId);
  });

  // Update header
  const avatarEl = document.getElementById('chat-avatar');
  avatarEl.style.background = getAvatarColor(peerId);
  avatarEl.textContent      = (peer.displayName || '?')[0].toUpperCase();

  document.getElementById('chat-peer-name').textContent = peer.displayName || peerId.substring(0, 8);
  document.getElementById('chat-peer-fp').textContent   = peer.fingerprint
    ? peer.fingerprint.substring(0, 20) + '...'
    : peer.nodeId.substring(0, 16) + '...';

  // Update connection badge
  updateChatConnectionBadge(peerId);

  // BUG #6 FIX: Load history dari DB dulu, baru render
  loadMessageHistory(peerId).then(() => {
    // Scroll ke pesan terbaru setelah load
    const scroll = document.getElementById('messages-scroll');
    if (scroll) scroll.scrollTop = scroll.scrollHeight;
  });

  // Render pesan yang sudah ada di state (sementara history di-load)
  renderMessages(peerId);

  // Send button
  const sendBtn  = document.getElementById('send-btn');
  const msgInput = document.getElementById('msg-input');
  sendBtn.disabled = peer.status !== 'connected';

  // Jika belum connected, coba initiate connection
  if (peer.status === 'discovered' || peer.status === 'failed' || peer.status === 'disconnected') {
    initiateConnection(peerId);
  }
}

function updateChatConnectionBadge(peerId) {
  if (state.activePeerId !== peerId) return;
  const peer = state.peers.get(peerId);
  if (!peer) return;

  const badge    = document.getElementById('chat-conn-badge');
  const retryBtn = document.getElementById('retry-conn-btn');
  const sendBtn  = document.getElementById('send-btn');

  const configs = {
    connected:    { cls: 'online',      text: '● Online',          retry: false },
    connecting:   { cls: 'connecting',  text: '⟳ Menghubungkan...', retry: false },
    discovered:   { cls: 'offline',     text: '○ Ditemukan',       retry: false },
    failed:       { cls: 'failed',      text: '✕ Gagal',           retry: true  },
    disconnected: { cls: 'offline',     text: '● Offline',         retry: true  },
  };

  const cfg = configs[peer.status] || configs.disconnected;

  badge.className  = `conn-badge ${cfg.cls}`;
  badge.textContent = cfg.text;
  retryBtn.style.display = cfg.retry ? '' : 'none';
  sendBtn.disabled = peer.status !== 'connected';
}

// ── 8. Peer Connection State Machine ──────────────────────────────────────

async function initiateConnection(peerId) {
  const peer = state.peers.get(peerId);
  if (!peer) return;
  if (peer.status === 'connecting' || peer.status === 'connected') return;

  setPeerStatus(peerId, 'connecting');

  // Timeout 7 detik
  if (state.activeConnTimeout) clearTimeout(state.activeConnTimeout);
  state.activeConnTimeout = setTimeout(() => {
    const p = state.peers.get(peerId);
    if (p && p.status === 'connecting') {
      setPeerStatus(peerId, 'failed');
      showToast(`Gagal terhubung ke ${p.displayName}. Pastikan peer aktif.`, 'error');
    }
  }, 7000);

  try {
    await ipc('add_peer_manual', { ip: peer.ip, port: peer.port });
  } catch (err) {
    setPeerStatus(peerId, 'failed');
    showToast('Gagal koneksi: ' + err, 'error');
  }
}

function setPeerStatus(peerId, status) {
  const peer = state.peers.get(peerId);
  if (!peer) return;

  peer.status = status;
  state.peers.set(peerId, peer);

  // Update peer item di list
  const item = document.querySelector(`.peer-item[data-peer-id="${peerId}"]`);
  if (item) {
    const dot = item.querySelector('.peer-status-dot');
    if (dot) {
      dot.className = `peer-status-dot ${status === 'connected' ? 'online' : status === 'connecting' ? 'connecting' : status === 'failed' ? 'failed' : ''}`;
    }
  }

  // Update chat header jika peer ini sedang aktif
  updateChatConnectionBadge(peerId);

  // Refresh radar
  // Radar auto-refresh via rAF
}

// ── 9. Sidebar & Peer List ────────────────────────────────────────────────

function renderPeerList() {
  const container = document.getElementById('peer-list-scroll');
  container.innerHTML = '';

  if (state.peers.size === 0) {
    container.innerHTML = `
      <div class="empty-state">
        <div class="empty-state-icon">🔍</div>
        <div class="empty-state-text">Mencari peer...</div>
        <small>Pastikan peer lain aktif di jaringan yang sama</small>
      </div>
    `;
    return;
  }

  let onlineCount = 0;
  state.peers.forEach((peer, nodeId) => {
    if (peer.status === 'connected') onlineCount++;

    const item = document.createElement('div');
    item.className = 'peer-item';
    item.dataset.peerId = nodeId;

    const avatarColor = getAvatarColor(nodeId);
    const initial     = (peer.displayName || '?')[0].toUpperCase();
    const statusClass = peer.status === 'connected' ? 'online'
                      : peer.status === 'connecting' ? 'connecting'
                      : peer.status === 'failed' ? 'failed' : '';

    item.innerHTML = `
      <div class="peer-avatar-wrap">
        <div class="peer-avatar" style="background:${avatarColor}">${initial}</div>
        <div class="peer-status-dot ${statusClass}"></div>
      </div>
      <div class="peer-item-info">
        <div class="peer-item-name">${escapeHtml(peer.displayName || 'Unknown')}</div>
        <div class="peer-item-meta">${peer.ip || ''}</div>
      </div>
    `;

    item.addEventListener('click', () => {
      navigateTo('chats', nodeId);
    });

    container.appendChild(item);
  });

  // Update counts
  document.getElementById('peer-count').textContent     = onlineCount;
  document.getElementById('card-peers-online').textContent = onlineCount;
  document.getElementById('card-peers-known').textContent  = state.peers.size;

  // Network status
  const netDot  = document.getElementById('net-status-dot');
  const netText = document.getElementById('net-status-text');
  const homeDot = document.getElementById('home-net-dot');
  const homeText= document.getElementById('home-net-text');

  if (onlineCount > 0) {
    netDot.className   = 'status-dot online';
    netText.textContent = `${onlineCount} peer terhubung`;
    homeDot.className  = 'status-dot online';
    homeText.textContent = `${onlineCount} online`;
  } else {
    netDot.className   = 'status-dot searching';
    netText.textContent = 'Mencari peer...';
    homeDot.className  = 'status-dot searching';
    homeText.textContent = 'Mencari...';
  }
}

// ── 10. Toast Notification System ─────────────────────────────────────────

function showToast(message, type = 'info', duration = 4000) {
  const container = document.getElementById('toast-container');

  const icons = { success: '✅', error: '❌', warning: '⚠️', info: '📡' };

  const toast = document.createElement('div');
  toast.className = `toast ${type}`;
  toast.innerHTML = `
    <span class="toast-icon">${icons[type] || icons.info}</span>
    <span class="toast-text">${escapeHtml(String(message))}</span>
  `;

  container.appendChild(toast);

  // Auto remove
  setTimeout(() => {
    toast.style.animation = 'toastOut 0.3s ease both';
    setTimeout(() => toast.remove(), 300);
  }, duration);
}

// ── 11. QR Code Modal ─────────────────────────────────────────────────────

// Minimalist QR encoder menggunakan canvas — menggunakan library qrcode-svg
// Karena kita pure vanilla, kita buat QR sederhana dengan data URL placeholder
// Implementasi actual memerlukan qrcode library atau qrcodegen

function generateQR() {
  const canvas    = document.getElementById('qr-canvas');
  const ctx       = canvas.getContext('2d');
  const localIpEl = document.getElementById('qr-local-ip');

  // Set IP
  localIpEl.textContent = state.myLocalIp || '—';

  // QR data: caraka://nodeId@ip:port
  const qrData = `caraka://${state.myNodeId}@${state.myLocalIp}:7771`;

  // Render sederhana: tampilkan teks info di canvas sampai library dimuat
  ctx.fillStyle = 'white';
  ctx.fillRect(0, 0, 200, 200);
  ctx.fillStyle = '#070e0e';
  ctx.font = '10px monospace';
  ctx.textAlign = 'center';

  // Cek apakah qrcode tersedia (dari CDN atau bundled)
  if (window.QRCode) {
    try {
      // Jika library ada, gunakan
      const qr = new window.QRCode(canvas, {
        text: qrData,
        width: 200, height: 200,
        colorDark: '#070e0e',
        colorLight: '#ffffff',
      });
    } catch {
      drawFallbackQR(ctx, qrData);
    }
  } else {
    drawFallbackQR(ctx, qrData);
  }
}

function drawFallbackQR(ctx, data) {
  // Fallback: tampilkan info sebagai teks
  ctx.fillStyle = 'white';
  ctx.fillRect(0, 0, 200, 200);

  // Border
  ctx.strokeStyle = '#070e0e';
  ctx.lineWidth   = 8;
  ctx.strokeRect(4, 4, 192, 192);

  ctx.fillStyle  = '#070e0e';
  ctx.font       = 'bold 11px monospace';
  ctx.textAlign  = 'center';
  ctx.fillText('CARAKA', 100, 80);
  ctx.font       = '9px monospace';

  // Potong data panjang ke beberapa baris
  const lines = data.match(/.{1,22}/g) || [data];
  lines.forEach((line, i) => {
    ctx.fillText(line, 100, 100 + i * 14);
  });
}

// ── 12. Settings Modal ────────────────────────────────────────────────────

function openSettings() {
  document.getElementById('settings-name-input').value   = state.myName;
  document.getElementById('settings-node-id').textContent = state.myNodeId
    ? state.myNodeId.substring(0, 32) + '...' : '—';
  document.getElementById('settings-fingerprint').textContent = state.myFingerprint || '—';
  document.getElementById('settings-local-ip').textContent    = state.myLocalIp || '—';

  document.getElementById('modal-settings').classList.remove('hidden');
}

async function saveSettings() {
  const name = document.getElementById('settings-name-input').value.trim();
  if (!name) {
    showToast('Nama tidak boleh kosong', 'error');
    return;
  }

  try {
    await ipc('set_display_name', { name });
    state.myName = name;
    updateAllNameDisplays();
    document.getElementById('modal-settings').classList.add('hidden');
    showToast('Nama berhasil disimpan', 'success');
  } catch (err) {
    showToast('Gagal menyimpan: ' + err, 'error');
  }
}

// ── 13. Backend Event Listeners ───────────────────────────────────────────

async function setupEventListeners() {
  // Node siap
  await on('node_ready', (event) => {
    console.log('[CARAKA] Node ready:', event.payload);
  });

  // Emergency Mode events
  try { await setupEmergencyEventListeners(); } catch(emErr) {
    console.warn('[CARAKA] Emergency event listeners gagal:', emErr);
  }

  // Peer ditemukan via UDP discovery
  await on('peer_discovered', (event) => {
    const d = event.payload;
    if (!state.peers.has(d.nodeId)) {
      state.peers.set(d.nodeId, {
        nodeId:      d.nodeId,
        displayName: d.displayName || `Node ${d.nodeId.substring(0, 8)}`,
        ip:          d.ip,
        port:        d.port,
        status:      'discovered',
        fingerprint: '',
      });
    } else {
      const p = state.peers.get(d.nodeId);
      p.ip   = d.ip;
      p.port = d.port;
      if (d.displayName && d.displayName !== 'Unknown') p.displayName = d.displayName;
      state.peers.set(d.nodeId, p);
    }
    renderPeerList();
  });

  // Peer mulai connecting
  await on('peer_connecting', (event) => {
    const { ip, nodeId: connectingNodeId } = event.payload;
    // Update via nodeId langsung (Tor outbound) atau cari via IP (LAN)
    if (connectingNodeId && state.peers.has(connectingNodeId)) {
      setPeerStatus(connectingNodeId, 'connecting');
    } else {
      state.peers.forEach((peer, nodeId) => {
        if (peer.ip === ip) setPeerStatus(nodeId, 'connecting');
      });
    }
  });

  // Peer handshake selesai — juga set status connected
  await on('peer_handshaked', (event) => {
    const d = event.payload;
    if (!state.peers.has(d.nodeId)) {
      state.peers.set(d.nodeId, {
        nodeId:      d.nodeId,
        displayName: d.displayName || `Node ${d.nodeId.substring(0, 8)}`,
        ip:          d.ip,
        port:        d.port || 7771,
        status:      'connected',   // handshake = sudah terhubung
        fingerprint: d.fingerprint || '',
      });
    } else {
      const p = state.peers.get(d.nodeId);
      if (d.displayName) p.displayName = d.displayName;
      if (d.fingerprint) p.fingerprint = d.fingerprint;
      p.status = 'connected';       // update status ke connected
      state.peers.set(d.nodeId, p);
    }
    // Jika peer ini sedang aktif di chat, update badge
    updateChatConnectionBadge(d.nodeId);
    if (state.activePeerId === d.nodeId) {
      document.getElementById('send-btn').disabled = false;
    }
    renderPeerList();
  });

  // Peer connected
  await on('peer_connected', (event) => {
    const d = event.payload;
    const nodeId = d.nodeId || findPeerByIp(d.ip);
    // Jika peer belum ada di state (Tor inbound baru), buat entry sementara
    if (nodeId && !state.peers.has(nodeId)) {
      state.peers.set(nodeId, {
        nodeId,
        displayName: `Node ${nodeId.substring(0, 8)}`,
        ip:          d.ip || 'tor',
        port:        d.port || 9999,
        status:      'connected',
        fingerprint: '',
      });
    }
    if (state.activeConnTimeout) { clearTimeout(state.activeConnTimeout); state.activeConnTimeout = null; }
    if (nodeId) {
      setPeerStatus(nodeId, 'connected');
      const peer = state.peers.get(nodeId);
      if (peer) {
        showToast(`${peer.displayName} terhubung`, 'success');
        pushNotif('peer', '🟢', `<strong>${peer.displayName}</strong> bergabung ke jaringan mesh`);
      }
    }
    renderPeerList();
  });

  // Peer connect failed
  await on('peer_connect_failed', (event) => {
    const { ip, reason } = event.payload;
    const nodeId = findPeerByIp(ip);
    if (state.activeConnTimeout) { clearTimeout(state.activeConnTimeout); state.activeConnTimeout = null; }
    if (nodeId) {
      setPeerStatus(nodeId, 'failed');
      const peer = state.peers.get(nodeId);
      showToast(`Gagal ke ${peer?.displayName || ip}: ${reason}`, 'error');
    }
    renderPeerList();
  });

  // Peer disconnected
  await on('peer_disconnected', (event) => {
    const d = event.payload;
    const nodeId = d.nodeId || findPeerByIp(d.ip);
    if (nodeId) {
      setPeerStatus(nodeId, 'disconnected');
      const peer = state.peers.get(nodeId);
      if (peer) {
        showToast(`${peer.displayName} terputus`, 'warning');
        pushNotif('warn', '🔴', `<strong>${peer.displayName}</strong> terputus dari jaringan`);
      }
    }
    renderPeerList();
  });

  // Pesan DM diterima
  await on('clamp_packet_received', (event) => {
    handleIncomingPacket(event.payload);
  });

  // Pesan DM terkirim
  await on('message_sent', (event) => {
    const d = event.payload;
    appendMessage(d.recipientId, {
      id:          d.id,
      text:        d.text,
      timestamp:   d.timestamp,
      outgoing:    true,
      replyToId:   d.replyToId   || null,
      replyToText: d.replyToText || null,
    });
  });

  // Broadcast diterima
  await on('broadcast_received', (event) => {
    const d = event.payload;
    addBroadcastBubble({
      senderId:   d.senderId,
      senderName: d.senderName,
      text:       d.text,
      timestamp:  d.timestamp,
      messageId:  d.messageId,
      hopCount:   d.hopCount || 0,
    });
    const shortText = d.text.length > 40 ? d.text.substring(0, 40) + '...' : d.text;
    pushNotif('warn', '📢', `<strong>${d.senderName || 'Anonim'}</strong>: ${shortText}`);
  });

  // Broadcast terkirim
  await on('broadcast_sent', (event) => {
    const d = event.payload;
    addBroadcastBubble({
      senderId:   d.senderId,
      senderName: d.senderName,
      text:       d.text,
      timestamp:  d.timestamp,
      messageId:  d.messageId,
      hopCount:   0,
    }, true);
  });

  // Error node
  await on('node_error', (event) => {
    showToast('Node error: ' + (event.payload?.error || 'Unknown'), 'error', 8000);
  });
}

async function handleIncomingPacket(d) {
  try {
    // BUG #2 FIX: Gunakan parameter names yang sesuai dengan Rust command signature
    // Transport emits: packetId, nonce, ciphertext, aeadTag
    // Rust expects: packet_id (packetId), nonce_hex (nonceHex), ciphertext_hex (ciphertextHex), aead_tag_hex (aeadTagHex)
    const result = await ipc('try_decrypt_packet', {
      packetId:     d.packetId,
      nonceHex:     d.nonce,
      ciphertextHex: d.ciphertext,
      aeadTagHex:   d.aeadTag,
      ttl:          d.ttl ?? null,   // TTL aktual dari paket untuk AAD yang benar
    });

    if (result && result.plaintext) {
      const senderId = result.senderId || d.packetId;
      appendMessage(senderId, {
        id:          result.id,
        text:        result.plaintext,
        timestamp:   result.timestamp || Math.floor(Date.now() / 1000),
        outgoing:    false,
        replyToId:   result.replyToId   || null,
        replyToText: result.replyToText || null,
      });
      // Notif pesan masuk
      const sender = state.peers.get(senderId);
      const senderName = sender?.displayName || senderId?.substring(0, 8) || 'Unknown';
      const shortText = result.plaintext.length > 40 ? result.plaintext.substring(0, 40) + '...' : result.plaintext;
      pushNotif('msg', '💬', `<strong>${escapeHtml(senderName)}</strong>: ${escapeHtml(shortText)}`);
    }
  } catch (err) {
    console.warn('[CARAKA] Gagal dekripsi packet:', err);
  }
}

// ── 14. App Initialization ────────────────────────────────────────────────

// ── Vault Screens (F1) ─────────────────────────────────────────────────────

function showVaultCreate() {
  const splash = document.getElementById('view-splash');
  if (splash) { splash.style.opacity = '0'; splash.classList.remove('active'); }
  const vc = document.getElementById('view-vault-create');
  if (vc) { vc.classList.add('active'); vc.style.display = ''; }
  document.getElementById('vc-pass')?.focus();
}

function showVaultUnlock() {
  const splash = document.getElementById('view-splash');
  if (splash) { splash.style.opacity = '0'; splash.classList.remove('active'); }
  const vu = document.getElementById('view-vault-unlock');
  if (vu) { vu.classList.add('active'); vu.style.display = ''; }
  document.getElementById('vu-pass')?.focus();
}

function hideVaultScreens() {
  ['view-vault-create', 'view-vault-unlock'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.classList.remove('active');
  });
}

function vaultStrength(pass) {
  if (!pass) return { label: '', cls: '' };
  if (pass.length < 8) return { label: 'Terlalu pendek', cls: 'weak' };
  let score = 0;
  if (/[A-Z]/.test(pass)) score++;
  if (/[0-9]/.test(pass)) score++;
  if (/[^A-Za-z0-9]/.test(pass)) score++;
  if (pass.length >= 12) score++;
  if (score <= 1) return { label: 'Lemah', cls: 'weak' };
  if (score === 2) return { label: 'Sedang', cls: 'medium' };
  return { label: 'Kuat', cls: 'strong' };
}

function setupVaultScreens() {
  // Eye toggle — show/hide password
  document.querySelectorAll('.vault-eye').forEach(btn => {
    btn.addEventListener('click', () => {
      const inp = document.getElementById(btn.dataset.target);
      if (!inp) return;
      inp.type = inp.type === 'password' ? 'text' : 'password';
    });
  });

  // Password strength indicator (create screen)
  const vcPass = document.getElementById('vc-pass');
  const vcStrength = document.getElementById('vc-strength');
  vcPass?.addEventListener('input', () => {
    if (!vcStrength) return;
    const s = vaultStrength(vcPass.value);
    vcStrength.textContent = s.label;
    vcStrength.className = 'vault-strength ' + s.cls;
  });

  // Create vault submit
  const vcSubmit = document.getElementById('vc-submit');
  const vcError = document.getElementById('vc-error');
  vcSubmit?.addEventListener('click', async () => {
    const pass = document.getElementById('vc-pass')?.value || '';
    const confirm = document.getElementById('vc-confirm')?.value || '';
    if (vcError) vcError.classList.add('hidden');

    if (pass.length < 8) {
      if (vcError) { vcError.textContent = 'Password minimal 8 karakter.'; vcError.classList.remove('hidden'); }
      return;
    }
    if (pass !== confirm) {
      if (vcError) { vcError.textContent = 'Password tidak cocok. Coba lagi.'; vcError.classList.remove('hidden'); }
      return;
    }

    const btnText = document.getElementById('vc-btn-text');
    const spinner = document.getElementById('vc-spinner');
    if (btnText) btnText.classList.add('hidden');
    if (spinner) spinner.classList.remove('hidden');
    vcSubmit.disabled = true;

    try {
      await ipc('create_vault', { passphrase: pass });
      hideVaultScreens();
      await proceedAfterVault();
    } catch (err) {
      if (vcError) { vcError.textContent = String(err); vcError.classList.remove('hidden'); }
    } finally {
      if (btnText) btnText.classList.remove('hidden');
      if (spinner) spinner.classList.add('hidden');
      vcSubmit.disabled = false;
    }
  });

  // Enter key on create form
  document.getElementById('vc-confirm')?.addEventListener('keydown', e => {
    if (e.key === 'Enter') vcSubmit?.click();
  });
  document.getElementById('vc-pass')?.addEventListener('keydown', e => {
    if (e.key === 'Enter') document.getElementById('vc-confirm')?.focus();
  });

  // Unlock vault submit
  const vuSubmit = document.getElementById('vu-submit');
  const vuError = document.getElementById('vu-error');
  vuSubmit?.addEventListener('click', async () => {
    const pass = document.getElementById('vu-pass')?.value || '';
    if (vuError) vuError.classList.add('hidden');

    if (!pass) {
      if (vuError) { vuError.textContent = 'Masukkan password.'; vuError.classList.remove('hidden'); }
      return;
    }

    const btnText = document.getElementById('vu-btn-text');
    const spinner = document.getElementById('vu-spinner');
    if (btnText) btnText.classList.add('hidden');
    if (spinner) spinner.classList.remove('hidden');
    vuSubmit.disabled = true;

    try {
      await ipc('unlock_vault', { passphrase: pass });
      hideVaultScreens();
      await proceedAfterVault();
    } catch (err) {
      if (vuError) { vuError.textContent = String(err); vuError.classList.remove('hidden'); }
      document.getElementById('vu-pass')?.select();
    } finally {
      if (btnText) btnText.classList.remove('hidden');
      if (spinner) spinner.classList.add('hidden');
      vuSubmit.disabled = false;
    }
  });

  // Enter key on unlock form
  document.getElementById('vu-pass')?.addEventListener('keydown', e => {
    if (e.key === 'Enter') vuSubmit?.click();
  });
}

/** Dipanggil setelah vault berhasil dibuka — lanjutkan init node */
async function proceedAfterVault() {
  const statusEl = document.getElementById('splash-status');

  // Tampilkan splash lagi sebentar sambil init
  const splash = document.getElementById('view-splash');
  if (splash) {
    splash.style.opacity = '1';
    splash.classList.add('active');
  }
  if (statusEl) statusEl.textContent = 'Menginisialisasi node...';

  try {
    const nodeData = await ipc('init_node');
    console.log('[CARAKA] Node ready:', nodeData.nodeId?.substring(0, 8));

    state.myNodeId      = nodeData.nodeId || '';
    state.myNodeIdShort = nodeData.nodeId ? nodeData.nodeId.substring(0, 16) + '...' : '';
    state.myName        = nodeData.displayName || 'User';
    state.myFingerprint = nodeData.fingerprint || '';

    try { state.myLocalIp = await ipc('get_local_ip'); } catch { state.myLocalIp = 'Tidak diketahui'; }

    if (statusEl) statusEl.textContent = 'Node siap!';

    try {
      const peers = await ipc('get_peers');
      peers.forEach(p => {
        const nodeId      = p.nodeId || p.node_id;
        const displayName = p.displayName || p.display_name || nodeId?.substring(0, 8) || '?';
        const ip          = p.ipAddress || p.ip_address || p.ip || '';
        const port        = p.tcpPort || p.tcp_port || 7771;
        if (nodeId) {
          state.peers.set(nodeId, { nodeId, displayName, ip, port, status: 'disconnected', fingerprint: p.fingerprint || '' });
        }
      });
    } catch (e) { console.warn('[CARAKA] get_peers error:', e); }

    await delay(600);

    const needsOnboarding = !nodeData.displayName
      || nodeData.displayName === 'User'
      || nodeData.displayName.trim() === '';

    if (needsOnboarding) { showOnboarding(nodeData); } else { showAppView(); }

  } catch (err) {
    console.error('[CARAKA] proceedAfterVault error:', err);
    if (statusEl) statusEl.textContent = 'Error: ' + err;
    const statusDot = document.querySelector('.splash-status-dot');
    if (statusDot) { statusDot.style.background = '#d94f4f'; statusDot.style.animation = 'none'; }
    setTimeout(() => showToast('Gagal memulai node: ' + err, 'error', 10000), 500);
  }
}

async function initApp() {
  console.log('[CARAKA] Memulai inisialisasi...');
  const statusEl = document.getElementById('splash-status');
  if (statusEl) statusEl.textContent = 'Memeriksa keamanan...';

  // Setup vault UI handlers sekali saja
  setupVaultScreens();

  // Setup background event listeners (tidak butuh state)
  try { await setupEventListeners(); } catch(evErr) {
    console.warn('[CARAKA] Event listeners gagal:', evErr);
  }

  // Cek apakah vault sudah ada
  try {
    if (statusEl) statusEl.textContent = 'Memuat vault...';
    const vaultExists = await ipc('check_vault_exists');

    await delay(400); // beri waktu splash tampil sebentar

    if (vaultExists) {
      showVaultUnlock();
    } else {
      showVaultCreate();
    }
  } catch (err) {
    console.error('[CARAKA] Vault check error:', err);
    // Fallback: tampilkan vault create (safe default)
    showVaultCreate();
  }
}

function showOnboarding(nodeData) {
  const splash = document.getElementById('view-splash');
  if (splash) splash.style.opacity = '0';
  setTimeout(() => {
    if (splash) splash.classList.remove('active');
    const onb = document.getElementById('view-onboarding');
    if (onb) {
      onb.style.opacity = '1';
      onb.classList.add('active');
    }
    // Reset slide ke 0 dan inisialisasi
    currentSlide = 0;
    initOnboarding(nodeData);
  }, 300);
}

function showAppView() {
  const splash = document.getElementById('view-splash');
  const onb    = document.getElementById('view-onboarding');
  if (splash) {
    splash.style.opacity = '0';
    splash.classList.remove('active');
  }
  if (onb) {
    onb.style.opacity = '0';
    setTimeout(() => onb.classList.remove('active'), 300);
  }

  const app = document.getElementById('view-app');
  app.classList.add('active');

  // Setup UI setelah app tampil
  updateAllNameDisplays();
  updateHomeCards();
  renderPeerList();
  renderRadar();
  setupRadarInteraction();
  initBroadcastView();
  setupUIInteractions();
  setupChatInput();
  setupHomeQuickActions();
  setupEmergencyMode();
  updateUptime();
  setupNotifFeed();
  setupInviteModal();
  setupFileAttach();
  setupFileEventListeners().catch(e => console.warn('file/tor listeners:', e));
  document.getElementById('copy-onion-btn')?.addEventListener('click', () => {
    if (state.onionAddress) copyText(state.onionAddress, 'Onion address disalin');
  });
}

function updateAllNameDisplays() {
  const initial = state.myName ? state.myName[0].toUpperCase() : 'C';

  // Avatar button di sidebar
  const avatarBtn = document.getElementById('my-avatar-btn');
  if (avatarBtn) avatarBtn.textContent = initial;

  // Home greeting
  const greetingEl = document.getElementById('home-greeting-name');
  if (greetingEl) greetingEl.textContent = state.myName || 'Node';

  // My short ID
  const shortIdEl = document.getElementById('my-short-id');
  if (shortIdEl) shortIdEl.textContent = state.myNodeIdShort || '—';
}

function updateHomeCards() {
  const fpEl  = document.getElementById('card-fingerprint');
  const ipEl  = document.getElementById('card-local-ip');
  const qrIpEl = document.getElementById('qr-local-ip');

  if (fpEl)   fpEl.textContent  = state.myFingerprint || '—';
  if (ipEl)   ipEl.textContent  = state.myLocalIp     || '—';
  if (qrIpEl) qrIpEl.textContent = state.myLocalIp   || '—';
}

function setupUIInteractions() {
  // Nav buttons
  document.querySelectorAll('.nav-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      navigateTo(btn.dataset.panel);
    });
  });

  // My avatar → settings (profile)
  document.getElementById('my-avatar-btn').addEventListener('click', openSettings);

  // Logout / Reset button
  const logoutBtn = document.getElementById('logout-btn');
  if (logoutBtn) {
    logoutBtn.addEventListener('click', () => {
      if (confirm('Reset aplikasi dan keluar? Semua data lokal (nama, riwayat) akan dihapus.')) {
        try { localStorage.clear(); } catch(_) {}
        try { sessionStorage.clear(); } catch(_) {}
        showToast('Mereset aplikasi...', 'info', 1500);
        setTimeout(() => window.location.reload(), 1600);
      }
    });
  }

  // Settings modal
  document.getElementById('close-settings-modal').addEventListener('click', () => {
    document.getElementById('modal-settings').classList.add('hidden');
  });
  document.getElementById('cancel-settings').addEventListener('click', () => {
    document.getElementById('modal-settings').classList.add('hidden');
  });
  document.getElementById('save-settings').addEventListener('click', saveSettings);

  // ── Add Peer Modal (LAN / Tor tabs) ───────────────────────────────────────

  // State tab aktif
  let addPeerActiveTab = 'lan';

  function openAddPeerModal() {
    document.getElementById('modal-add-peer').classList.remove('hidden');
    // Default tab: LAN
    switchAddPeerTab('lan');
  }

  function switchAddPeerTab(tab) {
    addPeerActiveTab = tab;
    const lanBtn  = document.getElementById('tab-lan-btn');
    const torBtn  = document.getElementById('tab-tor-btn');
    const lanBody = document.getElementById('tab-lan-content');
    const torBody = document.getElementById('tab-tor-content');
    const confirmBtn = document.getElementById('confirm-add-peer');

    if (tab === 'lan') {
      lanBtn.style.background  = 'var(--accent)';
      lanBtn.style.color       = '#fff';
      lanBtn.style.border      = '1.5px solid var(--accent)';
      torBtn.style.background  = 'transparent';
      torBtn.style.color       = 'var(--text-secondary)';
      torBtn.style.border      = '1.5px solid var(--border)';
      lanBody.style.display    = '';
      torBody.style.display    = 'none';
      confirmBtn.textContent   = 'Hubungkan';
    } else {
      torBtn.style.background  = 'linear-gradient(135deg,#6b46c1,#553c9a)';
      torBtn.style.color       = '#fff';
      torBtn.style.border      = '1.5px solid #6b46c1';
      lanBtn.style.background  = 'transparent';
      lanBtn.style.color       = 'var(--text-secondary)';
      lanBtn.style.border      = '1.5px solid var(--border)';
      lanBody.style.display    = 'none';
      torBody.style.display    = '';
      confirmBtn.textContent   = '🧅 Hubungkan via Tor';
      // Update Tor status & onion address saat tab dibuka
      updateTorModalStatus();
    }
  }

  async function updateTorModalStatus() {
    const statusEl = document.getElementById('tor-modal-status');
    const myOnionEl = document.getElementById('my-onion-in-modal');
    try {
      const s = await ipc('get_tor_status');
      if (s.status === 'ready') {
        statusEl.textContent = '✅ Tor siap — koneksi onion tersedia';
        statusEl.style.background = 'rgba(0,200,100,0.10)';
        statusEl.style.borderColor = 'rgba(0,200,100,0.3)';
        statusEl.style.color = '#34d399';
        if (s.onionAddress) {
          state.onionAddress = s.onionAddress;
          myOnionEl.textContent = s.onionAddress;
          // Update card di home juga
          const cardEl = document.getElementById('card-onion-addr');
          if (cardEl) cardEl.textContent = s.onionAddress;
          document.getElementById('card-onion')?.style.removeProperty('display');
        }
      } else if (s.status === 'bootstrapping') {
        statusEl.textContent = '⏳ Tor sedang bootstrapping... (30-60 detik pertama kali)';
        statusEl.style.background = 'rgba(107,70,193,0.12)';
        statusEl.style.borderColor = 'rgba(107,70,193,0.3)';
        statusEl.style.color = 'var(--text-secondary)';
        myOnionEl.textContent = '— (menunggu Tor siap)';
      } else if (s.status === 'failed') {
        statusEl.textContent = `❌ Tor gagal: ${s.error || 'error tidak diketahui'}`;
        statusEl.style.background = 'rgba(239,68,68,0.10)';
        statusEl.style.borderColor = 'rgba(239,68,68,0.3)';
        statusEl.style.color = '#f87171';
        myOnionEl.textContent = '— (Tor tidak tersedia)';
      } else {
        statusEl.textContent = '— Vault belum di-unlock';
        myOnionEl.textContent = '—';
      }
    } catch(e) {
      statusEl.textContent = '❓ Status tidak diketahui: ' + e;
    }
  }

  document.getElementById('add-peer-btn').addEventListener('click', openAddPeerModal);

  document.getElementById('tab-lan-btn').addEventListener('click', () => switchAddPeerTab('lan'));
  document.getElementById('tab-tor-btn').addEventListener('click', () => switchAddPeerTab('tor'));

  document.getElementById('close-add-peer-modal').addEventListener('click', () => {
    document.getElementById('modal-add-peer').classList.add('hidden');
  });
  document.getElementById('cancel-add-peer').addEventListener('click', () => {
    document.getElementById('modal-add-peer').classList.add('hidden');
  });

  // Klik onion address sendiri → salin
  document.getElementById('my-onion-in-modal')?.addEventListener('click', () => {
    const addr = state.onionAddress;
    if (addr && addr !== '—') copyText(addr, 'Onion address disalin!');
  });

  document.getElementById('confirm-add-peer').addEventListener('click', async () => {
    if (addPeerActiveTab === 'lan') {
      // ── LAN connect ──────────────────────────────────────────
      const ip   = document.getElementById('peer-ip-input').value.trim();
      const port = parseInt(document.getElementById('peer-port-input').value) || 7771;
      if (!ip) { showToast('IP tidak boleh kosong', 'error'); return; }
      try {
        await ipc('add_peer_manual', { ip, port });
        document.getElementById('modal-add-peer').classList.add('hidden');
        showToast(`Menghubungkan ke ${ip}:${port}...`, 'info');
      } catch (err) {
        showToast('Gagal: ' + err, 'error');
      }
    } else {
      // ── Tor connect ──────────────────────────────────────────
      const onion = document.getElementById('peer-onion-input').value.trim().toLowerCase();
      if (!onion) { showToast('Masukkan onion address peer', 'error'); return; }
      if (!onion.endsWith('.onion')) { showToast('Alamat harus berakhiran .onion', 'error'); return; }
      const btn = document.getElementById('confirm-add-peer');
      btn.disabled = true;
      btn.textContent = '🧅 Menghubungkan...';
      try {
        await ipc('connect_via_tor', { onionAddress: onion, nodeId: null });
        document.getElementById('modal-add-peer').classList.add('hidden');
        document.getElementById('peer-onion-input').value = '';
        showToast(`Menghubungkan ke ${onion.substring(0,16)}... via Tor`, 'info');
        pushNotif('tor', '🧅', `Koneksi Tor ke <strong>${onion.substring(0,16)}...</strong> dimulai`);
      } catch (err) {
        showToast('Koneksi Tor gagal: ' + err, 'error');
      } finally {
        btn.disabled = false;
        btn.textContent = '🧅 Hubungkan via Tor';
      }
    }
  });

  // QR Code modal
  document.getElementById('qr-btn').addEventListener('click', () => {
    generateQR();
    document.getElementById('modal-qr').classList.remove('hidden');
  });
  document.getElementById('close-qr-modal').addEventListener('click', () => {
    document.getElementById('modal-qr').classList.add('hidden');
  });
  document.getElementById('close-qr-modal-btn').addEventListener('click', () => {
    document.getElementById('modal-qr').classList.add('hidden');
  });
  document.getElementById('download-qr-btn').addEventListener('click', () => {
    const canvas = document.getElementById('qr-canvas');
    const link   = document.createElement('a');
    link.download = 'caraka-qrcode.png';
    link.href     = canvas.toDataURL();
    link.click();
  });

  // Copy buttons
  document.getElementById('copy-ip-btn').addEventListener('click', () => {
    copyText(state.myLocalIp, 'IP berhasil disalin');
  });
  document.getElementById('copy-fp-btn').addEventListener('click', () => {
    copyText(state.myFingerprint, 'Fingerprint berhasil disalin');
  });
  document.getElementById('copy-node-id-btn').addEventListener('click', () => {
    copyText(state.myNodeId, 'Node ID berhasil disalin');
  });

  // Chat fingerprint copy
  document.getElementById('chat-peer-fp').addEventListener('click', () => {
    if (state.activePeerId) {
      const peer = state.peers.get(state.activePeerId);
      if (peer) copyText(peer.fingerprint || peer.nodeId, 'Fingerprint disalin');
    }
  });

  // Retry connection
  document.getElementById('retry-conn-btn').addEventListener('click', () => {
    if (state.activePeerId) initiateConnection(state.activePeerId);
  });

  // Close modals on overlay click
  document.querySelectorAll('.modal-overlay').forEach(overlay => {
    overlay.addEventListener('click', (e) => {
      if (e.target === overlay) overlay.classList.add('hidden');
    });
  });
}

function setupChatInput() {
  const input   = document.getElementById('msg-input');
  const sendBtn = document.getElementById('send-btn');
  const charCnt = document.getElementById('msg-char-count');

  input.addEventListener('input', () => {
    const len = input.value.length;
    charCnt.textContent = len;
    const peer = state.activePeerId ? state.peers.get(state.activePeerId) : null;
    sendBtn.disabled = len === 0 || !peer || peer.status !== 'connected';
  });

  // Auto-resize textarea
  input.addEventListener('input', () => {
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 110) + 'px';
  });

  // Ctrl+Enter untuk kirim
  input.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      if (!sendBtn.disabled) sendMessage();
    }
    // Escape → batal reply
    if (e.key === 'Escape' && state.replyTo) {
      e.preventDefault();
      clearReplyTo();
    }
  });

  sendBtn.addEventListener('click', sendMessage);

  // Cancel reply button
  document.getElementById('reply-bar-cancel')?.addEventListener('click', clearReplyTo);
}

async function sendMessage() {
  const input  = document.getElementById('msg-input');
  const text   = input.value.trim();
  if (!text || !state.activePeerId) return;

  const peer = state.peers.get(state.activePeerId);
  if (!peer || peer.status !== 'connected') {
    showToast('Peer belum terhubung', 'warning');
    return;
  }

  input.value = '';
  input.style.height = 'auto';
  document.getElementById('msg-char-count').textContent = '0';
  document.getElementById('send-btn').disabled = true;

  const currentReply = state.replyTo;
  clearReplyTo();

  try {
    await ipc('send_dm', {
      recipientId: state.activePeerId,
      plaintext: text,
      replyToId:   currentReply ? currentReply.id   : null,
      replyToText: currentReply ? currentReply.text : null,
    });
  } catch (err) {
    showToast('Gagal kirim: ' + err, 'error');
    input.value = text;
    if (currentReply) setReplyTo(currentReply.id, currentReply.text);
  }
}

// ── Helper: Messages ───────────────────────────────────────────────────────

function setReplyTo(id, text) {
  state.replyTo = { id, text };
  const bar    = document.getElementById('reply-bar');
  const textEl = document.getElementById('reply-bar-text');
  if (bar)    bar.classList.remove('hidden');
  if (textEl) textEl.textContent = text.length > 90 ? text.substring(0, 90) + '…' : text;
  document.getElementById('msg-input')?.focus();
}

function clearReplyTo() {
  state.replyTo = null;
  document.getElementById('reply-bar')?.classList.add('hidden');
}

function appendMessage(peerId, msg) {
  if (!state.messages.has(peerId)) state.messages.set(peerId, []);
  state.messages.get(peerId).push(msg);

  // Render jika peer ini sedang aktif
  if (state.activePeerId === peerId) {
    renderMessages(peerId);
  }
}

function renderMessages(peerId) {
  const scroll   = document.getElementById('messages-scroll');
  const msgs     = state.messages.get(peerId) || [];
  const wasAtBottom = scroll.scrollTop + scroll.clientHeight >= scroll.scrollHeight - 20;

  // Render hanya pesan baru (append saja)
  // Untuk sederhana, re-render semua
  scroll.innerHTML = '';

  if (msgs.length === 0) {
    scroll.innerHTML = `
      <div class="empty-state" style="padding-top:60px;">
        <div class="empty-state-icon">🔒</div>
        <div class="empty-state-text">Belum ada pesan</div>
        <small>Mulai percakapan terenkripsi pertama Anda</small>
      </div>
    `;
    return;
  }

  let lastDate = '';

  msgs.forEach((msg, idx) => {
    const date = new Date(msg.timestamp * 1000).toLocaleDateString('id-ID');

    if (date !== lastDate) {
      const sep = document.createElement('div');
      sep.className = 'date-separator';
      sep.textContent = date;
      scroll.appendChild(sep);
      lastDate = date;
    }

    const msgId = msg.id || `${msg.timestamp}_${idx}`;

    const wrapper = document.createElement('div');
    wrapper.className = `msg-wrapper ${msg.outgoing ? 'outgoing' : 'incoming'}`;
    wrapper.dataset.msgId = msgId;

    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';

    // Quoted block (DOM, bukan innerHTML)
    if (msg.replyToText) {
      const quoted = document.createElement('div');
      quoted.className = 'msg-quoted';
      const quotedText = document.createElement('div');
      quotedText.className = 'msg-quoted-text';
      const preview = msg.replyToText.length > 80
        ? msg.replyToText.substring(0, 80) + '…'
        : msg.replyToText;
      quotedText.textContent = preview;
      quoted.appendChild(quotedText);
      bubble.appendChild(quoted);
    }

    const msgText = document.createElement('div');
    msgText.className = 'msg-text';
    msgText.textContent = msg.text;

    const msgMeta = document.createElement('div');
    msgMeta.className = 'msg-meta';
    const timeSpan = document.createElement('span');
    timeSpan.textContent = formatTimestamp(msg.timestamp);
    const lockSpan = document.createElement('span');
    lockSpan.textContent = '🔒';
    msgMeta.appendChild(timeSpan);
    msgMeta.appendChild(lockSpan);

    bubble.appendChild(msgText);
    bubble.appendChild(msgMeta);

    // Klik bubble → set sebagai reply target
    bubble.addEventListener('click', () => setReplyTo(msgId, msg.text));

    wrapper.appendChild(bubble);
    scroll.appendChild(wrapper);
  });

  if (wasAtBottom) scroll.scrollTop = scroll.scrollHeight;
}

// Muat history dari DB
async function loadMessageHistory(peerId) {
  try {
    // BUG #6 FIX: perbaiki parameter name — Rust expects peer_id (JS: peerId)
    const messages = await ipc('get_messages', { peerId: peerId, limit: 100 });
    if (messages && messages.length > 0) {
      state.messages.set(peerId, messages.map(m => ({
        text:      m.text || m.plaintext,
        timestamp: m.timestamp,
        outgoing:  m.isOutgoing !== undefined ? m.isOutgoing : (m.direction === 'outgoing' || m.outgoing),
      })));
      renderMessages(peerId);
    }
  } catch (err) {
    console.warn('Gagal load history:', err);
  }
}

// ── Utility Functions ──────────────────────────────────────────────────────

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function formatTimestamp(ts) {
  const d = new Date(ts * 1000);
  return d.toLocaleTimeString('id-ID', { hour: '2-digit', minute: '2-digit' });
}

function delay(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

// Warna avatar deterministic dari string (nodeId atau nama)
function getAvatarColor(str) {
  const colors = [
    'linear-gradient(135deg, #20C9D8, #20808D)',
    'linear-gradient(135deg, #2D5F9E, #4A9EBF)',
    'linear-gradient(135deg, #1db87a, #20808D)',
    'linear-gradient(135deg, #7B5EA7, #4A9EBF)',
    'linear-gradient(135deg, #D97B2A, #e5a832)',
    'linear-gradient(135deg, #d94f4f, #D97B2A)',
    'linear-gradient(135deg, #20808D, #2D5F9E)',
    'linear-gradient(135deg, #4A9EBF, #1db87a)',
  ];
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = (hash * 31 + str.charCodeAt(i)) | 0;
  }
  return colors[Math.abs(hash) % colors.length];
}

function findPeerByIp(ip) {
  let found = null;
  state.peers.forEach((peer, nodeId) => {
    if (peer.ip === ip) found = nodeId;
  });
  return found;
}

function copyText(text, successMsg = 'Disalin!') {
  if (!text || text === '—') return;
  navigator.clipboard.writeText(text)
    .then(() => showToast(successMsg, 'success'))
    .catch(() => showToast('Gagal menyalin', 'error'));
}

// ── 15. Emergency Mode — Komunikasi Saat Mati Lampu ──────────────────────

/**
 * Tampilkan banner darurat di atas aplikasi.
 * @param {string} title - Judul banner
 * @param {string} subtitle - Subtitle / deskripsi
 */
function showEmergencyBanner(title, subtitle) {
  const banner = document.getElementById('emergency-banner');
  if (!banner) return;
  document.getElementById('emergency-banner-title').textContent = title || 'Jaringan Terputus';
  document.getElementById('emergency-banner-sub').textContent   = subtitle || 'Mati lampu? Aktifkan Mode Darurat.';
  banner.classList.remove('hidden');

  // Tambah class ke view-app untuk visual tint
  document.getElementById('view-app')?.classList.add('emergency-active');

  // Update status bar
  const netText = document.getElementById('home-net-text');
  if (netText) netText.textContent = '⚡ Jaringan Terputus';
}

function hideEmergencyBanner() {
  document.getElementById('emergency-banner')?.classList.add('hidden');
  document.getElementById('view-app')?.classList.remove('emergency-active');
}

/** Buka modal Emergency Mode dan refresh status */
async function openEmergencyModal() {
  document.getElementById('modal-emergency').classList.remove('hidden');
  await refreshEmergencyStatus();
}

/** Refresh status di dalam modal emergency */
async function refreshEmergencyStatus() {
  try {
    const status = await ipc('get_emergency_status');
    document.getElementById('em-net-state').textContent    = status.networkState || '—';
    document.getElementById('em-hotspot-state').textContent =
      status.hotspotActive ? `✅ Aktif (${status.hotspotSsid})` : '❌ Tidak aktif';
    document.getElementById('em-peer-count').textContent   = String(status.connectedPeers || 0);
  } catch (e) {
    console.warn('[Emergency] Gagal refresh status:', e);
  }
}

/** Setup semua interaksi Emergency Mode */
function setupEmergencyMode() {
  // Tombol di banner → buka modal
  document.getElementById('btn-emergency-host')?.addEventListener('click', () => {
    openEmergencyModal();
    // Scroll ke pilihan host
    setTimeout(() => document.getElementById('choice-host')?.scrollIntoView({ behavior: 'smooth' }), 200);
  });

  document.getElementById('btn-emergency-join')?.addEventListener('click', () => {
    openEmergencyModal();
    setTimeout(() => document.getElementById('choice-join')?.scrollIntoView({ behavior: 'smooth' }), 200);
  });

  document.getElementById('btn-emergency-dismiss')?.addEventListener('click', hideEmergencyBanner);

  // Quick action button di Home
  document.getElementById('qa-emergency')?.addEventListener('click', openEmergencyModal);

  // Tutup modal
  document.getElementById('close-emergency-modal')?.addEventListener('click', () => {
    document.getElementById('modal-emergency').classList.add('hidden');
  });

  // ── Activate Hotspot ────────────────────────────────────────────────────
  document.getElementById('btn-activate-hotspot')?.addEventListener('click', async () => {
    const btn = document.getElementById('btn-activate-hotspot');
    btn.disabled = true;
    btn.textContent = '⏳ Mengaktifkan...';

    try {
      const result = await ipc('activate_emergency_hotspot');
      showToast(result, 'success', 6000);
      pushNotif('warn', '⚡', `<strong>Hotspot darurat aktif</strong>: CARAKA-Emergency (tanpa password)`);
      await refreshEmergencyStatus();

      // Update banner jadi "Hotspot Aktif"
      showEmergencyBanner(
        '⚡ Hotspot Darurat Aktif',
        'SSID: CARAKA-Emergency (tanpa password) — Tunggu rekan terhubung'
      );
    } catch (err) {
      // Tampilkan instruksi manual fallback
      document.getElementById('emergency-manual-note').style.display = 'block';
      showToast('Hotspot gagal — lihat instruksi manual di bawah', 'warning', 8000);
      console.warn('[Emergency] Hotspot gagal:', err);
    } finally {
      btn.disabled = false;
      btn.textContent = '📶 Aktifkan Hotspot';
    }
  });

  // ── Open WiFi Settings ──────────────────────────────────────────────────
  document.getElementById('btn-open-wifi-settings')?.addEventListener('click', async () => {
    try {
      // Buka ms-settings:network-wifi via shell
      await ipc('add_peer_manual', { ip: '0.0.0.0', port: 0 })
        .catch(() => {}); // Ignore error, hanya trigger shell

      // Alternatif: gunakan window.open dengan ms-settings scheme
      // (tidak selalu berhasil di webview)
      showToast('Buka WiFi Settings di Windows, konek ke CARAKA-Emergency', 'info', 6000);
    } catch (_) {
      showToast('Buka Settings > WiFi > CARAKA-Emergency', 'info', 6000);
    }
  });

  // ── Scan Emergency Network ──────────────────────────────────────────────
  document.getElementById('btn-scan-emergency')?.addEventListener('click', async () => {
    const btn = document.getElementById('btn-scan-emergency');
    btn.disabled = true;
    btn.textContent = '🔍 Scanning 192.168.137.x...';

    showToast('Scanning subnet 192.168.137.x untuk peer CARAKA...', 'info', 3000);

    try {
      const foundIps = await ipc('scan_emergency_network');
      if (foundIps.length === 0) {
        showToast('Tidak ada peer ditemukan. Pastikan terhubung ke CARAKA-Emergency WiFi.', 'warning', 6000);
      } else {
        showToast(`✅ ${foundIps.length} peer ditemukan — mencoba terhubung...`, 'success', 5000);
        pushNotif('peer', '🔍', `Scan darurat: <strong>${foundIps.length} peer</strong> ditemukan di subnet hotspot`);
      }
      await refreshEmergencyStatus();
    } catch (err) {
      showToast('Scan gagal: ' + err, 'error');
    } finally {
      btn.disabled = false;
      btn.textContent = '🔍 Cari Peer Darurat';
    }
  });

  // ── Reconnect Known Peers ───────────────────────────────────────────────
  document.getElementById('btn-reconnect-known')?.addEventListener('click', async () => {
    const btn = document.getElementById('btn-reconnect-known');
    btn.disabled = true;
    btn.textContent = '⏳ Mencoba reconnect...';

    try {
      const count = await ipc('reconnect_known_peers');
      if (count === 0) {
        showToast('Tidak ada peer terkenal untuk di-reconnect', 'warning');
      } else {
        showToast(`Mencoba reconnect ke ${count} peer terkenal...`, 'info', 4000);
        pushNotif('system', '↩', `Mencoba <strong>reconnect ${count} peer</strong> dari sesi sebelumnya`);
      }
    } catch (err) {
      showToast('Reconnect gagal: ' + err, 'error');
    } finally {
      btn.disabled = false;
      btn.textContent = '↺ Reconnect Semua Peer Terkenal';
    }
  });
}

/** Setup event listeners untuk Emergency Mode events dari backend */
async function setupEmergencyEventListeners() {
  // Jaringan hilang
  await on('network_lost', (event) => {
    const d = event.payload;
    console.log('[Emergency] Network lost:', d);
    showEmergencyBanner('⚡ Jaringan Terputus', d.message || 'Router mati? Aktifkan Mode Darurat.');
    pushNotif('warn', '⚡', `<strong>Jaringan terputus</strong> — ${d.message || 'Aktifkan Mode Darurat'}`);

    // Update status network di UI
    const netText = document.getElementById('home-net-text');
    if (netText) netText.textContent = '⚡ Terputus';

    const netDot = document.getElementById('home-net-dot');
    if (netDot) {
      netDot.style.background = '#f97316';
      netDot.style.boxShadow  = '0 0 8px rgba(249,115,22,0.8)';
    }
  });

  // Jaringan kembali
  await on('network_restored', (event) => {
    const d = event.payload;
    console.log('[Emergency] Network restored:', d);
    hideEmergencyBanner();
    showToast('✅ ' + (d.message || 'Jaringan kembali normal'), 'success', 5000);
    pushNotif('system', '✅', `<strong>Jaringan pulih</strong> — Mode normal aktif kembali`);

    // Restore status dot
    const netDot = document.getElementById('home-net-dot');
    if (netDot) {
      netDot.style.background = '';
      netDot.style.boxShadow  = '';
      netDot.className = 'status-dot searching';
    }
  });

  // Emergency mode aktif (hotspot terdeteksi)
  await on('emergency_mode_active', (event) => {
    const d = event.payload;
    showEmergencyBanner('⚡ Mode Darurat Aktif', d.message || 'Hotspot darurat aktif');
    pushNotif('warn', '📶', `<strong>Mode Darurat aktif</strong> — ${d.message || ''}`);
  });

  // Hotspot berhasil diaktifkan
  await on('emergency_hotspot_started', (event) => {
    const d = event.payload;
    showToast(`Hotspot '${d.ssid}' aktif — rekan bisa konek tanpa password`, 'success', 8000);
    showEmergencyBanner('📶 Hotspot Aktif', `SSID: ${d.ssid} (tanpa password) — Menunggu rekan terhubung`);
  });

  // Hotspot perlu manual
  await on('emergency_hotspot_manual_needed', (event) => {
    const d = event.payload;
    document.getElementById('emergency-manual-note').style.display = 'block';
    showToast('Buka Windows Settings → Mobile Hotspot secara manual', 'warning', 8000);
  });

  // Hotspot dimatikan
  await on('emergency_hotspot_stopped', () => {
    hideEmergencyBanner();
    showToast('Hotspot darurat dimatikan', 'info');
  });
}

// ── Bootstrap ──────────────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', () => {
  const statusEl = document.getElementById('splash-status');

  // Tauri API detection — sets _invoke and _listen
  function detectTauriAPI() {
    if (window.__TAURI__?.core?.invoke) {
      _invoke = (...a) => window.__TAURI__.core.invoke(...a);
      _listen = window.__TAURI__.event?.listen
        ? (...a) => window.__TAURI__.event.listen(...a)
        : null;
      return true;
    }
    if (window.__TAURI_INTERNALS__?.invoke) {
      _invoke = (...a) => window.__TAURI_INTERNALS__.invoke(...a);
      _listen = window.__TAURI_INTERNALS__.listen
        ? (...a) => window.__TAURI_INTERNALS__.listen(...a)
        : null;
      return true;
    }
    if (window.__TAURI__?.invoke) {
      _invoke = (...a) => window.__TAURI__.invoke(...a);
      _listen = window.__TAURI__.event?.listen
        ? (...a) => window.__TAURI__.event.listen(...a)
        : null;
      return true;
    }
    return false;
  }

  // Sequential: Tunggu Tauri API terdeteksi, LALU jalankan initApp
  async function waitForTauriAndStart() {
    if (statusEl) statusEl.textContent = 'Mendeteksi runtime...';

    // Coba deteksi API (max 8 detik, poll setiap 100ms)
    for (let i = 0; i < 80; i++) {
      if (detectTauriAPI()) {
        console.log('[CARAKA] Tauri API terdeteksi pada percobaan ke-' + (i + 1));
        break;
      }
      await new Promise(r => setTimeout(r, 100));
    }

    if (!_invoke) {
      if (statusEl) statusEl.textContent = 'Tauri runtime tidak ditemukan';
      console.error('[CARAKA] Tauri API not found. __TAURI__:', window.__TAURI__);
      return;
    }

    // API siap — mulai init
    if (statusEl) statusEl.textContent = 'Menginisialisasi node...';
    try {
      await initApp();
    } catch (err) {
      console.error('[CARAKA] initApp fatal error:', err);
      if (statusEl) statusEl.textContent = 'Error: ' + String(err).substring(0, 80);
    }
  }

  waitForTauriAndStart();
});

// ═══════════════════════════════════════════════════════════════════════════
// F0 — Tor Status Chip
// ═══════════════════════════════════════════════════════════════════════════

function updateTorStatusChip(status, onionAddress) {
  const icon = document.getElementById('tor-status-icon');
  const text = document.getElementById('tor-status-text');
  const card = document.getElementById('card-onion');
  if (!icon || !text) return;

  if (status === 'bootstrapping') {
    icon.style.background = 'linear-gradient(135deg,#6b46c1,#553c9a)';
    icon.textContent = '⏳';
    text.textContent = 'Bootstrap...';
  } else if (status === 'ready') {
    icon.style.background = 'linear-gradient(135deg,#7c3aed,#5b21b6)';
    icon.textContent = '🧅';
    text.textContent = onionAddress
      ? onionAddress.substring(0, 16) + '…'
      : 'Siap';
    if (onionAddress && card) {
      card.style.display = '';
      const addr = document.getElementById('card-onion-addr');
      if (addr) addr.textContent = onionAddress;
    }
    state.onionAddress = onionAddress || null;
    pushNotif('system', '🧅', `Tor siap — <strong>${onionAddress ? onionAddress.substring(0,20) + '…' : 'onion address aktif'}</strong>`);
  } else if (status === 'failed') {
    icon.style.background = 'linear-gradient(135deg,#991b1b,#7f1d1d)';
    icon.textContent = '✕';
    text.textContent = 'Tor gagal';
    const chip = document.getElementById('tor-status-chip');
    if (chip) chip.title = onionAddress || 'Periksa firewall / koneksi internet';
    pushNotif('system', '⚠️', 'Tor gagal — fitur mesh via internet tidak tersedia. Pesan LAN tetap berjalan.');
  }
}

// ═══════════════════════════════════════════════════════════════════════════
// F6 — Invite Code Modal
// ═══════════════════════════════════════════════════════════════════════════

function setupInviteModal() {
  const openBtn   = document.getElementById('invite-code-btn');
  const closeBtn  = document.getElementById('close-invite-modal');
  const cancelBtn = document.getElementById('cancel-invite');
  const copyBtn   = document.getElementById('copy-invite-btn');
  const connectBtn = document.getElementById('connect-from-invite');
  const modal     = document.getElementById('modal-invite');
  const display   = document.getElementById('invite-code-display');
  const viaBadge  = document.getElementById('invite-via-badge');
  const pasteInput = document.getElementById('paste-invite-input');
  const parseResult = document.getElementById('invite-parse-result');

  if (!openBtn) return;

  openBtn.addEventListener('click', async () => {
    modal.classList.remove('hidden');
    display.textContent = '⏳ Membuat kode...';
    if (viaBadge) viaBadge.textContent = '';
    if (pasteInput) pasteInput.value = '';
    if (parseResult) { parseResult.textContent = ''; parseResult.classList.add('hidden'); }

    try {
      const code = await ipc('generate_invite_code');
      display.textContent = code;
      // Decode untuk tampilkan via-type
      const raw = atob(code.replace(/-/g, '+').replace(/_/g, '/'));
      if (viaBadge) {
        viaBadge.textContent = raw.startsWith('caraka1:') ? '🧅 via Tor' : '📡 via LAN';
        viaBadge.className   = 'invite-via-badge ' + (raw.startsWith('caraka1:') ? 'tor' : 'lan');
      }
    } catch (e) {
      display.textContent = 'Gagal membuat kode: ' + e;
    }
  });

  const closeModal = () => modal.classList.add('hidden');
  closeBtn?.addEventListener('click', closeModal);
  cancelBtn?.addEventListener('click', closeModal);
  modal?.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });

  copyBtn?.addEventListener('click', () => {
    const code = display.textContent;
    if (code && !code.startsWith('⏳') && !code.startsWith('Gagal')) {
      copyText(code, 'Kode undangan disalin!');
    }
  });

  // Parse kode yang ditempel
  pasteInput?.addEventListener('input', async () => {
    const code = pasteInput.value.trim();
    if (!code || !parseResult) return;

    try {
      const info = await ipc('parse_invite_code', { code });
      parseResult.classList.remove('hidden');
      parseResult.textContent = '';
      const span = document.createElement('span');
      span.className = 'parse-ok';
      if (info.via === 'tor') {
        span.textContent = `🧅 Tor — Node: ${escapeHtml(info.nodeId).substring(0, 16)}…`;
      } else {
        span.textContent = `📡 LAN — ${escapeHtml(String(info.ip))}:${escapeHtml(String(info.port))} — Node: ${escapeHtml(info.nodeId).substring(0, 16)}…`;
      }
      parseResult.appendChild(span);
      parseResult.dataset.info = JSON.stringify(info);
    } catch {
      parseResult.classList.remove('hidden');
      parseResult.textContent = '';
      const span = document.createElement('span');
      span.className = 'parse-err';
      span.textContent = 'Kode tidak valid';
      parseResult.appendChild(span);
      delete parseResult.dataset.info;
    }
  });

  connectBtn?.addEventListener('click', async () => {
    const raw = parseResult?.dataset.info;
    if (!raw) { showToast('Tempel kode yang valid dulu', 'warning'); return; }

    try {
      const info = JSON.parse(raw);
      if (info.via === 'lan') {
        await ipc('add_peer_manual', { ip: info.ip, port: info.port });
        showToast(`Menghubungkan ke ${info.ip}:${info.port}…`, 'info');
      } else {
        const msg = await ipc('connect_via_tor', {
          onionAddress: info.onionAddress,
          nodeId: info.nodeId,
        });
        showToast(`🧅 ${msg} Dial bisa butuh 1-2 menit.`, 'info', 8000);
      }
      closeModal();
    } catch (e) {
      showToast('Gagal connect: ' + e, 'error');
    }
  });
}

// ═══════════════════════════════════════════════════════════════════════════
// F2 — File Transfer UI + F4 — Image Preview
// ═══════════════════════════════════════════════════════════════════════════

function setupFileAttach() {
  const attachBtn  = document.getElementById('attach-btn');
  const fileInput  = document.getElementById('file-input');
  if (!attachBtn || !fileInput) return;

  attachBtn.addEventListener('click', () => {
    if (!state.activePeerId) { showToast('Pilih peer dulu', 'warning'); return; }
    const peer = state.peers.get(state.activePeerId);
    if (!peer || peer.status !== 'connected') { showToast('Peer belum terhubung', 'warning'); return; }
    fileInput.click();
  });

  fileInput.addEventListener('change', async () => {
    const file = fileInput.files[0];
    if (!file) return;
    fileInput.value = '';

    const MAX_MB = 5;
    if (file.size > MAX_MB * 1024 * 1024) {
      showToast(`File terlalu besar (maks ${MAX_MB} MB)`, 'error');
      return;
    }

    showToast(`Mengirim ${file.name}…`, 'info');
    try {
      // Tauri path picker tidak tersedia di web — gunakan path dari File object
      // Di Tauri v2 desktop, window.__TAURI__.path menyediakan pathFrom
      let filePath = file.path || (window.__TAURI__?.path ? await window.__TAURI__.path.resolve(file.name) : null);

      if (!filePath) {
        // Fallback: baca sebagai ArrayBuffer dan kirim via virtual path
        showToast('Kirim file via path tidak tersedia di mode ini', 'warning');
        return;
      }

      const transferId = await ipc('send_file', {
        recipientId: state.activePeerId,
        filePath,
      });
      showToast(`File terkirim: ${file.name}`, 'success');
    } catch (e) {
      showToast('Gagal kirim file: ' + e, 'error');
    }
  });
}

// Tambahkan listener file_received & file_sent ke setupEventListeners
// (dipanggil terpisah agar tidak memodifikasi fungsi yang sudah ada)
async function setupFileEventListeners() {
  await on('file_sent', (event) => {
    const d = event.payload;
    if (!d) return;
    const sizeKb = Math.round((d.fileSize || 0) / 1024);
    appendMessage(d.recipientId, {
      id:        `file_${d.transferId}`,
      text:      `📎 ${d.filename} (${sizeKb} KB)`,
      fileInfo:  d,
      timestamp: d.timestamp || Math.floor(Date.now() / 1000),
      outgoing:  true,
    });
  });

  await on('file_received', (event) => {
    const d = event.payload;
    if (!d) return;
    const sizeKb = Math.round((d.fileSize || 0) / 1024);
    appendMessage(d.senderId, {
      id:        `file_${d.transferId}`,
      text:      `📎 ${d.filename} (${sizeKb} KB)`,
      fileInfo:  d,
      timestamp: d.timestamp || Math.floor(Date.now() / 1000),
      outgoing:  false,
    });
    const sender = state.peers.get(d.senderId);
    const name   = sender?.displayName || d.senderId?.substring(0, 8) || '?';
    pushNotif('msg', '📎', `<strong>${escapeHtml(name)}</strong> mengirim file: ${escapeHtml(d.filename)}`);
    showToast(`File diterima: ${d.filename} → ${d.savedPath}`, 'success', 7000);
  });

  await on('tor_status', (event) => {
    const d = event.payload;
    if (!d) return;
    updateTorStatusChip(d.status, d.onionAddress || d.error || null);
  });

  // Sync status Tor segera setelah listener terdaftar —
  // event "bootstrapping" mungkin sudah lewat sebelum listener aktif.
  try {
    const torNow = await ipc('get_tor_status');
    if (torNow) updateTorStatusChip(torNow.status, torNow.onionAddress || torNow.error || null);
  } catch (_) { /* node belum siap, tidak masalah */ }

  // Tangani packet tipe File dari transport
  const origHandleIncomingPacket = window._origHandleIncomingPacket || handleIncomingPacket;
  window._carakaHandlePacket = async function(d) {
    if (d.packetType === 8) {
      try {
        await ipc('try_decrypt_file_packet', {
          packetId:     d.packetId,
          nonceHex:     d.nonce,
          ciphertextHex: d.ciphertext,
          aeadTagHex:   d.aeadTag,
        });
      } catch (err) {
        console.warn('[CARAKA] Gagal dekripsi file packet:', err);
      }
    } else {
      await origHandleIncomingPacket(d);
    }
  };
}

// ── Patch renderMessages untuk F4 Image Preview ─────────────────────────────

const _origRenderMessages = renderMessages;
function renderMessages(peerId) {
  const scroll = document.getElementById('messages-scroll');
  const msgs   = state.messages.get(peerId) || [];
  const wasAtBottom = scroll.scrollTop + scroll.clientHeight >= scroll.scrollHeight - 20;

  scroll.innerHTML = '';

  if (msgs.length === 0) {
    scroll.innerHTML = `
      <div class="empty-state" style="padding-top:60px;">
        <div class="empty-state-icon">🔒</div>
        <div class="empty-state-text">Belum ada pesan</div>
        <small>Mulai percakapan terenkripsi pertama Anda</small>
      </div>`;
    return;
  }

  let lastDate = '';

  msgs.forEach((msg, idx) => {
    const date = new Date(msg.timestamp * 1000).toLocaleDateString('id-ID');
    if (date !== lastDate) {
      const sep = document.createElement('div');
      sep.className = 'date-separator';
      sep.textContent = date;
      scroll.appendChild(sep);
      lastDate = date;
    }

    const msgId = msg.id || `${msg.timestamp}_${idx}`;
    const wrapper = document.createElement('div');
    wrapper.className = `msg-wrapper ${msg.outgoing ? 'outgoing' : 'incoming'}`;
    wrapper.dataset.msgId = msgId;

    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';

    // Quoted block
    if (msg.replyToText) {
      const quoted = document.createElement('div');
      quoted.className = 'msg-quoted';
      const quotedText = document.createElement('div');
      quotedText.className = 'msg-quoted-text';
      quotedText.textContent = msg.replyToText.length > 80
        ? msg.replyToText.substring(0, 80) + '…'
        : msg.replyToText;
      quoted.appendChild(quotedText);
      bubble.appendChild(quoted);
    }

    // F4: Image preview jika fileInfo + isImage
    if (msg.fileInfo && msg.fileInfo.isImage && msg.fileInfo.savedPath) {
      const imgWrap = document.createElement('div');
      imgWrap.className = 'msg-image-wrap';
      const img = document.createElement('img');
      // Tauri asset protocol: gunakan convertFileSrc jika tersedia
      if (window.__TAURI__?.core?.convertFileSrc) {
        img.src = window.__TAURI__.core.convertFileSrc(msg.fileInfo.savedPath);
      } else {
        img.src = 'file://' + msg.fileInfo.savedPath.replace(/\\/g, '/');
      }
      img.className = 'msg-image-preview';
      img.alt = msg.fileInfo.filename || 'gambar';
      img.loading = 'lazy';
      imgWrap.appendChild(img);
      bubble.appendChild(imgWrap);
    }

    // File attachment bubble (non-image)
    if (msg.fileInfo && !msg.fileInfo.isImage) {
      const fileChip = document.createElement('div');
      fileChip.className = 'msg-file-chip';
      const fileIcon = document.createElement('span');
      fileIcon.textContent = '📎';
      const fileName = document.createElement('span');
      fileName.className = 'msg-file-name';
      fileName.textContent = msg.fileInfo.filename || msg.text;
      const fileSize = document.createElement('span');
      fileSize.className = 'msg-file-size';
      fileSize.textContent = msg.fileInfo.fileSize
        ? Math.round(msg.fileInfo.fileSize / 1024) + ' KB'
        : '';
      fileChip.appendChild(fileIcon);
      fileChip.appendChild(fileName);
      fileChip.appendChild(fileSize);
      if (msg.fileInfo.savedPath) {
        fileChip.title = 'Disimpan: ' + msg.fileInfo.savedPath;
        fileChip.style.cursor = 'pointer';
      }
      bubble.appendChild(fileChip);
    } else if (!msg.fileInfo) {
      // Pesan teks biasa
      const msgText = document.createElement('div');
      msgText.className = 'msg-text';
      msgText.textContent = msg.text;
      bubble.appendChild(msgText);
    } else if (msg.fileInfo && msg.fileInfo.isImage) {
      // Untuk gambar, tampilkan nama file sebagai caption kecil
      const cap = document.createElement('div');
      cap.className = 'msg-image-caption';
      cap.textContent = msg.fileInfo.filename || '';
      bubble.appendChild(cap);
    }

    const msgMeta = document.createElement('div');
    msgMeta.className = 'msg-meta';
    const timeSpan = document.createElement('span');
    timeSpan.textContent = formatTimestamp(msg.timestamp);
    const lockSpan = document.createElement('span');
    lockSpan.textContent = '🔒';
    msgMeta.appendChild(timeSpan);
    msgMeta.appendChild(lockSpan);
    bubble.appendChild(msgMeta);

    bubble.addEventListener('click', () => {
      if (!msg.fileInfo) setReplyTo(msgId, msg.text);
    });

    wrapper.appendChild(bubble);
    scroll.appendChild(wrapper);
  });

  if (wasAtBottom) scroll.scrollTop = scroll.scrollHeight;
}

// Override handleIncomingPacket agar mendukung file packet
const _origHandleIncomingPacket = handleIncomingPacket;
async function handleIncomingPacket(d) {
  if (d.packetType === 8) {
    try {
      await ipc('try_decrypt_file_packet', {
        packetId:      d.packetId,
        nonceHex:      d.nonce,
        ciphertextHex: d.ciphertext,
        aeadTagHex:    d.aeadTag,
      });
    } catch (err) {
      console.warn('[CARAKA] Gagal dekripsi file packet:', err);
    }
  } else {
    await _origHandleIncomingPacket(d);
  }
}

})(); // end IIFE
