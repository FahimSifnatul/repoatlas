"""PHP: classes and functions are listed. Linked through the names they use (see base.implicit)."""
from .base import Language


class Php(Language):
    name = "PHP"
    extensions = (".php",)
    interpreters = ("php",)
    color = "#4F5D95"
