"""Scala: classes, traits and defs are listed. Import links are not followed yet (add resolve() to enable them)."""
from .base import Language


class Scala(Language):
    name = "Scala"
    extensions = (".scala",)
    color = "#c22d40"
