"""Language plug-ins.

Every module in this folder that defines a `Language` subclass is registered automatically.
To support a new language, add a file here (see ruby.py for the smallest example).
"""
import importlib
import pkgutil

from .base import Language, Project, trim  # noqa: F401  (re-exported for the core)


def _discover():
    found = []
    for info in sorted(pkgutil.iter_modules(__path__), key=lambda m: m.name):
        if info.name.startswith("_") or info.name == "base":
            continue
        mod = importlib.import_module(f"{__name__}.{info.name}")
        found += [c for c in vars(mod).values()
                  if isinstance(c, type) and issubclass(c, Language) and c.__module__ == mod.__name__ and c.name]
    return found


ALL = _discover()
BY_EXT = {}
for _cls in ALL:
    for _ext in _cls.extensions:
        if _ext in BY_EXT:
            raise RuntimeError(f"{_ext} is claimed by both {BY_EXT[_ext].name} and {_cls.name}")
        BY_EXT[_ext] = _cls
MANIFESTS = {m for c in ALL for m in c.manifests}
BY_INTERPRETER = {i: c for c in ALL for i in c.interpreters}
COLORS = {c.name: c.color for c in ALL}
