/**
 * session-client.js — browser client for LocalGPT collaborative sessions.
 *
 * Joins a hosted Gen session over WebSocket (spec:
 * docs/rfcs/multiplayer/collaborative-world-engine-architecture.md):
 * receives the world, applies committed ops live, shows peers as avatars,
 * and sends presence, chat and prompts.
 *
 * The host serves this file at /session-client.js next to the join page;
 * the page's importmap provides "three" and "./world-viewer.js" is served
 * at /world-viewer.js.
 */

import * as THREE from 'three';
import { createWorldViewer } from '/world-viewer.js';

const AVATAR_COLORS = [0x5b8dd9, 0xd95f5f, 0x5fd98a, 0xd9b45f, 0xa55fd9, 0x5fd1d9, 0xd95fa1, 0x8ed95f];

function el(id) { return document.getElementById(id); }

function escapeText(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function makeLabel(text) {
  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d');
  const font = '24px system-ui, sans-serif';
  ctx.font = font;
  const w = Math.ceil(ctx.measureText(text).width) + 24;
  canvas.width = w; canvas.height = 40;
  ctx.font = font;
  ctx.fillStyle = 'rgba(0,0,0,0.55)';
  ctx.beginPath(); ctx.roundRect(0, 0, w, 40, 8); ctx.fill();
  ctx.fillStyle = '#fff';
  ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
  ctx.fillText(text, w / 2, 21);
  const tex = new THREE.CanvasTexture(canvas);
  tex.colorSpace = THREE.SRGBColorSpace;
  const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: tex, depthTest: false }));
  sprite.scale.set(w / 160, 0.25, 1);
  return sprite;
}

function makeAvatar(peer) {
  const group = new THREE.Group();
  const color = AVATAR_COLORS[Number(peer.id) % AVATAR_COLORS.length];
  const body = new THREE.Mesh(
    new THREE.CapsuleGeometry(0.3, 0.9, 4, 12),
    new THREE.MeshStandardMaterial({ color, roughness: 0.6 })
  );
  body.position.y = 0.75;
  body.castShadow = true;
  const label = makeLabel(peer.name || `guest-${peer.id}`);
  label.position.y = 1.65;
  group.add(body, label);
  return group;
}

export function startSessionClient() {
  const joinOverlay = el('join-overlay');
  const nameInput = el('name-input');
  const joinBtn = el('join-btn');
  const joinError = el('join-error');
  const hudSession = el('hud-session');
  const hudPeers = el('hud-peers');
  const statusEl = el('status');
  const chatLog = el('chat-log');
  const chatInput = el('chat-input');
  const promptInput = el('prompt-input');

  nameInput.value = localStorage.getItem('localgpt.session.name') || '';

  // Token rides the URL fragment, which is never sent in the HTTP request.
  const token = new URLSearchParams(location.hash.slice(1)).get('t') || '';

  let ws = null;
  let viewer = null;
  let myPeerId = null;
  let revision = 0;
  const peers = new Map(); // peerId -> { info, avatar, target }
  let statusTimer = null;

  function say(text, sticky = false) {
    statusEl.textContent = text;
    statusEl.style.display = text ? '' : 'none';
    if (statusTimer) clearTimeout(statusTimer);
    if (text && !sticky) statusTimer = setTimeout(() => { statusEl.style.display = 'none'; }, 5000);
  }

  function addChatLine(name, text, kind) {
    const div = document.createElement('div');
    div.className = `chat-line chat-${kind}`;
    div.innerHTML = `<b>${escapeText(name)}</b> ${escapeText(text)}`;
    chatLog.appendChild(div);
    chatLog.scrollTop = chatLog.scrollHeight;
  }

  function updatePeerList() {
    const names = [...peers.values()].map((p) => escapeText(p.info.name));
    hudPeers.textContent = names.length ? `With: ${names.join(', ')}` : '';
  }

  function send(msg) {
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
  }

  function removePeer(id) {
    const p = peers.get(id);
    if (!p) return;
    if (viewer) viewer.scene.remove(p.avatar);
    peers.delete(id);
    updatePeerList();
  }

  function handle(msg) {
    switch (msg.type) {
      case 'welcome': {
        myPeerId = msg.peer_id;
        revision = msg.revision;
        hudSession.textContent = msg.session.name;
        viewer = createWorldViewer(el('scene'), msg.world, { assetBase: msg.asset_base || null });
        for (const peer of msg.peers) {
          if (peer.id === myPeerId) continue;
          const avatar = makeAvatar(peer);
          avatar.visible = !!peer.presence;
          if (peer.presence) avatar.position.set(...peer.presence.position);
          peers.set(peer.id, { info: peer, avatar, target: null });
          viewer.scene.add(avatar);
        }
        for (const job of msg.jobs) {
          if (job.state.state === 'queued' || job.state.state === 'running') {
            say(`Building: ${job.prompt}`);
          }
        }
        joinOverlay.style.display = 'none';
        say(`Joined ${msg.session.name}`);
        break;
      }
      case 'ops': {
        if (!viewer) break;
        if (msg.revision !== revision + 1) {
          send({ type: 'resync' });
          break;
        }
        revision = msg.revision;
        viewer.applyOps(msg.ops);
        break;
      }
      case 'snapshot': {
        if (viewer) viewer.dispose();
        viewer = createWorldViewer(el('scene'), msg.world, {});
        revision = msg.revision;
        // Re-add avatars to the new scene.
        for (const p of peers.values()) viewer.scene.add(p.avatar);
        break;
      }
      case 'peer_joined': {
        if (!viewer || msg.peer.id === myPeerId) break;
        const avatar = makeAvatar(msg.peer);
        avatar.visible = false;
        peers.set(msg.peer.id, { info: msg.peer, avatar, target: null });
        viewer.scene.add(avatar);
        updatePeerList();
        say(`${msg.peer.name} joined`);
        break;
      }
      case 'peer_left': {
        const p = peers.get(msg.peer_id);
        if (p) say(`${p.info.name} left`);
        removePeer(msg.peer_id);
        break;
      }
      case 'presence': {
        const p = peers.get(msg.peer_id);
        if (!p) break;
        p.target = msg.presence;
        p.avatar.visible = true;
        break;
      }
      case 'job': {
        const s = msg.job.state;
        if (s.state === 'running') say(`Building: ${msg.job.prompt}`);
        else if (s.state === 'done') say('Build finished');
        else if (s.state === 'failed') say(`Build failed: ${s.reason}`);
        else if (s.state === 'rejected') say(`Prompt rejected: ${s.reason}`);
        break;
      }
      case 'chat':
        addChatLine(msg.from.name, msg.text, msg.kind);
        break;
      case 'reject':
        say(`Edit rejected: ${msg.reason}`);
        break;
      case 'error':
        joinError.textContent = msg.reason;
        joinOverlay.style.display = '';
        if (ws) ws.close();
        break;
      case 'pong':
        break;
      default:
        break;
    }
  }

  function connect(name) {
    joinError.textContent = '';
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(`${proto}//${location.host}/session`);
    ws.onopen = () => {
      send({ type: 'hello', protocol: 1, name, token: token || undefined, client: 'web' });
    };
    ws.onmessage = (event) => {
      let msg;
      try { msg = JSON.parse(event.data); } catch { return; }
      handle(msg);
    };
    ws.onclose = () => {
      if (joinOverlay.style.display !== 'none') return; // failed before join
      say('Disconnected — reload to rejoin', true);
      for (const id of [...peers.keys()]) removePeer(id);
    };
    ws.onerror = () => {
      joinError.textContent = 'Could not reach the session.';
    };
  }

  joinBtn.addEventListener('click', () => {
    const name = nameInput.value.trim();
    if (!name) { joinError.textContent = 'Pick a name first.'; return; }
    localStorage.setItem('localgpt.session.name', name);
    joinBtn.disabled = true;
    connect(name);
  });

  chatInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && chatInput.value.trim()) {
      const text = chatInput.value.trim();
      if (text === '/undo') {
        send({ type: 'undo' });
      } else {
        send({ type: 'chat', text });
      }
      chatInput.value = '';
    }
    e.stopPropagation();
  });

  promptInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && promptInput.value.trim()) {
      const target = viewer ? viewer.controls.target : { x: 0, y: 0, z: 0 };
      send({
        type: 'prompt',
        request_id: crypto.randomUUID(),
        text: promptInput.value.trim(),
        anchor: [target.x, 0, target.z],
      });
      say('Prompt sent…');
      promptInput.value = '';
    }
    e.stopPropagation();
  });

  // Presence at 5 Hz, plus avatar interpolation.
  setInterval(() => {
    if (!viewer || !ws || ws.readyState !== WebSocket.OPEN) return;
    const p = viewer.camera.position;
    const t = viewer.controls.target;
    send({ type: 'presence', position: [p.x, p.y, p.z], look_at: [t.x, t.y, t.z] });
  }, 200);

  function animateAvatars() {
    requestAnimationFrame(animateAvatars);
    for (const p of peers.values()) {
      if (!p.target) continue;
      p.avatar.position.lerp(new THREE.Vector3(...p.target.position), 0.25);
      const look = new THREE.Vector3(...p.target.look_at);
      if (look.distanceToSquared(p.avatar.position) > 0.01) p.avatar.lookAt(look);
    }
  }
  animateAvatars();
}
