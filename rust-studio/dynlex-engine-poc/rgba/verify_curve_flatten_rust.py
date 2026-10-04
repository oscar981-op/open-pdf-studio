#!/usr/bin/env python3
"""Diagnose independent bounded cubic subdivision against recorded Vello output."""
import argparse
import json
import subprocess
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--flattener", type=Path, required=True)
    parser.add_argument("--rasterizer", type=Path, required=True)
    parser.add_argument("--scene-tool", type=Path, required=True)
    parser.add_argument("--expected-report", type=Path)
    parser.add_argument("--json", type=Path)
    args = parser.parse_args()
    flat, raster, scene_tool = (getattr(args, name).resolve(strict=True)
                                for name in ("flattener", "rasterizer", "scene_tool"))
    report = {"note": "independent recursive de Casteljau candidate; Vello/Kurbo outline input remains a fixture", "fixtures": {}}
    with tempfile.TemporaryDirectory(prefix="opds-curve-flatten-") as folder:
        work=Path(folder)
        lines=work/"candidate.txt"
        output=work/"coverage.raw"
        scene=work/"candidate.opln"
        for name in ("arch","s-bend"):
            oracle=zlib.decompress((HERE/f"curve-coverage-{name}.zlib").read_bytes())
            reference_lines=[line for line in (HERE/f"curve-flat-{name}.txt").read_text().splitlines()
                             if line and not line.startswith('#')]
            variants={}
            for tolerance in (0.25,0.5):
                subprocess.run([flat,HERE/f"curve-outline-{name}.txt",lines,str(tolerance)],check=True,capture_output=True)
                subprocess.run([scene_tool,"encode",lines,scene],check=True,capture_output=True)
                subprocess.run([scene_tool,"validate",scene],check=True,capture_output=True)
                subprocess.run([raster,scene,output],check=True,capture_output=True)
                actual=output.read_bytes()
                if len(actual)!=256*192:
                    raise RuntimeError("wrong coverage buffer length")
                delta=[(i,x,y) for i,(x,y) in enumerate(zip(actual,oracle)) if x!=y]
                variants[str(tolerance)]={
                    "segments":len(lines.read_text().splitlines())-1,
                    "vello_segments":len(reference_lines),
                    "different_pixels":len(delta),
                    "absolute_error_sum":sum(abs(x-y) for _,x,y in delta),
                    "max_error":max((abs(x-y) for _,x,y in delta),default=0),
                    "first_differences":[{"x":i%256,"y":i//256,"candidate":x,"oracle":y}
                                         for i,x,y in delta[:5]],
                }
            report["fixtures"][name]=variants
        bad={
            "open":"M 0 0\nC 1 1 2 2 3 3\n",
            "nonfinite":"M 0 0\nC NaN 0 2 2 3 3\nZ\n",
            "unknown":"M 0 0\nQ 1 1 2 2\nZ\n",
            "bad-tolerance":"M 0 0\nL 1 0\nL 1 1\nZ\n",
        }
        for name,source in bad.items():
            path=work/f"bad-{name}.txt"
            path.write_text(source)
            lines.unlink(missing_ok=True)
            cmd=[flat,path,lines]
            if name=="bad-tolerance":cmd.append("0")
            result=subprocess.run(cmd,capture_output=True)
            if result.returncode==0 or lines.exists():
                raise RuntimeError(f"independent flattener accepted {name}")
        report["invalid_cases_rejected"]=len(bad)
    if args.expected_report and report!=json.loads(args.expected_report.read_text()):
        raise RuntimeError("flatten candidate changed from expected diagnostic")
    if args.json:args.json.write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps(report,indent=2))


if __name__=="__main__":main()
