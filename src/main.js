// src/main.js
// CARAKA Desktop — Frontend Application Logic
// Tauri v2: gunakan window.__TAURI_INTERNALS__ yang selalu tersedia di WebView

// ─── Tauri v2 API Wrappers ──────────────────────────────────────────────────

// invoke: panggil Tauri command dari Rust backend
function invoke(cmd, args) {
  return window.__TAURI_INTERNALS__.invoke(cmd, args ?? {});
}

// listen: subscribe ke event dari Rust backend
async function listen(event, handler) {
  const internals = window.__TAURI_INTERNALS__;

  // Buat callback ID yang Tauri bisa panggil dari Rust
  const callbackId = internals.transformCallback((eventData) => {
    handler({ payload: eventData.payload ?? eventData });
  });

  // Register listener via plugin event IPC
  const eventId = await internals.invoke('plugin:event|listen', {
    event:   event,
    target:  { kind: 'Any' },
    handler: callbackId,
  });

  // Return unlisten function
  return function unlisten() {
    internals.invoke('plugin:event|unlisten', {
      event:   event,
      eventId: eventId,
    });
  };
}

// ─── State ─────────────────────────────────────────────────────────────────

const state = {
  myNodeId: null,
  myFingerprint: null,
  myDisplayName: 'User',
  selectedPeerId: null,
  peers: new Map(),    // nodeId -> { nodeId, displayName, isOnline, fingerprint, ip, port }
  messages: new Map(), // peerId -> [messages]
};

// ─── UI Helpers ────────────────────────────────────────────────────────────

function showToast(message, type = 'info', duration = 3000) {
  const container = document.getElementById('toast-container');
  const toast = document.createElement('div');
  toast.className = `toast ${type}`;
  toast.textContent = message;
  container.appendChild(toast);
  setTimeout(() => {
    toast.style.animation = 'toastOut 0.3s ease forwards';
    setTimeout(() => toast.remove(), 300);
  }, duration);
}

function getAvatarColor(str) {
  const colors = [
    'linear-gradient(135deg, #6366f1, #8b5cf6)',
    'linear-gradient(135deg, #ec4899, #8b5cf6)',
    'linear-gradient(135deg, #14b8a6, #6366f1)',
    'linear-gradient(135deg, #f59e0b, #ef4444)',
    'linear-gradient(135deg, #10b981, #14b8a6)',
    'linear-gradient(135deg, #3b82f6, #6366f1)',
  ];
  let hash = 0;
  for (const ch of str) hash = (hash << 5) - hash + ch.charCodeAt(0);
  return colors[Math.abs(hash) % colors.length];
}

function getAvatarLetter(name) { return (name || '?').charAt(0).toUpperCase(); }

function formatTime(timestamp) {
  return new Date(timestamp * 1000).toLocaleTimeString('id-ID', { hour: '2-digit', minute: '2-digit' });
}

function formatDate(timestamp) {
  const d = new Date(timestamp * 1000);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return 'Hari ini';
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) return 'Kemarin';
  return d.toLocaleDateString('id-ID', { day: 'numeric', month: 'long', year: 'numeric' });
}

function shortNodeId(nodeId) {
  if (!nodeId || nodeId.length < 16) return nodeId;
  return nodeId.slice(0, 8) + '...' + nodeId.slice(-8);
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/\n/g, '<br>');
}

// ─── Initialization ────────────────────────────────────────────────────────

async function initApp() {
  await setupBackendListeners();
  setupUIListeners();

  try {
    const info = await invoke('init_node');
    handleNodeReady({
      nodeId: info.node_id,
      fingerprint: info.fingerprint,
      tcpPort: info.tcp_port,
    });
  } catch (e) {
    console.log('Menunggu node_ready event...', e);
  }
}

// ─── Backend Event Listeners ───────────────────────────────────────────────

async function setupBackendListeners() {
  await listen('node_ready',            e => handleNodeReady(e.payload));
  await listen('node_error',            e => { console.error(e.payload); showToast('Error: ' + e.payload.error, 'error', 5000); });
  await listen('peer_discovered',       e => handlePeerDiscovered(e.payload));
  await listen('peer_handshaked',       e => handlePeerHandshaked(e.payload));
  await listen('peer_connected',        e => handlePeerConnected(e.payload));
  await listen('peer_disconnected',     e => handlePeerDisconnected(e.payload));
  await listen('clamp_packet_received', e => handleClampPacketReceived(e.payload));
  await listen('message_sent',          e => handleMessageSent(e.payload));
}

// ─── Event Handlers ────────────────────────────────────────────────────────

function handleNodeReady(payload) {
  state.myNodeId = payload.nodeId;
  state.myFingerprint = payload.fingerprint;

  // Update fingerprint display
  const fpEl = document.getElementById('fp-value');
  if (fpEl) fpEl.textContent = payload.fingerprint;

  const nodeShortEl = document.getElementById('my-node-id-short');
  if (nodeShortEl) nodeShortEl.textContent = shortNodeId(payload.nodeId);

  const settingsNodeEl = document.getElementById('settings-full-node-id');
  if (settingsNodeEl) settingsNodeEl.textContent = payload.nodeId;

  const settingsFpEl = document.getElementById('settings-fingerprint');
  if (settingsFpEl) settingsFpEl.textContent = payload.fingerprint;

  // Update avatar
  const avatar = document.getElementById('my-avatar');
  if (avatar) {
    avatar.textContent = getAvatarLetter(state.myDisplayName);
    avatar.style.background = getAvatarColor(state.myNodeId);
  }

  // Show app, hide loading
  document.getElementById('loading-overlay').classList.add('hidden');
  document.getElementById('app').classList.remove('hidden');

  updateStatusSearching();
  loadPeers();
  showToast('✅ Node siap! Fingerprint: ' + state.myFingerprint, 'success');
}

function handlePeerDiscovered(payload) {
  const { nodeId, displayName, ip, port } = payload;
  if (!state.peers.has(nodeId)) {
    state.peers.set(nodeId, {
      nodeId,
      displayName: displayName || 'Unknown',
      isOnline: false,
      fingerprint: nodeId.slice(0, 8),
      ip, port,
    });
    renderPeersList();
    showToast(`📡 Peer ditemukan: ${displayName || nodeId.slice(0, 8)}`, 'info');
  }
}

function handlePeerHandshaked(payload) {
  const { nodeId, displayName } = payload;
  if (state.peers.has(nodeId)) {
    state.peers.get(nodeId).displayName = displayName || state.peers.get(nodeId).displayName;
  } else {
    state.peers.set(nodeId, { nodeId, displayName: displayName || 'Unknown', isOnline: true, fingerprint: nodeId.slice(0, 8), ip: '', port: 7771 });
  }
  renderPeersList();
}

function handlePeerConnected(payload) {
  const { nodeId, ip } = payload;
  if (state.peers.has(nodeId)) {
    state.peers.get(nodeId).isOnline = true;
    state.peers.get(nodeId).ip = ip;
  } else {
    state.peers.set(nodeId, { nodeId, displayName: nodeId.slice(0, 8), isOnline: true, fingerprint: nodeId.slice(0, 8), ip, port: 7771 });
  }
  renderPeersList();
  updateNetworkStatus();
  if (state.selectedPeerId === nodeId) updateChatHeader(nodeId);
  showToast(`🟢 Peer terhubung: ${shortNodeId(nodeId)}`, 'success');
}

function handlePeerDisconnected(payload) {
  const { nodeId } = payload;
  if (state.peers.has(nodeId)) state.peers.get(nodeId).isOnline = false;
  renderPeersList();
  updateNetworkStatus();
  if (state.selectedPeerId === nodeId) updateChatHeader(nodeId);
  showToast(`🔴 Peer terputus: ${shortNodeId(nodeId)}`, 'warning');
}

async function handleClampPacketReceived(payload) {
  try {
    const result = await invoke('try_decrypt_packet', {
      packetId:     payload.packetId,
      nonceHex:     payload.nonce,
      ciphertextHex: payload.ciphertext,
      aeadTagHex:   payload.aeadTag,
    });
    if (result) {
      addIncomingMessage(result);
      showToast(`💬 Pesan baru dari ${shortNodeId(result.sender_id)}`, 'info');
    }
  } catch (e) {
    console.error('Error dekripsi paket:', e);
  }
}

function handleMessageSent(payload) {
  addOutgoingMessage({
    id: payload.id,
    sender_id: state.myNodeId,
    recipient_id: payload.recipientId,
    plaintext: payload.text,
    timestamp: payload.timestamp,
    is_outgoing: true,
  });
}

// ─── Message Rendering ─────────────────────────────────────────────────────

function addOutgoingMessage(msg) {
  if (!state.messages.has(msg.recipient_id)) state.messages.set(msg.recipient_id, []);
  state.messages.get(msg.recipient_id).push(msg);
  if (state.selectedPeerId === msg.recipient_id) appendMessageToDOM(msg);
}

function addIncomingMessage(msgInfo) {
  const msg = {
    id: msgInfo.id,
    sender_id: msgInfo.sender_id,
    recipient_id: state.myNodeId,
    plaintext: msgInfo.plaintext,
    timestamp: msgInfo.timestamp,
    is_outgoing: false,
  };
  if (!state.messages.has(msg.sender_id)) state.messages.set(msg.sender_id, []);
  state.messages.get(msg.sender_id).push(msg);
  if (state.selectedPeerId === msg.sender_id) appendMessageToDOM(msg);
}

function appendMessageToDOM(msg) {
  const scroll = document.getElementById('messages-scroll');
  const wrapper = document.createElement('div');
  wrapper.className = `message-wrapper ${msg.is_outgoing ? 'outgoing' : 'incoming'}`;
  const bubble = document.createElement('div');
  bubble.className = 'message-bubble';
  bubble.innerHTML = `
    <div class="message-text">${escapeHtml(msg.plaintext)}</div>
    <div class="message-time">
      <span class="message-encrypted-badge">🔒</span>
      ${formatTime(msg.timestamp)}
    </div>`;
  wrapper.appendChild(bubble);
  scroll.appendChild(wrapper);
  scroll.scrollTop = scroll.scrollHeight;
}

// ─── Peers List ────────────────────────────────────────────────────────────

function renderPeersList() {
  const list = document.getElementById('peers-list');
  if (state.peers.size === 0) {
    list.innerHTML = '<div class="empty-state small"><span>📡 Mencari peer di jaringan...</span></div>';
    return;
  }
  list.innerHTML = '';
  const sorted = [...state.peers.values()].sort((a, b) => {
    if (a.isOnline !== b.isOnline) return b.isOnline - a.isOnline;
    return a.displayName.localeCompare(b.displayName);
  });
  for (const peer of sorted) list.appendChild(createPeerItem(peer));
}

function createPeerItem(peer) {
  const item = document.createElement('div');
  item.className = `peer-item${state.selectedPeerId === peer.nodeId ? ' active' : ''}`;
  item.dataset.nodeId = peer.nodeId;
  const fp = peer.fingerprint || peer.nodeId.slice(0, 8);
  item.innerHTML = `
    <div class="peer-avatar">
      <div class="peer-avatar-inner" style="background: ${getAvatarColor(peer.nodeId)}">${getAvatarLetter(peer.displayName)}</div>
      <div class="peer-online-dot ${peer.isOnline ? 'online' : ''}"></div>
    </div>
    <div class="peer-details">
      <div class="peer-name">${escapeHtml(peer.displayName)}</div>
      <div class="peer-meta">
        <span class="peer-fp">🔑 ${fp}</span>
        <span class="peer-badge ${peer.isOnline ? 'online' : 'offline'}">${peer.isOnline ? '● Online' : '○ Offline'}</span>
      </div>
    </div>`;
  item.addEventListener('click', () => selectPeer(peer.nodeId));
  return item;
}

// ─── Chat ──────────────────────────────────────────────────────────────────

async function selectPeer(peerId) {
  state.selectedPeerId = peerId;
  document.querySelectorAll('.peer-item').forEach(el => el.classList.toggle('active', el.dataset.nodeId === peerId));
  document.getElementById('no-chat-state').classList.add('hidden');
  document.getElementById('chat-view').classList.remove('hidden');
  updateChatHeader(peerId);
  updateSendButton();
  await loadMessagesForPeer(peerId);
}

function updateChatHeader(peerId) {
  const peer = state.peers.get(peerId);
  if (!peer) return;
  const fp = peer.fingerprint || peerId.slice(0, 8);
  document.getElementById('chat-peer-avatar').textContent = getAvatarLetter(peer.displayName);
  document.getElementById('chat-peer-avatar').style.background = getAvatarColor(peerId);
  document.getElementById('chat-peer-name').textContent = peer.displayName;
  document.getElementById('chat-peer-status-dot').className = `peer-status-dot ${peer.isOnline ? 'online' : ''}`;
  document.getElementById('chat-peer-status-text').textContent = peer.isOnline ? 'Online' : 'Offline';
  document.getElementById('chat-peer-fp').textContent = `🔑 ${fp}`;
  document.getElementById('relay-info').textContent = peer.isOnline ? '' : '📦 Pesan disimpan, dikirim saat peer online';
}

async function loadMessagesForPeer(peerId) {
  const scroll = document.getElementById('messages-scroll');
  scroll.innerHTML = '<div class="empty-state"><span>Memuat riwayat pesan...</span></div>';
  try {
    const messages = await invoke('get_messages', { peerId, limit: 50 });
    scroll.innerHTML = '';
    if (messages.length === 0) {
      scroll.innerHTML = '<div class="empty-state"><span>🔒</span><span>Belum ada pesan. Kirim pesan pertama!</span></div>';
      return;
    }
    let lastDate = null;
    for (const msg of [...messages].reverse()) {
      const msgDate = formatDate(msg.timestamp);
      if (msgDate !== lastDate) {
        const sep = document.createElement('div');
        sep.className = 'date-separator';
        sep.textContent = msgDate;
        scroll.appendChild(sep);
        lastDate = msgDate;
      }
      const wrapper = document.createElement('div');
      wrapper.className = `message-wrapper ${msg.isOutgoing ? 'outgoing' : 'incoming'}`;
      const bubble = document.createElement('div');
      bubble.className = 'message-bubble';
      bubble.innerHTML = `
        <div class="message-text">${escapeHtml(msg.text)}</div>
        <div class="message-time">
          <span class="message-encrypted-badge">🔒</span>
          ${formatTime(msg.timestamp)}
          ${msg.decrypted ? '' : '<span style="color:rgba(255,100,100,0.7)">⚠</span>'}
        </div>`;
      wrapper.appendChild(bubble);
      scroll.appendChild(wrapper);
    }
    scroll.scrollTop = scroll.scrollHeight;
  } catch (e) {
    console.error('Error load messages:', e);
    scroll.innerHTML = '<div class="empty-state"><span>Error memuat pesan</span></div>';
  }
}

// ─── Network Status ────────────────────────────────────────────────────────

async function loadPeers() {
  try {
    const peers = await invoke('get_peers');
    for (const peer of peers) {
      state.peers.set(peer.node_id, {
        nodeId: peer.node_id,
        displayName: peer.display_name,
        isOnline: peer.is_online,
        fingerprint: peer.fingerprint,
        ip: peer.ip_address,
        port: peer.tcp_port,
      });
    }
    renderPeersList();
    updateNetworkStatus();
  } catch (e) {
    console.error('Error load peers:', e);
  }
}

async function updateNetworkStatus() {
  try {
    const status = await invoke('get_network_status');
    const dot = document.getElementById('status-dot');
    const text = document.getElementById('status-text');
    const count = document.getElementById('connected-count');
    if (count) count.textContent = status.connected_peers;
    if (status.connected_peers > 0) {
      dot.className = 'status-dot online';
      text.textContent = `${status.connected_peers} peer aktif`;
    } else {
      dot.className = 'status-dot searching';
      text.textContent = 'Mencari peer...';
    }
  } catch (e) {
    console.error('Error get status:', e);
  }
}

function updateStatusSearching() {
  const dot = document.getElementById('status-dot');
  const text = document.getElementById('status-text');
  if (dot) dot.className = 'status-dot searching';
  if (text) text.textContent = 'Mencari peer...';
}

// ─── Send Message ──────────────────────────────────────────────────────────

async function sendMessage() {
  const input = document.getElementById('message-input');
  const text = input.value.trim();
  if (!text || !state.selectedPeerId) return;

  const sendBtn = document.getElementById('send-btn');
  sendBtn.disabled = true;

  try {
    await invoke('send_dm', { recipientId: state.selectedPeerId, plaintext: text });
    input.value = '';
    updateCharCount();
    input.style.height = 'auto';
  } catch (e) {
    console.error('Error send:', e);
    showToast('Gagal kirim: ' + e, 'error');
  } finally {
    sendBtn.disabled = !document.getElementById('message-input').value.trim();
  }
}

// ─── UI Event Listeners ────────────────────────────────────────────────────

function setupUIListeners() {
  const messageInput = document.getElementById('message-input');

  messageInput.addEventListener('input', () => {
    updateCharCount();
    updateSendButton();
    autoResizeTextarea(messageInput);
  });

  messageInput.addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (!document.getElementById('send-btn').disabled) sendMessage();
    }
  });

  document.getElementById('send-btn').addEventListener('click', sendMessage);

  // Add peer modal
  document.getElementById('add-peer-btn').addEventListener('click', () =>
    document.getElementById('add-peer-modal').classList.remove('hidden'));
  document.getElementById('close-modal-btn').addEventListener('click', () =>
    document.getElementById('add-peer-modal').classList.add('hidden'));
  document.getElementById('cancel-modal-btn').addEventListener('click', () =>
    document.getElementById('add-peer-modal').classList.add('hidden'));
  document.getElementById('confirm-add-peer-btn').addEventListener('click', async () => {
    const ip = document.getElementById('peer-ip-input').value.trim();
    const port = parseInt(document.getElementById('peer-port-input').value) || 7771;
    if (!ip) { showToast('Masukkan IP address peer', 'warning'); return; }
    try {
      const result = await invoke('add_peer_manual', { ip, port });
      showToast(result, 'info');
      document.getElementById('add-peer-modal').classList.add('hidden');
      document.getElementById('peer-ip-input').value = '';
    } catch (e) { showToast('Error: ' + e, 'error'); }
  });

  // Settings modal
  document.getElementById('settings-btn').addEventListener('click', () => {
    document.getElementById('display-name-input').value = state.myDisplayName;
    document.getElementById('settings-modal').classList.remove('hidden');
  });
  document.getElementById('close-settings-btn').addEventListener('click', () =>
    document.getElementById('settings-modal').classList.add('hidden'));
  document.getElementById('cancel-settings-btn').addEventListener('click', () =>
    document.getElementById('settings-modal').classList.add('hidden'));
  document.getElementById('save-settings-btn').addEventListener('click', async () => {
    const name = document.getElementById('display-name-input').value.trim();
    if (!name) { showToast('Nama tidak boleh kosong', 'warning'); return; }
    try {
      await invoke('set_display_name', { name });
      state.myDisplayName = name;
      document.getElementById('my-display-name').textContent = name;
      const av = document.getElementById('my-avatar');
      if (av) av.textContent = getAvatarLetter(name);
      document.getElementById('settings-modal').classList.add('hidden');
      showToast('Nama berhasil diubah', 'success');
    } catch (e) { showToast('Error: ' + e, 'error'); }
  });

  // Copy Node ID
  document.getElementById('copy-node-id-btn').addEventListener('click', () => {
    if (state.myNodeId) {
      navigator.clipboard.writeText(state.myNodeId);
      showToast('Node ID disalin!', 'success');
    }
  });

  // Close modal on overlay click
  document.querySelectorAll('.modal-overlay').forEach(overlay => {
    overlay.addEventListener('click', e => {
      if (e.target === overlay) overlay.classList.add('hidden');
    });
  });

  // Periodic refresh
  setInterval(updateNetworkStatus, 10000);
  setInterval(loadPeers, 30000);
}

function updateCharCount() {
  const input = document.getElementById('message-input');
  const el = document.getElementById('char-count');
  if (el) el.textContent = `${input.value.length}/4096`;
}

function updateSendButton() {
  const input = document.getElementById('message-input');
  const sendBtn = document.getElementById('send-btn');
  sendBtn.disabled = !input.value.trim() || !state.selectedPeerId;
}

function autoResizeTextarea(textarea) {
  textarea.style.height = 'auto';
  textarea.style.height = Math.min(textarea.scrollHeight, 120) + 'px';
}

// ─── Entry Point ────────────────────────────────────────────────────────────

function waitForTauri(callback, retries = 50) {
  if (window.__TAURI_INTERNALS__) {
    callback();
  } else if (retries > 0) {
    setTimeout(() => waitForTauri(callback, retries - 1), 100);
  } else {
    const sub = document.querySelector('.loading-subtitle');
    if (sub) sub.textContent = 'Error: Tauri IPC tidak tersedia. Pastikan app dijalankan via cargo tauri dev';
  }
}

document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('my-display-name').textContent = state.myDisplayName;

  waitForTauri(async () => {
    try {
      await initApp();
    } catch (e) {
      console.error('Error inisialisasi:', e);
      const sub = document.querySelector('.loading-subtitle');
      if (sub) sub.textContent = 'Error: ' + (e.message || e);
    }
  });
});
