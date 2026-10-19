"""Swift: types and functions are listed. Linked through the names they use (see base.implicit)."""
from .base import Language


class Swift(Language):
    name = "Swift"
    extensions = (".swift",)
    color = "#F05138"
    entry_re = r"^\s*@main\b"
