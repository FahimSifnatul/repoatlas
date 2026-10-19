"""C++: same include rules as C (see c.py)."""
from .c import C


class Cpp(C):
    name = "C++"
    extensions = (".cpp", ".cc", ".hpp")
    color = "#f34b7d"
