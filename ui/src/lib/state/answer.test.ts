import { describe, expect, it } from "vitest";
import { ask, problemIn } from "./answer";

const refused = { status: "error" as const, error: { kind: "offline" as const, message: "offline" } };

describe("ask", () => {
  it("keeps the core's refusal as it came, kind and all", async () => {
    const answer = await ask(Promise.resolve(refused));

    expect(answer).toBe(refused);
  });

  it("reads a call that rejects as a refusal with what it threw", async () => {
    const answer = await ask(Promise.reject(new Error("no core")));

    expect(answer).toEqual({ status: "error", error: { message: "Error: no core" } });
  });

  it("reads a call that throws before it answers as a refusal", async () => {
    const answer = await ask(() => {
      throw new Error("no IPC");
    });

    expect(answer).toEqual({ status: "error", error: { message: "Error: no IPC" } });
  });
});

describe("problemIn", () => {
  it("is nothing for a command that did what it was asked", async () => {
    expect(await problemIn(Promise.resolve({ status: "ok" as const, data: null }))).toBeUndefined();
  });

  it("is the refusal's message", async () => {
    expect(await problemIn(Promise.resolve(refused))).toBe("offline");
  });
});
