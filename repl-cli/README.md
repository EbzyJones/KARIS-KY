# KARIS-KY Escrow REPL

A minimal interactive command-line interface for read-only escrow diagnostics.
It delegates contract invocation to the [Stellar CLI](https://developers.stellar.org/docs/tools/stellar-cli).

## Requirements

- Node.js 18 or newer
- The `stellar` CLI installed and configured for the target network
- A deployed escrow contract ID

## Start

Set `CONTRACT_ID` and optionally `NETWORK` and `SOURCE`, then start the REPL:

```sh
CONTRACT_ID=C... NETWORK=testnet npm start
```

`SOURCE` is optional and is only needed when required by the Stellar CLI configuration.
The diagnostic contract entrypoint itself does not require authorization.

## Commands

- `diagnose` invokes `detect_state_inconsistencies` on the configured contract and prints the report returned by the Stellar CLI. Invocation failures are reported to stderr.
- `help` lists available commands.
- `quit` or `exit` leaves the REPL.

The diagnostic is read-only with respect to contract storage; it does not repair
inconsistencies. The contract may emit its diagnostic event when any inconsistency
is found.

## Tests

═══════════════════════════════════════════════════════
```

### `help [command]`

**Purpose**: Display help information for available commands.

**Usage**:
```
repl-cli> help
repl-cli> help check_health
repl-cli> help get_health
```

### `fund_with_commitment`

**Purpose**: Test and simulate first-time investor contribution with commitment lock and view selected yield tier.

**Arguments**:
- `<investor>` - The investor address (Stellar format)
- `<amount>` - Principal contribution in stroops
- `<lock_secs>` - Lock duration in seconds

**Usage**:
```
repl-cli> fund_with_commitment GDPM3QMXN3APYMYBNPIBMVJHD3FQJSCAEBFHDZZS3MSVVUAOTBMVYF2 50000000 86400
{
  "status": "success",
  "investor": "GDPM3QMXN3APYMYBNPIBMVJHD3FQJSCAEBFHDZZS3MSVVUAOTBMVYF2",
  "amount": 50000000,
  "lock_secs": 86400,
  "yield_tier_selected": {
    "tier_index": 1,
    "effective_yield_bps": 650
  },
  "claim_not_before": 1700086400
}
```

### `fund-batch`

**Purpose**: Fund multiple investors in a single `fund_batch` contract call from a CSV file. This speeds up batch funding for operators.

**Arguments**:
- `<path-to-csv>` - Path to a CSV file containing `(address, amount)` pairs

**CSV Format**:
- One `address,amount` pair per line
- `amount` is in stroops (integer)
- Blank lines are ignored
- Lines starting with `#` are treated as comments and ignored
- At most 50 entries per call (matching the contract's `MAX_FUND_BATCH`)

Example CSV (`investors.csv`):
```
# address,amount (stroops)
GDPM3QMXN3APYMYBNPIBMVJHD3FQJSCAEBFHDZZS3MSVVUAOTBMVYF2,50000000
GABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRSTUV,25000000
```

**Usage**:
```
repl-cli> fund-batch investors.csv
{
  "status": "success",
  "entries": 2,
  "total_amount": 75000000
}
```

### Entrypoint help (`help <entrypoint>`)

Every contract entrypoint has help text generated from `sdk-ts/spec.json`
(description, required auth, arguments with types, and return type).
`help` with no argument lists all entrypoint names.

```
$ repl-cli help settle
$ repl-cli help          # list every entrypoint
```

### Tab completion (`complete <prefix>`)

Entrypoint names complete from the contract spec. `complete <prefix>` prints
all matching entrypoints (sorted), suitable for wiring into shell completion:

```
$ repl-cli complete fund
fund
fund_batch
fund_with_commitment
```

### JSON output (`--json`)

Pass `--json` anywhere on the command line to emit the parsed command as a
single JSON object instead of human-readable text, for consumption by scripts:

```
$ repl-cli --json fund GABC 100
{"args":["GABC","100"],"command":"fund","known_entrypoint":true}
```

### RPC retry policy

Read-only RPC calls are wrapped in `retry_read`: transient failures
(timeouts, connection resets, 5xx) are retried up to **3 attempts total** with
exponential backoff starting at **100ms** (100ms, 200ms). Fatal errors are
returned immediately. Mutating calls are **never** retried, to avoid
applying a write twice.

### `quit` / `exit`

**Purpose**: Exit the REPL CLI.

**Usage**:
```
repl-cli> quit
repl-cli> exit
```
