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

  // ---- Direct editing (editor role): select, drag, rotate, scale, delete ----
  const edit = {
    role: 'guest',
    selected: null, // a viewer record
    helper: null,
    drag: null,
    clientSeq: 0,
  };
  const raycaster = new THREE.Raycaster();
  const pointerNdc = new THREE.Vector2();

  function isTyping() {
    return /^(INPUT|TEXTAREA)$/.test(document.activeElement?.tagName || '');
  }

  function recordObjects() {
    const set = new Set();
    if (!viewer) return set;
    for (const rec of viewer.entities.values()) set.add(rec.object);
    return set;
  }

  function recordAt(clientX, clientY) {
    if (!viewer) return null;
    const rect = viewer.renderer.domElement.getBoundingClientRect();
    pointerNdc.set(
      ((clientX - rect.left) / rect.width) * 2 - 1,
      -((clientY - rect.top) / rect.height) * 2 + 1
    );
    raycaster.setFromCamera(pointerNdc, viewer.camera);
    const roots = [...viewer.entities.values()].map((r) => r.object);
    const hits = raycaster.intersectObjects(roots, true);
    if (!hits.length) return null;
    let obj = hits[0].object;
    while (obj) {
      for (const rec of viewer.entities.values()) {
        if (rec.object === obj) return rec;
      }
      obj = obj.parent;
    }
    return null;
  }

  function clearSelection() {
    edit.selected = null;
    if (edit.helper) {
      viewer.scene.remove(edit.helper);
      edit.helper.dispose?.();
      edit.helper = null;
    }
  }

  function select(rec) {
    clearSelection();
    edit.selected = rec;
    edit.helper = new THREE.BoxHelper(rec.object, 0xffcc44);
    viewer.scene.add(edit.helper);
    say(`Selected ${rec.def.name} — drag to move, Q/E rotate, +/- scale, Del delete`);
  }

  function submitOps(ops) {
    // No expected_revision: the authority's order decides (last write wins
    // per field), so rapid successive edits can't reject each other.
    send({
      type: 'submit',
      client_seq: ++edit.clientSeq,
      ops,
    });
  }

  function submitTransform(rec) {
    const o = rec.object;
    const deg = (r) => (r * 180) / Math.PI;
    submitOps([
      {
        ModifyEntity: {
          id: rec.def.id,
          patch: {
            transform: {
              position: [o.position.x, o.position.y, o.position.z],
              rotation_degrees: [deg(o.rotation.x), deg(o.rotation.y), deg(o.rotation.z)],
              scale: [o.scale.x, o.scale.y, o.scale.z],
              visible: o.visible,
            },
          },
        },
      },
    ]);
  }

  function onPointerDown(e) {
    if (edit.role !== 'editor' || !viewer || e.button !== 0 || isTyping()) return;
    const rec = recordAt(e.clientX, e.clientY);
    if (!rec) {
      clearSelection();
      return;
    }
    select(rec);
    // Move on the horizontal plane through the grab point.
    const rect = viewer.renderer.domElement.getBoundingClientRect();
    pointerNdc.set(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1
    );
    raycaster.setFromCamera(pointerNdc, viewer.camera);
    const grab = new THREE.Vector3();
    const world = rec.object.getWorldPosition(new THREE.Vector3());
    const plane = new THREE.Plane(new THREE.Vector3(0, 1, 0), -world.y);
    if (!raycaster.ray.intersectPlane(plane, grab)) return;
    edit.drag = {
      rec,
      plane,
      offset: grab.sub(world),
      moved: false,
    };
    viewer.controls.enabled = false;
    viewer.renderer.domElement.setPointerCapture?.(e.pointerId);
  }

  function onPointerMove(e) {
    if (!edit.drag || !viewer) return;
    const rect = viewer.renderer.domElement.getBoundingClientRect();
    pointerNdc.set(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1
    );
    raycaster.setFromCamera(pointerNdc, viewer.camera);
    const point = new THREE.Vector3();
    if (!raycaster.ray.intersectPlane(edit.drag.plane, point)) return;
    const target = point.sub(edit.drag.offset);
    // Convert world → the record's parent space (its transform is local).
    const parent = edit.drag.rec.object.parent;
    if (parent && parent !== viewer.scene) parent.worldToLocal(target);
    edit.drag.rec.object.position.copy(target);
    edit.drag.moved = true;
  }

  function onPointerUp() {
    if (!edit.drag) return;
    const { rec, moved } = edit.drag;
    edit.drag = null;
    if (viewer) viewer.controls.enabled = true;
    if (moved) submitTransform(rec);
  }

  function onEditKey(e) {
    if (edit.role !== 'editor' || !edit.selected || isTyping()) return;
    const rec = edit.selected;
    const step = THREE.MathUtils.degToRad(15);
    switch (e.code) {
      case 'KeyQ':
        rec.object.rotateY(step);
        submitTransform(rec);
        e.preventDefault();
        break;
      case 'KeyE':
        rec.object.rotateY(-step);
        submitTransform(rec);
        e.preventDefault();
        break;
      case 'Equal':
      case 'NumpadAdd':
        rec.object.scale.multiplyScalar(1.1).clampScalar(0.05, 100);
        submitTransform(rec);
        e.preventDefault();
        break;
      case 'Minus':
      case 'NumpadSubtract':
        rec.object.scale.multiplyScalar(1 / 1.1).clampScalar(0.05, 100);
        submitTransform(rec);
        e.preventDefault();
        break;
      case 'Delete':
      case 'Backspace':
        submitOps([{ DeleteEntity: { id: rec.def.id } }]);
        clearSelection();
        e.preventDefault();
        break;
      case 'Escape':
        clearSelection();
        break;
      default:
        break;
    }
  }

  function setupEditing() {
    const canvas = viewer.renderer.domElement;
    canvas.style.cursor = 'crosshair';
    canvas.addEventListener('pointerdown', onPointerDown);
    canvas.addEventListener('pointermove', onPointerMove);
    canvas.addEventListener('pointerup', onPointerUp);
    canvas.addEventListener('pointercancel', onPointerUp);
    document.addEventListener('keydown', onEditKey);
  }

  // Keep the selection helper glued to its (possibly edited) object.
  function updateSelectionHelper() {
    if (!viewer) return;
    if (edit.selected) {
      const stillThere = viewer.entitiesById.get(String(edit.selected.def.id)) === edit.selected;
      if (!stillThere) {
        clearSelection();
        return;
      }
      edit.helper?.update();
    }
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
        edit.role = msg.role === 'editor' || msg.role === 'host' ? 'editor' : 'guest';
        hudSession.textContent = msg.session.name;
        viewer = createWorldViewer(el('scene'), msg.world, { assetBase: msg.asset_base || null });
        setupEditing();
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
        clearSelection();
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
        // Our optimistic edit diverged; take the authoritative state.
        send({ type: 'resync' });
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
    updateSelectionHelper();
    for (const p of peers.values()) {
      if (!p.target) continue;
      p.avatar.position.lerp(new THREE.Vector3(...p.target.position), 0.25);
      const look = new THREE.Vector3(...p.target.look_at);
      if (look.distanceToSquared(p.avatar.position) > 0.01) p.avatar.lookAt(look);
    }
  }
  animateAvatars();

  // Debug/testing handle.
  window.__session = {
    get viewer() { return viewer; },
    get revision() { return revision; },
    get role() { return edit.role; },
    selectByName(name) {
      const rec = viewer?.entities.get(name);
      if (rec) select(rec);
      return !!rec;
    },
    get selected() { return edit.selected?.def.name || null; },
    submitTransform,
    submitOps,
  };
}
