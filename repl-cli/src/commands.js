import { spawnSync } from "node:child_process";

export function diagnose(config, io, runCli = spawnSync) {
  if (!config.contractId) {
    throw new Error("Set CONTRACT_ID to the deployed escrow contract address.");
  }

  const args = ["contract", "invoke", "--id", config.contractId];
  if (config.source) args.push("--source", config.source);
  if (config.network) args.push("--network", config.network);
  args.push("--", "detect_state_inconsistencies");

  const result = runCli("stellar", args, { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.stderr) io.stderr.write(result.stderr);
  if (result.status !== 0) {
    throw new Error(`stellar contract invoke exited with status ${result.status}`);
  }

  if (result.stdout) {
    io.stdout.write(result.stdout);
    if (!result.stdout.endsWith("\n")) io.stdout.write("\n");
  }
}

export function executeCommand(input, config, io, runCli = spawnSync) {
  const [command, ...args] = input.trim().split(/\s+/);
  if (!command) return true;

  switch (command) {
    case "diagnose":
      if (args.length > 0) {
        io.stderr.write("Usage: diagnose\n");
        return true;
      }
      try {
        diagnose(config, io, runCli);
      } catch (error) {
        io.stderr.write(`diagnose failed: ${error.message}\n`);
      }
      return true;
    case "help":
      io.stdout.write("Commands: diagnose, help, quit\n");
      return true;
    case "quit":
    case "exit":
      return false;
    default:
      io.stderr.write(`Unknown command: ${command}. Type 'help' for available commands.\n`);
      return true;
  }
}
