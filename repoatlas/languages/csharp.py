"""C#: classes and methods are listed. Linked through the names they use (see base.implicit)."""
from .base import Language


class CSharp(Language):
    name = "C#"
    extensions = (".cs",)
    color = "#178600"
    entry_re = r"\bstatic\s+(?:async\s+)?[\w<>]+\s+Main\s*\("
