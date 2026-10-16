#!/usr/bin/env python3
"""Research audit: ordered DynLex image and text layers vs original PDF page 5."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path


SCALES = (0.25, 0.87, 1.0, 1.15, 1.5625, 2.0)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "text-only", "image-only", "worker", "image-candidate",
                 "hybrid-candidate", "backend", "output-dir"):
        p.add_argument("--" + name, type=Path, required=True)
    a = p.parse_args()
    a.output_dir.mkdir(parents=True, exist_ok=True)
    if a.text_only.stat().st_size > 16_000_000:
        raise ValueError("research text fixture exceeds worker input bound")
    cases = []
    for scale in SCALES:
        stem = f"original-page5-z{scale}"
        scene = a.output_dir / f"{stem}.pwpg"
        image = a.output_dir / f"{stem}.image.rgba"
        combined = a.output_dir / f"{stem}.dynlex-layers.rgba"
        oracle = a.output_dir / f"{stem}.hayro.rgba"
        with a.text_only.open("rb") as pdf, scene.open("wb") as output:
            subprocess.run([str(a.worker), "-", "1", str(scale), "-", "--scene-worker"],
                           stdin=pdf, stdout=output, stderr=subprocess.PIPE,
                           check=True, timeout=20)
        subprocess.run([str(a.image_candidate), str(a.image_only), str(image), str(scale)],
                       capture_output=True, check=True, timeout=120)
        result = subprocess.run([str(a.hybrid_candidate), str(scene), str(image), str(combined)],
                                capture_output=True, text=True, check=True, timeout=120)
        subprocess.run([str(a.backend), str(a.source), str(oracle), "--hayro",
                        f"--scale={scale}", "--page=5"],
                       capture_output=True, check=True, timeout=120)
        left, right = combined.read_bytes(), oracle.read_bytes()
        if len(left) != len(right) or len(left) > 6_000_000 * 4:
            raise ValueError("candidate/oracle length or output bound")
        errors = [i for i, (x, y) in enumerate(zip(left, right)) if x != y]
        cases.append({"scale": scale, "rgba_bytes": len(left),
                      "worker_scene_bytes": scene.stat().st_size,
                      "image_sha256": sha(image), "scene_sha256": sha(scene),
                      "candidate_sha256": sha(combined), "hayro_sha256": sha(oracle),
                      "different_channels": len(errors),
                      "different_pixels": len({i // 4 for i in errors}),
                      "max_channel_error": max((abs(left[i] - right[i]) for i in errors), default=0),
                      "hybrid_status": result.stderr.strip()})
    report = {"scope": "manual research layer isolation, not native original-page acceptance",
              "original_sha256": sha(a.source), "text_only_sha256": sha(a.text_only),
              "image_only_sha256": sha(a.image_only),
              "worker_sha256": sha(a.worker), "image_candidate_sha256": sha(a.image_candidate),
              "hybrid_candidate_sha256": sha(a.hybrid_candidate),
              "backend_sha256": sha(a.backend), "cases": cases}
    path = a.output_dir / "tekst-page5-hybrid-layer-audit-2026-10-09.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print([(row["scale"], row["different_channels"]) for row in cases])
    if any(row["different_channels"] for row in cases):
        raise SystemExit("layered candidate differs from original PDF")


if __name__ == "__main__":
    main()
