"""Vue single-file components: the <script> block is scanned like JavaScript."""
from .javascript import JavaScript


class Vue(JavaScript):
    name = "Vue"
    extensions = (".vue",)
    color = "#41b883"
    entry_by_name = False
    default_layer = "interface"   # components are what people see
