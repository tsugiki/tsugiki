// The PreToolUse hook that .claude/settings.json runs before each Bash or PowerShell command an agent sends.
// Claude Code passes the call as JSON on stdin. Exit 2 refuses the command and shows stderr to the agent.
import { readFileSync } from "node:fs";

import { shellEdit } from "./shell-edits.ts";

interface Call {
  tool_input?: { command?: unknown };
}
const call = JSON.parse(readFileSync(0, "utf8")) as Call;
const command = call.tool_input?.command;
const why = typeof command === "string" ? shellEdit(command) : undefined;
if (why !== undefined) {
  console.error(why);
  process.exit(2);
}
