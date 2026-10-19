"""Ruby: classes, modules and methods are listed. Import links are not followed yet (add resolve() to enable them)."""
from .base import Language


class Ruby(Language):
    name = "Ruby"
    extensions = (".rb",)
    color = "#701516"
    entry_by_name = True
