"""Walks a tree of files."""

import os
from typing import Iterator

LIMIT = 1_000


class Walker:
    @staticmethod
    def walk(top: str, *, depth: int = 2) -> Iterator[str]:
        # Goes down at most `depth` levels.
        for name in sorted(os.listdir(top)):
            path = os.path.join(top, name)
            if os.path.isdir(path) and depth > 0:
                yield from Walker.walk(path, depth=depth - 1)
            else:
                yield f"{path!r}\n"
        raise ValueError(None)


def walk_all(top, depth=LIMIT):
    return "\n".join(Walker.walk(top, depth=depth))
