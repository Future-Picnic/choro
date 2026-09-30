#!/usr/bin/env python3
from pathlib import Path

required = ["index.html", "styles.css", "choro_docs/product-brief.choro"]
missing = [path for path in required if not Path(path).is_file()]
if missing:
    raise SystemExit("Missing: " + ", ".join(missing))
print("Northstar fixture verified")
