"""HTML pages and templates: counted as the interface layer."""
from .base import Language


class Html(Language):
    name = "HTML"
    extensions = (".html",)
    color = "#e34c26"
    default_layer = "interface"   # pages are what people see
    entry_by_name = False
    markup = True
    implicit = "none"             # pages link files by path (<script src>, href)
