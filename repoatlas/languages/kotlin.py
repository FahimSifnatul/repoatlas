"""Kotlin: same package/import rules as Java, plus `object` declarations and top-level `fun`."""
from .java import Java


class Kotlin(Java):
    name = "Kotlin"
    extensions = (".kt",)
    color = "#A97BFF"
    type_keywords = "class|interface|enum|object"
    entry_re = r"^fun\s+main\s*\(|@SpringBootApplication"
    function_re = r"^(?:(?:public|private|internal|suspend)\s+)*fun\s+(?:<[^>]+>\s*)?(?:[\w.]+\.)?(\w+)\s*\("
