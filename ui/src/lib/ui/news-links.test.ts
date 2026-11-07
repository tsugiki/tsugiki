import { expect, it } from "vitest";
import { address } from "./news-links";

it("gives each kind of link the address CPython's docs give it", () => {
  expect([
    address({ site: "web", url: "https://github.com/python/cpython" }),
    address({ site: "pep", number: 8 }),
    address({ site: "gh_issue", number: 123 }),
    address({ site: "bpo", number: 456 }),
    address({ site: "rfc", number: 9110 }),
    address({ site: "cve", id: "CVE-2026-1" }),
    address({ site: "pypi", project: "pip" }),
  ]).toEqual([
    "https://github.com/python/cpython",
    "https://peps.python.org/pep-0008/",
    "https://github.com/python/cpython/issues/123",
    "https://bugs.python.org/issue456",
    "https://datatracker.ietf.org/doc/html/rfc9110.html",
    "https://www.cve.org/CVERecord?id=CVE-2026-1",
    "https://pypi.org/project/pip/",
  ]);
});
