const BACKEND_URL = 'http://127.0.0.1:8000';

function renderFreezeStatus(status) {
  if (
    typeof status?.legal_hold_active !== 'boolean'
    || typeof status?.dispute_paused !== 'boolean'
  ) {
    throw new Error('Backend returned an invalid escrow freeze status.');
  }

  const alerts = [];

  if (status.legal_hold_active) {
    alerts.push(`
      <div class="freeze-alert legal-hold" role="status">
        <strong>Legal hold active</strong>
        <p>Blocked operations: funding (including committed funding), settlement, SME withdrawal, investor payout claims, and terminal dust sweep.</p>
      </div>
    `);
  }

  if (status.dispute_paused) {
    alerts.push(`
      <div class="freeze-alert dispute-pause" role="status">
        <strong>Dispute pause active</strong>
        <p>Blocked operations: funding, settlement, SME withdrawal, and investor payout claims.</p>
      </div>
    `);
  }

  return alerts.length
    ? alerts.join('')
    : '<p class="freeze-clear">No active legal hold or dispute pause.</p>';
}

async function loadStatus() {
  const statusEl = document.getElementById('status');

  try {
    const response = await fetch(`${BACKEND_URL}/api/health`);
    if (!response.ok) {
      throw new Error(`Backend health request failed (${response.status}).`);
    }

    const payload = await response.json();
    statusEl.replaceChildren();
    const contracts = Array.isArray(payload.contracts) ? payload.contracts.join(', ') : '';
    for (const [label, value] of [
      ['Backend status:', payload.status],
      ['Service:', payload.service],
      ['Contracts:', contracts]
    ]) {
      const strong = document.createElement('strong');
      strong.textContent = label;
      statusEl.append(strong, document.createTextNode(` ${String(value ?? '')}`));
      if (label !== 'Contracts:') statusEl.append(document.createElement('br'));
    }
  } catch (error) {
    statusEl.textContent = `Unable to connect to backend: ${String(error.message ?? error)}`;
  }

  const freezeStatusEl = document.getElementById('freeze-status');
  try {
    const response = await fetch(`${BACKEND_URL}/api/escrow-status`);
    if (!response.ok) {
      throw new Error(`Backend freeze-status request failed (${response.status}).`);
    }

    freezeStatusEl.innerHTML = renderFreezeStatus(await response.json());
  } catch (error) {
    freezeStatusEl.textContent = `Freeze status unavailable. ${error.message}`;
  }
}

if (typeof document !== 'undefined') {
  loadStatus();
}
