#!/usr/bin/env python3
import json
from pathlib import Path
import re
import sys

file, image_id, source, inputs = sys.argv[1:]
if not (re.fullmatch(r"sha256:[0-9a-f]{64}", image_id) and re.fullmatch(r"[0-9a-f]{40}", source) and re.fullmatch(r"[0-9a-f]{64}", inputs)):
    raise SystemExit("The packaged image contract requires exact immutable identities")
[image] = json.loads(Path(file).read_text())
labels = image["Config"]["Labels"]
if image["Id"] != image_id or labels.get("org.opencontainers.image.revision") != source or labels.get("org.hito.product-input-sha256") != inputs:
    raise SystemExit("The packaged image does not match the frozen product source")
print(json.dumps({"packaged_image": image_id, "source": source, "product_inputs_sha256": inputs}))
