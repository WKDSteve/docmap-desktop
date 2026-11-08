// The Rust source of the commands, as the tests that join `main.js` to the backend by string equality read it: the
// text of `main.rs` and of the modules the commands moved to. Only the tests import this.

import { readFileSync } from "node:fs";

const DIR = new URL("../../src-tauri/src/", import.meta.url);

/** The modules `main.rs` hands its commands to; add a module here when a command moves into it. */
export const COMMAND_MODULES = ["engine", "import", "nvim", "paths", "traffic_cmds", "views", "workspace"];

/** `main.rs` followed by every command module, one string. */
export const MAIN_RS = [
  "main.rs",
  ...COMMAND_MODULES.map((name) => `${name}.rs`),
]
  .map((file) => readFileSync(new URL(file, DIR), "utf8"))
  .join("\n");
