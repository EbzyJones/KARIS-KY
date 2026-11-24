#!/usr/bin/env node

import { createInterface } from "node:readline";
import { executeCommand } from "./commands.js";

const config = {
  contractId: process.env.CONTRACT_ID,
  network: process.env.NETWORK,
  source: process.env.SOURCE,
};

const repl = createInterface({
  input: process.stdin,
  output: process.stdout,
  prompt: "> ",
});

repl.prompt();
repl.on("line", (input) => {
  if (!executeCommand(input, config, process)) {
    repl.close();
    return;
  }
  repl.prompt();
});
