"""Svelte components: the <script> block is scanned like JavaScript."""
from .javascript import JavaScript


class Svelte(JavaScript):
    name = "Svelte"
    extensions = (".svelte",)
    color = "#ff3e00"
    entry_by_name = False
    default_layer = "interface"   # components are what people see
