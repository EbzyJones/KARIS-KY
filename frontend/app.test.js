const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const test = require('node:test');
const vm = require('node:vm');

const source = readFileSync(`${__dirname}/app.js`, 'utf8');
const context = {};
vm.runInNewContext(source, context);

test('renders legal-hold status and the blocked operations', () => {
  const html = context.renderFreezeStatus({
    legal_hold_active: true,
    dispute_paused: false,
  });

  assert.match(html, /Legal hold active/);
  assert.match(html, /funding \(including committed funding\)/);
  assert.match(html, /settlement, SME withdrawal, investor payout claims, and terminal dust sweep/);
  assert.doesNotMatch(html, /Dispute pause active/);
});

test('renders dispute-pause status and the blocked operations', () => {
  const html = context.renderFreezeStatus({
    legal_hold_active: false,
    dispute_paused: true,
  });

  assert.match(html, /Dispute pause active/);
  assert.match(html, /funding, settlement, SME withdrawal, and investor payout claims/);
  assert.doesNotMatch(html, /Legal hold active/);
});

test('renders both active freeze types and rejects incomplete backend status', () => {
  const html = context.renderFreezeStatus({
    legal_hold_active: true,
    dispute_paused: true,
  });

  assert.match(html, /Legal hold active/);
  assert.match(html, /Dispute pause active/);
  assert.throws(
    () => context.renderFreezeStatus({ legal_hold_active: false }),
    /invalid escrow freeze status/,
  );
});

test('loads freeze state from the backend and renders the dispute pause', async () => {
  const elements = {
    status: { innerHTML: '' },
    'freeze-status': { innerHTML: '', textContent: '' },
  };
  const requests = [];
  context.document = {
    getElementById: (id) => elements[id],
  };
  context.fetch = async (url) => {
    requests.push(url);
    return {
      ok: true,
      status: 200,
      json: async () => url.endsWith('/api/escrow-status')
        ? { legal_hold_active: false, dispute_paused: true }
        : { status: 'ok', service: 'backend', contracts: ['escrow'] },
    };
  };

  await context.loadStatus();

  assert.ok(requests.some((url) => url.endsWith('/api/escrow-status')));
  assert.match(elements['freeze-status'].innerHTML, /Dispute pause active/);
  assert.match(elements['freeze-status'].innerHTML, /investor payout claims/);
});
