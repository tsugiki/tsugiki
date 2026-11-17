// The scripts' helpers for running commands and git, and for stopping with a message for the maintainer.
import { execFileSync } from "node:child_process";

/** Prints `message` and exits with 1. */
export function stop(message: string): never {
  console.error(message);
  process.exit(1);
}

/** Runs a command and returns its output, or stops with `failed` after the command's own message. */
export function run(failed: string, command: string, ...args: string[]): string {
  try {
    return execFileSync(command, args, { encoding: "utf8" }).trim();
  } catch {
    stop(failed);
  }
}

/** Runs a command and returns its output, or `undefined` when it fails, without printing its message. */
export function attempt(command: string, ...args: string[]): string | undefined {
  try {
    return execFileSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return undefined;
  }
}

/** The message of a caught `cause`: an `Error`'s own, or anything else as text. */
export function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * Runs git and returns its output without the line break after it. It throws when git fails, with git's
 * message in the error. A whole diff of the branch can run past Node's default buffer of 1 MiB.
 */
export function git(...args: string[]): string {
  return execFileSync("git", args, {
    encoding: "utf8",
    maxBuffer: 2 ** 28,
    stdio: ["ignore", "pipe", "pipe"],
  }).replace(/\r?\n$/, "");
}

/**
 * The files in `folder`'s checkout that git doesn't track but would add, relative to `folder`. With `-z`, git
 * prints a path with spaces or non-ASCII letters as it is, where it would otherwise quote it.
 */
export function untracked(folder = "."): string[] {
  return nulSeparated(git("-C", folder, "ls-files", "--others", "--exclude-standard", "-z"));
}

/** The paths in what a git command printed with `-z`. */
export function nulSeparated(listed: string): string[] {
  return listed.split("\0").filter((path) => path !== "");
}
