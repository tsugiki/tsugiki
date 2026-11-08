import { render, screen } from "@testing-library/svelte";
import { expect, it } from "vitest";
import { issueIcon, prIcon, type Octicon } from "./octicons";
import StateIcon from "./StateIcon.svelte";

const named = (icon: Octicon | undefined) => icon && [icon.label, icon.tone];

it("gives each Issue state GitHub's icon and color, and an Issue of no known state none", () => {
  const states = ["OPEN", "CLOSED", "NOT_PLANNED", "DUPLICATE", null] as const;

  expect(states.map((state) => named(issueIcon(state)))).toEqual([
    ["Open issue", "open"],
    ["Closed issue", "done"],
    ["Closed as not planned", "muted"],
    ["Closed as duplicate", "muted"],
    undefined,
  ]);
});

it("gives each PR state GitHub's icon and color, and a draft that ended the icon of how it ended", () => {
  const icons = [
    prIcon({ state: "OPEN", is_draft: false }),
    prIcon({ state: "OPEN", is_draft: true }),
    prIcon({ state: "MERGED", is_draft: false }),
    prIcon({ state: "CLOSED", is_draft: false }),
    prIcon({ state: "MERGED", is_draft: true }),
    prIcon({ state: "CLOSED", is_draft: true }),
  ];

  expect(icons.map(named)).toEqual([
    ["Open PR", "open"],
    ["Draft PR", "muted"],
    ["Merged PR", "done"],
    ["Closed PR", "closed"],
    ["Merged PR", "done"],
    ["Closed PR", "closed"],
  ]);
});

it("draws the icon in its color, names it for a screen reader and a tooltip, and adds no text", () => {
  const { container } = render(StateIcon, { icon: prIcon({ state: "MERGED", is_draft: false }) });

  const icon = screen.getByRole("img", { name: "Merged PR" });

  expect([icon.classList.contains("done"), icon.title, container.textContent?.trim()]).toEqual([
    true,
    "Merged PR",
    "",
  ]);
  expect(icon.querySelectorAll("path")).toHaveLength(1);
});
