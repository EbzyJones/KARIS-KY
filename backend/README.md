# Backend logging

The Node API (`server.js`) and Python API (`server.py`) write one structured
application log record for each escrow-summary request. Each record is a
single-line JSON object:

```json
{"timestamp":"2026-01-02T03:04:05.000Z","level":"info","event":"escrow.summary","escrow_id":"C...","status":200}
```

`timestamp` is UTC in RFC 3339 format. `level` is `info` for successful
responses, `warn` for client errors, and `error` for server errors. `event`
identifies the handler operation, `escrow_id` is the contract ID from the
request path (also populated for invalid IDs), and `status` is the HTTP status
returned by the handler. Records are written to the process's standard log
output; no external sink is configured.

Run the Node API tests with `npm test` from this directory. Run the Python
backend tests from the repository root with `python -m pytest backend/tests`.
