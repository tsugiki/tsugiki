import { describe, expect, it } from "vitest";
import { syncedWords } from "./synced";

const now = new Date("2026-11-05T12:00:00Z");

describe("syncedWords", () => {
  it("says so before the first sync finishes", () => {
    expect(syncedWords(null, now)).toBe("Not synced yet");
  });

  it("calls the last minute just now, whatever the clocks say", () => {
    expect(syncedWords("2026-11-05T11:59:30Z", now)).toBe("Synced just now");
    expect(syncedWords("2026-11-05T12:00:30Z", now)).toBe("Synced just now");
  });

  it("counts minutes, then hours, then days", () => {
    expect(syncedWords("2026-11-05T11:58:00Z", now)).toBe("Synced 2 min ago");
    expect(syncedWords("2026-11-05T11:00:01Z", now)).toBe("Synced 59 min ago");
    expect(syncedWords("2026-11-05T09:00:00Z", now)).toBe("Synced 3 h ago");
    expect(syncedWords("2026-11-03T12:00:00Z", now)).toBe("Synced 2 d ago");
  });
});
