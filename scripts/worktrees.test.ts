import { describe, expect, it } from "vitest";

import {
  addRefusal,
  pullRefusal,
  ticketBranch,
  ticketNumber,
  ticketTitle,
  titleSlug,
  worktreeFolder,
} from "./worktrees.ts";

describe("ticketNumber", () => {
  it("reads a ticket's number", () => {
    expect(ticketNumber("738")).toBe(738);
  });

  it.each([undefined, "", "0", "07", "-3", "7a", "#7", "1.5"])("refuses %j", (arg) => {
    expect(ticketNumber(arg)).toBeUndefined();
  });
});

describe("titleSlug", () => {
  it.each([
    ["Make a ready worktree with one recipe", "make-ready-worktree"],
    ["Say why clangd can't answer", "say-why-clangd"],
    ["Set each language server on Settings › Advanced", "set-language-server"],
    ["The split Definition peek and the `[diff] peek` setting", "split-definition-peek"],
    ["Remove a test's temporary folder when the test passes", "remove-test-temporary"],
    ["Go to a C definition with clangd", "go-c-definition"],
    ["Bump to Node 26", "bump-node-26"],
    ["Ändern über Café", "andern-uber-cafe"],
  ])("takes %j to %j", (title, slug) => {
    expect(titleSlug(title)).toBe(slug);
  });

  it("gives nothing for a title of filler alone", () => {
    expect(titleSlug("The one and the other")).toBe("other");
    expect(titleSlug("And the")).toBe("");
  });
});

describe("ticketTitle", () => {
  it("reads a ticket's title", () => {
    expect(ticketTitle(738, JSON.stringify({ title: "Make a worktree", pull_request: null }))).toBe(
      "Make a worktree",
    );
    expect(ticketTitle(738, JSON.stringify({ title: "Make a worktree" }))).toBe("Make a worktree");
  });

  it("refuses a PR", () => {
    const pr = JSON.stringify({ title: "Release 0.3.1", pull_request: { url: "https://example.com" } });
    expect(() => ticketTitle(737, pr)).toThrow("#737 is a PR, not a ticket.");
  });

  it("refuses an answer with no title", () => {
    expect(() => ticketTitle(738, "{}")).toThrow("GitHub gave #738 no title.");
  });
});

describe("ticketBranch", () => {
  const title = () => "Make a ready worktree with one recipe";

  it("takes the slug from the title", () => {
    expect(ticketBranch(738, "", title)).toBe("738-make-ready-worktree");
  });

  it("takes the slug it's given, and doesn't read the title", () => {
    const unread = () => {
      throw new Error("read the title");
    };
    expect(ticketBranch(738, "worktree-recipe", unread)).toBe("738-worktree-recipe");
  });

  it("refuses a slug our branches wouldn't take", () => {
    expect(() => ticketBranch(738, "Worktree_recipe", title)).toThrow(`"Worktree_recipe" isn't a slug`);
    expect(() => ticketBranch(738, "worktree--recipe", title)).toThrow("isn't a slug");
  });

  it("asks for a slug when the title gives none", () => {
    expect(() => ticketBranch(738, "", () => "“…”")).toThrow("`just worktree 738 some-words`");
  });
});

describe("worktreeFolder", () => {
  it("puts the worktree under .temp", () => {
    expect(worktreeFolder(738)).toBe(".temp/wt-738");
  });
});

describe("addRefusal", () => {
  it("lets a fresh worktree through", () => {
    expect(addRefusal(undefined, ".temp/wt-738", false)).toBeUndefined();
    expect(addRefusal("", ".temp/wt-738", false)).toBeUndefined();
  });

  it("refuses while CARGO_TARGET_DIR is set", () => {
    expect(addRefusal("X:/shared", ".temp/wt-738", false)).toContain("CARGO_TARGET_DIR is set to X:/shared");
  });

  it("refuses a folder that's there already", () => {
    expect(addRefusal(undefined, ".temp/wt-738", true)).toContain(".temp/wt-738 is there already");
  });
});

describe("pullRefusal", () => {
  it("lets a clean main pull", () => {
    expect(pullRefusal("main", "")).toBeUndefined();
  });

  it("refuses a main checkout on another branch", () => {
    expect(pullRefusal("842-trees", "")).toContain("on 842-trees");
    expect(pullRefusal("", "")).toContain("on no branch");
  });

  it("refuses a main checkout with uncommitted changes", () => {
    expect(pullRefusal("main", " M AGENTS.md\n")).toContain("changes git hasn't committed");
  });
});
