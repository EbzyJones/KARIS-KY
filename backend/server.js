const http = require('http');

const HOST = '127.0.0.1';
const PORT = 8000;

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);

  if (url.pathname === '/api/health') {
    res.writeHead(200, {
      'Content-Type': 'application/json',
      'Access-Control-Allow-Origin': '*'
    });
    res.end(JSON.stringify({
      status: 'ok',
      service: 'karis-ky-backend',
      contracts: ['escrow']
    }));
    return;
  }

  if (url.pathname === '/api/contract-summary') {
    res.writeHead(200, {
      'Content-Type': 'application/json',
      'Access-Control-Allow-Origin': '*'
    });
    res.end(JSON.stringify({
      name: 'karis-ky Escrow Contracts',
      description: 'Soroban smart contracts for invoice liquidity on Stellar.',
      entrypoints: ['init', 'fund', 'settle', 'withdraw']
    }));
    return;
  }

  res.writeHead(404, {
    'Content-Type': 'application/json',
    'Access-Control-Allow-Origin': '*'
  });
  res.end(JSON.stringify({ error: 'not found' }));
});

server.listen(PORT, HOST, () => {
  console.log(`Backend listening on http://${HOST}:${PORT}`);
});
