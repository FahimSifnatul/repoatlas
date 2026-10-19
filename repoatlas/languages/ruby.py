"""Ruby: classes, modules and methods are listed. Linked through the names they use (see base.implicit)."""
from .base import Language


class Ruby(Language):
    name = "Ruby"
    extensions = (".rb", ".ru", ".rake")   # .ru: the Rack file a Ruby web server boots
    interpreters = ("ruby",)
    color = "#701516"
    entry_re = r"^if\s+(?:__FILE__\s*==\s*\$(?:0|PROGRAM_NAME)|\$(?:0|PROGRAM_NAME)\s*==\s*__FILE__)"
