// An issue's open blockers, as `just next` and `just tree` read them from GitHub's GraphQL API. A blocker can
// sit in another repository, whose numbers mean other issues, so each one says where it lives. `blockedBy`
// lists only the first 100.

/**
 * The `blockedBy` field to ask for on an issue. It can't filter by state, so closed blockers count toward its
 * 100 too.
 */
export const BLOCKED_BY =
  "blockedBy(first: 100) { totalCount nodes { number state repository { nameWithOwner } } }";

/** GitHub's answer to `BLOCKED_BY`. */
export interface BlockedBy {
  totalCount: number;
  nodes: { number: number; state: "OPEN" | "CLOSED"; repository: { nameWithOwner: string } }[];
}

/** An issue's open blockers. */
export interface OpenBlockers {
  /** The numbers of its open blockers in its own repository. */
  openBlockers: number[];
  /** Its open blockers in other repositories, as `owner/name#N`. */
  blockersElsewhere: string[];
}

/** The open blockers of an issue in `repository`, given as `owner/name`. */
export function openBlockers(blockedBy: BlockedBy, repository: string): OpenBlockers {
  const open = blockedBy.nodes.filter((blocker) => blocker.state === "OPEN");
  const here = (blocker: (typeof open)[number]): boolean => blocker.repository.nameWithOwner === repository;
  return {
    openBlockers: open.filter(here).map((blocker) => blocker.number),
    blockersElsewhere: open
      .filter((blocker) => !here(blocker))
      .map((blocker) => `${blocker.repository.nameWithOwner}#${String(blocker.number)}`),
  };
}

/** The warning to print when GitHub listed only some of an issue's blockers, if it did. */
export function cutWarning(issues: { number: number; blockedBy: BlockedBy }[]): string | undefined {
  const cut = issues
    .filter((issue) => issue.blockedBy.nodes.length < issue.blockedBy.totalCount)
    .map((issue) => `#${String(issue.number)}`);
  return cut.length === 0
    ? undefined
    : `GitHub listed only the first 100 blockers of ${cut.join(", ")}, so open ones may be missing.`;
}
