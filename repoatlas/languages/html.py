"""HTML pages and templates: counted as the interface layer."""
from .base import Language


class Html(Language):
    name = "HTML"
    extensions = (".html",)
    color = "#e34c26"
    default_layer = "interface"   # pages are what people see
