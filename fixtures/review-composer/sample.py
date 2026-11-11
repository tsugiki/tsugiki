"""A fixture for trying Tsugiki's review composer. Never merged."""

from dataclasses import dataclass, field


@dataclass
class Draft:
    path: str
    line: int
    body: str
    side: str = "RIGHT"


@dataclass
class PendingReview:
    number: int
    drafts: list[Draft] = field(default_factory=list)

    def add(self, draft: Draft) -> None:
        if not draft.body.strip():
            raise ValueError("a draft needs a body")
        self.drafts.append(draft)

    def remove(self, path: str, line: int) -> bool:
        before = len(self.drafts)
        self.drafts = [d for d in self.drafts if (d.path, d.line) != (path, line)]
        return len(self.drafts) < before

    def count(self) -> int:
        return len(self.drafts)


def body_fits(body: str, limit: int = 262_144) -> bool:
    """GitHub takes a comment body of at most 262,144 UTF-8 bytes."""
    return len(body.encode("utf-8")) <= limit


def summary(review: PendingReview) -> str:
    n = review.count()
    if n == 0:
        return f"#{review.number}: no drafts"
    return f"#{review.number}: {n} draft{'s' if n != 1 else ''}"


if __name__ == "__main__":
    review = PendingReview(367)
    review.add(Draft("sample.py", 12, "Is this still needed?"))
    print(summary(review))
