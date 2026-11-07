// Where a NEWS entry's links go. The core says what each link names, and this gives the address CPython's
// docs build gives it, so the core names no site but GitHub (ADR 0005).
import type { LinkTo } from "$lib/state/pr-files.svelte";

export function address(to: LinkTo): string {
  switch (to.site) {
    case "web":
      return to.url;
    case "pep":
      return `https://peps.python.org/pep-${String(to.number).padStart(4, "0")}/`;
    case "gh_issue":
      return `https://github.com/python/cpython/issues/${String(to.number)}`;
    case "bpo":
      return `https://bugs.python.org/issue${String(to.number)}`;
    case "rfc":
      return `https://datatracker.ietf.org/doc/html/rfc${String(to.number)}.html`;
    case "cve":
      return `https://www.cve.org/CVERecord?id=${encodeURIComponent(to.id)}`;
    case "pypi":
      return `https://pypi.org/project/${encodeURIComponent(to.project)}/`;
  }
}
