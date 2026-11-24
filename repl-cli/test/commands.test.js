import assert from "node:assert/strict";
import test from "node:test";
import { executeCommand } from "../src/commands.js";

function createIo() {
  let output = "";
  let errors = "";
  return {
    io: {
      stdout: { write: (text) => { output += text; } },
      stderr: { write: (text) => { errors += text; } },
    },
    output: () => output,
    errors: () => errors,
  };
}

test("diagnose invokes the state consistency entrypoint and prints its report", () => {
  const captured = createIo();
  let invocation;
  const runCli = (binary, args, options) => {
    invocation = { binary, args, options };
    return {
      status: 0,
      stdout: '{"funded_amount_positive_status_open":false}\n',
      stderr: "",
    };
  };

  executeCommand(
    "diagnose",
    { contractId: "C123", network: "testnet", source: "operator" },
    captured.io,
    runCli,
  );

  assert.deepEqual(invocation, {
    binary: "stellar",
    args: [
      "contract", "invoke", "--id", "C123",
      "--source", "operator", "--network", "testnet",
      "--", "detect_state_inconsistencies",
    ],
    options: { encoding: "utf8" },
  });
  assert.equal(captured.output(), '{"funded_amount_positive_status_open":false}\n');
  assert.equal(captured.errors(), "");
});

test("diagnose reports invocation failures without printing a report", () => {
  const captured = createIo();
  executeCommand(
    "diagnose",
    { contractId: "C123" },
    captured.io,
    () => ({ status: 1, stdout: "", stderr: "RPC unavailable\n" }),
  );

  assert.equal(captured.output(), "");
  assert.match(captured.errors(), /RPC unavailable/);
  assert.match(captured.errors(), /exited with status 1/);
});
