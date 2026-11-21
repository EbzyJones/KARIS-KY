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

```sh
npm test
```
