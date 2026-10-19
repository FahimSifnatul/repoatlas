"""Lets `python -m repoatlas <repo>` work, and is what the single-file executable runs."""
from .cli import main

raise SystemExit(main())
