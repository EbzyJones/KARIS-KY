async function loadStatus() {
  const statusEl = document.getElementById('status');

  try {
    const response = await fetch('http://127.0.0.1:8000/api/health');
    const payload = await response.json();
    statusEl.innerHTML = `
      <strong>Backend status:</strong> ${payload.status}<br />
      <strong>Service:</strong> ${payload.service}<br />
      <strong>Contracts:</strong> ${payload.contracts.join(', ')}
    `;
  } catch (error) {
    statusEl.innerHTML = `Unable to connect to backend: ${error.message}`;
  }
}

loadStatus();
