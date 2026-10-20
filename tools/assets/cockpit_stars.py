"""Prepare a small offline Hipparcos catalogue for the cockpit star mesh."""

import csv
import hashlib
import io
import json
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
QUERY = (
    "https://vizier.cds.unistra.fr/viz-bin/asu-tsv?"
    "-source=I/239/hip_main&Vmag=%3C6&"
    "-out=HIP,RAICRS,DEICRS,Vmag,B-V&-out.max=10000"
)


def prepare():
    cached = ROOT / "target/cockpit-stars/hipparcos.tsv"
    cached.parent.mkdir(parents=True, exist_ok=True)
    if not cached.exists():
        with urllib.request.urlopen(QUERY, timeout=60) as response:
            cached.write_bytes(response.read())
    payload = cached.read_bytes()
    rows = []
    for line in payload.decode().splitlines():
        values = line.split("\t")
        if len(values) != 5 or not values[0].strip().isdigit():
            continue
        hip, ra, dec, magnitude, color = values
        if not ra.strip() or not dec.strip():
            continue
        rows.append(
            [
                int(hip),
                round(float(ra), 5),
                round(float(dec), 5),
                float(magnitude),
                float(color) if color.strip() else 0.65,
            ]
        )
    if not 4000 < len(rows) < 6000 or len({r[0] for r in rows}) != len(rows):
        raise ValueError("Incomplete or duplicated bright-star catalogue")
    rows.sort()
    buffer = io.StringIO(newline="")
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(["hip", "ra_degrees", "dec_degrees", "magnitude_v", "b_minus_v"])
    writer.writerows(rows)
    data = buffer.getvalue().encode()
    output = ROOT / "examples/demos/scenes/cockpit/authoring/stars"
    output.mkdir(exist_ok=True)
    (output / "hipparcos.csv").write_bytes(data)
    (output / "source.json").write_text(
        json.dumps(
            {
                "credit": "The Hipparcos and Tycho Catalogues (ESA 1997), CDS I/239",
                "catalogue": "https://cdsarc.cds.unistra.fr/viz-bin/cat/I/239",
                "query": QUERY,
                "selection": "Johnson V magnitude < 6; ICRS positions, epoch J1991.25",
                "processing": "Rows without positions omitted; positions rounded to 0.00001 degree; missing B-V defaults to 0.65. Runtime exposure is artistic, not photometric calibration.",
                "rows": len(rows),
                "sourceSha256": hashlib.sha256(payload).hexdigest(),
                "preparedSha256": hashlib.sha256(data).hexdigest(),
            },
            indent=2,
        )
        + "\n"
    )
    print(f"Prepared {len(rows)} stars ({len(data)} bytes)")


if __name__ == "__main__":
    prepare()
