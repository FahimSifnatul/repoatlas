"""TypeScript: same syntax family and import rules as JavaScript (see javascript.py)."""
from .javascript import JavaScript


class TypeScript(JavaScript):
    name = "TypeScript"
    extensions = (".ts", ".tsx")
    color = "#3178c6"
    skip_suffixes = (".d.ts",)   # type declarations describe code, they are not code
