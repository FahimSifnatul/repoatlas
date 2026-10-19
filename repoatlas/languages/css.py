"""CSS / SCSS style sheets: counted as the interface layer."""
from .base import Language


class Css(Language):
    name = "CSS"
    extensions = (".css", ".scss")
    color = "#563d7c"
    default_layer = "interface"   # styling is what people see
