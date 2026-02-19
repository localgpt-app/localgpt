const API = '/api';
let sessionId = null;
let isStreaming = false;
let statusPollInterval = null;
let logsAutoRefreshInterval = null;

// Initialize on DOM load
document.addEventListener('DOMContentLoaded', () => {
    loadSessions();
    setupEventListeners();
    showEmptyState();
    loadStatus();
    startStatusPolling();
});

function setupEventListeners() {
    document.getElementById('send').onclick = sendMessage;
    document.getElementById('new-session').onclick = newSession;

    const input = document.getElementById('input');
    input.onkeydown = (e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            sendMessage();
        }
    };

    // Auto-resize textarea
    input.oninput = () => {
        input.style.height = 'auto';
        input.style.height = Math.min(input.scrollHeight, 200) + 'px';
    };

    document.getElementById('session-select').onchange = async (e) => {
        if (e.target.value) {
            sessionId = e.target.value;
            clearMessages();
            await loadSessionMessages(sessionId);
        }
    };

    // Status panel toggle
    document.getElementById('status-toggle').onclick = toggleStatusPanel;
    document.getElementById('status-close').onclick = toggleStatusPanel;

    // Logs panel
    document.getElementById('logs-toggle').onclick = toggleLogsPanel;
    document.getElementById('logs-close').onclick = toggleLogsPanel;
    document.getElementById('logs-refresh').onclick = loadDaemonLogs;
    document.getElementById('logs-auto').onchange = (e) => {
        if (e.target.checked) {
            startLogsAutoRefresh();
        } else {
            stopLogsAutoRefresh();
        }
    };

    // Sessions panel
    document.getElementById('sessions-toggle').onclick = toggleSessionsPanel;
    document.getElementById('sessions-close').onclick = toggleSessionsPanel;
    document.getElementById('session-back').onclick = showSessionsList;
}

function showEmptyState() {
    const messages = document.getElementById('messages');
    if (messages.children.length === 0) {
        messages.innerHTML = `
            <div class="empty-state">
                <h2>Welcome to LocalGPT</h2>
                <p>Start a conversation by typing a message below.</p>
            </div>
        `;
    }
}

function showFirstRunWelcome() {
    const messages = document.getElementById('messages');
    if (messages.children.length === 0) {
        messages.innerHTML = `
            <div class="empty-state welcome">
                <h1>Welcome to LocalGPT</h1>
                <p>This is your first session. I've set up a fresh workspace for you.</p>
                <h3>Quick Start</h3>
                <ol>
                    <li><strong>Just chat</strong> - I'm ready to help with coding, writing, research, or anything else</li>
                    <li><strong>Your memory files</strong> are in the workspace:
                        <ul>
                            <li><code>MEMORY.md</code> - I'll remember important things here</li>
                            <li><code>SOUL.md</code> - Customize my personality and behavior</li>
                            <li><code>HEARTBEAT.md</code> - Tasks for autonomous mode</li>
                        </ul>
                    </li>
                </ol>
                <h3>Tell Me About Yourself</h3>
                <p>What's your name? What kind of projects do you work on? Any preferences for how I should communicate?</p>
                <p><em>I'll save what I learn to MEMORY.md so I remember it next time.</em></p>
            </div>
        `;
    }
}

function clearEmptyState() {
    const emptyState = document.querySelector('.empty-state');
    if (emptyState) {
        emptyState.remove();
    }
}

async function loadSessions() {
    try {
        const res = await fetch(`${API}/sessions`);
        const data = await res.json();
        const sessions = data.sessions || [];

        const select = document.getElementById('session-select');
        if (sessions.length === 0) {
            select.innerHTML = '<option value="">No sessions</option>';
        } else {
            select.innerHTML = sessions.map(s =>
                `<option value="${s.session_id}">${s.session_id.slice(0, 8)}... (idle ${formatTime(s.idle_seconds)})</option>`
            ).join('');
            sessionId = sessions[0].session_id;
        }
    } catch (err) {
        console.error('Failed to load sessions:', err);
    }
}

function formatTime(seconds) {
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    return `${Math.floor(seconds / 3600)}h`;
}

async function newSession() {
    sessionId = null;
    clearMessages();
    showEmptyState();

    // Update select
    const select = document.getElementById('session-select');
    const newOption = document.createElement('option');
    newOption.value = '';
    newOption.text = 'New session';
    newOption.selected = true;
    select.insertBefore(newOption, select.firstChild);
}

function clearMessages() {
    document.getElementById('messages').innerHTML = '';
}

async function loadSessionMessages(sessionId) {
    try {
        const res = await fetch(`${API}/sessions/${sessionId}/messages`);
        if (!res.ok) {
            if (res.status === 404) {
                // Session not found, show empty state
                showEmptyState();
                return;
            }
            throw new Error(`HTTP ${res.status}`);
        }

        const data = await res.json();

        if (!data.messages || data.messages.length === 0) {
            showEmptyState();
            return;
        }

        // Render each message
        for (const msg of data.messages) {
            if (msg.role === 'system') continue; // Skip system messages

            if (msg.role === 'user') {
                appendMessage('user', msg.content || '');
            } else if (msg.role === 'assistant') {
                const div = appendMessage('assistant', msg.content || '');

                // Render tool calls if present
                if (msg.tool_calls && msg.tool_calls.length > 0) {
                    for (const tc of msg.tool_calls) {
                        const toolDiv = document.createElement('div');
                        toolDiv.className = 'message tool';
                        toolDiv.innerHTML = `<span class="tool-name">[${tc.name}]</span>`;
                        div.after(toolDiv);
                    }
                }
            } else if (msg.role === 'toolResult') {
                const toolDiv = document.createElement('div');
                toolDiv.className = 'message tool';
                const output = msg.content ? msg.content.slice(0, 300) : 'Done';
                toolDiv.innerHTML = `<span class="tool-name">[result]</span><div class="tool-output">${escapeHtml(output)}</div>`;
                document.getElementById('messages').appendChild(toolDiv);
            }
        }

        scrollToBottom();
    } catch (err) {
        console.error('Failed to load session messages:', err);
        showEmptyState();
    }
}

async function sendMessage() {
    if (isStreaming) return;

    const input = document.getElementById('input');
    const message = input.value.trim();
    if (!message) return;

    input.value = '';
    input.style.height = 'auto';
    clearEmptyState();

    // Handle slash commands client-side
    if (message.startsWith('/')) {
        if (handleSlashCommand(message)) return;
    }

    appendMessage('user', message);
    const assistantDiv = appendMessage('assistant', '');
    assistantDiv.classList.add('loading');

    const sendBtn = document.getElementById('send');
    sendBtn.disabled = true;
    isStreaming = true;

    try {
        const res = await fetch(`${API}/chat/stream`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ message, session_id: sessionId })
        });

        if (!res.ok) {
            throw new Error(`HTTP ${res.status}: ${res.statusText}`);
        }

        const reader = res.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';

        while (true) {
            const { done, value } = await reader.read();
            if (done) break;

            buffer += decoder.decode(value, { stream: true });
            const lines = buffer.split('\n');
            buffer = lines.pop() || '';

            for (const line of lines) {
                if (!line.startsWith('data: ')) continue;
                const data = line.slice(6);
                if (data === '[DONE]') continue;

                try {
                    const event = JSON.parse(data);
                    handleEvent(event, assistantDiv);
                } catch (e) {
                    // Ignore parse errors for partial data
                }
            }
        }
    } catch (err) {
        assistantDiv.classList.remove('loading');
        assistantDiv.classList.add('error');
        assistantDiv.textContent = `Error: ${err.message}`;
    } finally {
        assistantDiv.classList.remove('loading');
        sendBtn.disabled = false;
        isStreaming = false;
        scrollToBottom();
    }
}

function handleEvent(event, assistantDiv) {
    switch (event.type) {
        case 'session':
            sessionId = event.session_id;
            updateSessionSelect(sessionId);
            break;

        case 'content':
            assistantDiv.textContent += event.delta;
            scrollToBottom();
            break;

        case 'tool_start':
            const toolStartDiv = document.createElement('div');
            toolStartDiv.className = 'message tool';
            toolStartDiv.id = `tool-${event.id}`;
            const toolLabel = event.detail
                ? `[${event.name}: ${escapeHtml(event.detail)}]`
                : `[${event.name}]`;
            toolStartDiv.innerHTML = `<span class="tool-name">${toolLabel}</span> Running...`;
            assistantDiv.after(toolStartDiv);
            scrollToBottom();
            break;

        case 'tool_end':
            const toolEl = document.getElementById(`tool-${event.id}`);
            if (toolEl) {
                const output = event.output ? event.output.slice(0, 300) : 'Done';
                toolEl.innerHTML = `<span class="tool-name">[${event.name}]</span><div class="tool-output">${escapeHtml(output)}</div>`;
            }
            scrollToBottom();
            break;

        case 'error':
            assistantDiv.classList.add('error');
            assistantDiv.textContent = `Error: ${event.message}`;
            break;

        case 'done':
            break;
    }
}

function updateSessionSelect(newSessionId) {
    const select = document.getElementById('session-select');

    // Check if this session already exists
    for (let i = 0; i < select.options.length; i++) {
        if (select.options[i].value === newSessionId) {
            select.selectedIndex = i;
            return;
        }
    }

    // Add new session to select
    const option = document.createElement('option');
    option.value = newSessionId;
    option.text = `${newSessionId.slice(0, 8)}... (new)`;
    option.selected = true;
    select.insertBefore(option, select.firstChild);

    // Remove "New session" placeholder if exists
    for (let i = 0; i < select.options.length; i++) {
        if (select.options[i].value === '') {
            select.remove(i);
            break;
        }
    }
}

function appendMessage(role, content) {
    const div = document.createElement('div');
    div.className = `message ${role}`;
    div.textContent = content;
    document.getElementById('messages').appendChild(div);
    scrollToBottom();
    return div;
}

function scrollToBottom() {
    const container = document.getElementById('chat-container');
    container.scrollTop = container.scrollHeight;
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Slash command handling
function handleSlashCommand(input) {
    const parts = input.split(/\s+/);
    const cmd = parts[0];
    const arg = parts.slice(1).join(' ').trim();

    switch (cmd) {
        case '/new':
            newSession();
            return true;
        case '/help':
            appendSystemMessage(
                'Available commands:\n' +
                '  /new              Start a new session\n' +
                '  /model            Show current model\n' +
                '  /compact          Compact session history\n' +
                '  /sessions         Toggle sessions panel\n' +
                '  /status           Toggle status panel\n' +
                '  /logs             Toggle logs panel\n' +
                '  /clear            Clear chat display\n' +
                '  /help             Show this help text'
            );
            return true;
        case '/sessions':
            toggleSessionsPanel();
            return true;
        case '/status':
            toggleStatusPanel();
            return true;
        case '/logs':
            toggleLogsPanel();
            return true;
        case '/model':
            loadStatus().then(() => {
                const model = document.getElementById('status-model').textContent;
                appendSystemMessage(`Current model: ${model}`);
            });
            return true;
        case '/clear':
            clearMessages();
            showEmptyState();
            return true;
        case '/compact':
            if (!sessionId) {
                appendSystemMessage('No active session to compact.');
                return true;
            }
            fetch(`${API}/sessions/${sessionId}/compact`, { method: 'POST' })
                .then(res => res.json())
                .then(data => {
                    if (data.error) {
                        appendSystemMessage(`Compact failed: ${data.error}`);
                    } else {
                        appendSystemMessage(
                            `Session compacted: ${data.token_count_before || '?'} -> ${data.token_count_after || '?'} tokens`
                        );
                    }
                })
                .catch(err => appendSystemMessage(`Compact failed: ${err.message}`));
            return true;
        default:
            appendSystemMessage(`Unknown command: ${cmd}. Type /help for available commands.`);
            return true;
    }
}

function appendSystemMessage(text) {
    const div = document.createElement('div');
    div.className = 'message system';
    div.style.whiteSpace = 'pre-wrap';
    div.textContent = text;
    document.getElementById('messages').appendChild(div);
    scrollToBottom();
}

// Status panel functions
function toggleStatusPanel() {
    const panel = document.getElementById('status-panel');
    panel.classList.toggle('hidden');
    if (!panel.classList.contains('hidden')) {
        loadStatus();
    }
}

function startStatusPolling() {
    // Poll status every 30 seconds
    statusPollInterval = setInterval(loadStatus, 30000);
}

async function loadStatus() {
    try {
        // Fetch both status and heartbeat in parallel
        const [statusRes, heartbeatRes] = await Promise.all([
            fetch(`${API}/status`),
            fetch(`${API}/heartbeat/status`)
        ]);

        const status = await statusRes.json();
        const heartbeat = await heartbeatRes.json();

        updateStatusPanel(status, heartbeat);
        
        // Show detailed welcome message on first run
        if (status.is_brand_new) {
            showFirstRunWelcome();
        }
    } catch (err) {
        console.error('Failed to load status:', err);
    }
}

function updateStatusPanel(status, heartbeat) {
    // Update general status
    document.getElementById('status-version').textContent = status.version || '-';
    document.getElementById('status-model').textContent = status.model || '-';
    document.getElementById('status-sessions').textContent = status.active_sessions || '0';

    // Update heartbeat status
    const statusDot = document.getElementById('status-dot');
    const heartbeatStatusEl = document.getElementById('heartbeat-status');
    const heartbeatIntervalEl = document.getElementById('heartbeat-interval');
    const heartbeatLastEl = document.getElementById('heartbeat-last');
    const heartbeatDetailRow = document.getElementById('heartbeat-detail-row');
    const heartbeatDetailEl = document.getElementById('heartbeat-detail');

    heartbeatIntervalEl.textContent = heartbeat.interval || '-';

    if (!heartbeat.enabled) {
        statusDot.className = 'status-dot disabled';
        heartbeatStatusEl.innerHTML = '<span class="heartbeat-badge disabled">Disabled</span>';
        heartbeatLastEl.textContent = '-';
        heartbeatDetailRow.style.display = 'none';
        return;
    }

    if (!heartbeat.last_event) {
        statusDot.className = 'status-dot';
        heartbeatStatusEl.innerHTML = '<span class="heartbeat-badge">No events yet</span>';
        heartbeatLastEl.textContent = '-';
        heartbeatDetailRow.style.display = 'none';
        return;
    }

    const event = heartbeat.last_event;
    const statusClass = event.status;

    statusDot.className = `status-dot ${statusClass}`;
    heartbeatStatusEl.innerHTML = `<span class="heartbeat-badge ${statusClass}">${formatHeartbeatStatus(event.status)}</span>`;

    // Format last run time
    if (event.age_seconds !== undefined) {
        heartbeatLastEl.textContent = `${formatAge(event.age_seconds)} (${event.duration_ms}ms)`;
    } else {
        heartbeatLastEl.textContent = `${event.duration_ms}ms`;
    }

    // Show detail if available
    if (event.reason || event.preview) {
        heartbeatDetailRow.style.display = 'flex';
        const detail = event.reason || (event.preview ? event.preview.slice(0, 100) + '...' : '-');
        heartbeatDetailEl.textContent = detail;
    } else {
        heartbeatDetailRow.style.display = 'none';
    }
}

function formatHeartbeatStatus(status) {
    const labels = {
        'ok': 'OK',
        'sent': 'Sent',
        'skipped': 'Skipped',
        'failed': 'Failed'
    };
    return labels[status] || status;
}

function formatAge(seconds) {
    if (seconds < 60) return `${seconds}s ago`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
    return `${Math.floor(seconds / 86400)}d ago`;
}

// Logs panel functions
function toggleLogsPanel() {
    const panel = document.getElementById('logs-panel');
    panel.classList.toggle('hidden');
    if (!panel.classList.contains('hidden')) {
        loadDaemonLogs();
    } else {
        stopLogsAutoRefresh();
        document.getElementById('logs-auto').checked = false;
    }
}

async function loadDaemonLogs() {
    try {
        const res = await fetch(`${API}/logs/daemon?lines=200`);
        const data = await res.json();

        const output = document.getElementById('logs-output');
        output.textContent = data.lines.join('\n') || 'No logs available';

        // Auto-scroll to bottom
        output.scrollTop = output.scrollHeight;
    } catch (err) {
        console.error('Failed to load daemon logs:', err);
        document.getElementById('logs-output').textContent = `Error: ${err.message}`;
    }
}

function startLogsAutoRefresh() {
    if (logsAutoRefreshInterval) return;
    logsAutoRefreshInterval = setInterval(loadDaemonLogs, 3000);
}

function stopLogsAutoRefresh() {
    if (logsAutoRefreshInterval) {
        clearInterval(logsAutoRefreshInterval);
        logsAutoRefreshInterval = null;
    }
}

// Sessions panel functions
function toggleSessionsPanel() {
    const panel = document.getElementById('sessions-panel');
    panel.classList.toggle('hidden');
    if (!panel.classList.contains('hidden')) {
        loadSavedSessions();
    }
}

async function loadSavedSessions() {
    try {
        const res = await fetch(`${API}/saved-sessions`);
        const data = await res.json();

        const listEl = document.getElementById('sessions-list');
        const viewerEl = document.getElementById('session-viewer');

        // Show list, hide viewer
        listEl.style.display = 'block';
        viewerEl.classList.add('hidden');

        if (!data.sessions || data.sessions.length === 0) {
            listEl.innerHTML = '<div class="session-item"><em>No saved sessions</em></div>';
            return;
        }

        listEl.innerHTML = data.sessions.map(s => `
            <div class="session-item" onclick="viewSession('${s.id}')">
                <div class="session-item-id">${s.id.slice(0, 16)}...</div>
                <div class="session-item-meta">${s.created_at} \u2022 ${s.message_count} messages</div>
            </div>
        `).join('');
    } catch (err) {
        console.error('Failed to load saved sessions:', err);
        document.getElementById('sessions-list').innerHTML = `<div class="session-item error">Error: ${err.message}</div>`;
    }
}

async function viewSession(sessionId) {
    try {
        const res = await fetch(`${API}/saved-sessions/${sessionId}`);
        const data = await res.json();

        const listEl = document.getElementById('sessions-list');
        const viewerEl = document.getElementById('session-viewer');
        const messagesEl = document.getElementById('session-messages');

        // Hide list, show viewer
        listEl.style.display = 'none';
        viewerEl.classList.remove('hidden');

        messagesEl.innerHTML = data.messages.map(msg => renderSessionMessage(msg)).join('');
    } catch (err) {
        console.error('Failed to view session:', err);
    }
}

function renderSessionMessage(msg) {
    const roleClass = msg.role === 'user' ? 'user' :
                      msg.role === 'toolResult' ? 'tool' : 'assistant';

    let html = `<div class="message ${roleClass}">`;

    if (msg.content) {
        html += escapeHtml(msg.content);
    }

    // Render tool calls
    if (msg.tool_calls && msg.tool_calls.length > 0) {
        for (const tc of msg.tool_calls) {
            const args = tc.arguments || '{}';
            let formattedArgs;
            try {
                formattedArgs = JSON.stringify(JSON.parse(args), null, 2);
            } catch {
                formattedArgs = args;
            }

            html += `
                <div class="tool-call-block" onclick="this.classList.toggle('expanded')">
                    <div class="tool-call-header">
                        <span>[${tc.name}]</span>
                        <span>\u25BC</span>
                    </div>
                    <div class="tool-call-body">${escapeHtml(formattedArgs)}</div>
                </div>
            `;
        }
    }

    // Tool result indicator
    if (msg.tool_call_id) {
        html = `<div class="message tool"><span class="tool-name">[result]</span> ${escapeHtml(msg.content || '')}`;
    }

    html += '</div>';
    return html;
}

function showSessionsList() {
    const listEl = document.getElementById('sessions-list');
    const viewerEl = document.getElementById('session-viewer');
    listEl.style.display = 'block';
    viewerEl.classList.add('hidden');
}
