"""Scala: classes, traits and defs are listed. Linked through the names they use (see base.implicit)."""
from .base import Language


class Scala(Language):
    name = "Scala"
    extensions = (".scala",)
    color = "#c22d40"
    entry_re = r"\bdef\s+main\s*\(|\bextends\s+App\b"
