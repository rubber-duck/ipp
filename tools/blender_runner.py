"""Run the real addon in background Blender with a cooperative main-thread loop."""

import argparse
import json
from pathlib import Path
import runpy
import signal
import sys
import time

import bpy


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ready-file", required=True, type=Path)
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--blend", type=Path)
    parser.add_argument("--control-file", type=Path)
    parser.add_argument("--port", type=int, default=8118)
    parser.add_argument("--certificate", default="")
    parser.add_argument("--private-key", default="")
    parser.add_argument("--allowed-origin", action="append", default=[])
    parser.add_argument("--viewer-url", default="")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])
    root = Path(__file__).resolve().parents[1]
    sys.path[:0] = [
        str(root / "target/blender/dependencies"),
        str(root / "integrations/blender"),
    ]
    import ipp_blender

    if args.blend:
        bpy.ops.wm.open_mainfile(
            filepath=str(args.blend.resolve()), load_ui=False, use_scripts=False
        )
    fixture = runpy.run_path(str(args.fixture)) if args.fixture else {}
    if "create_scene" in fixture:
        fixture["create_scene"]()
    running = True

    def terminate(*_args):
        nonlocal running
        running = False

    signal.signal(signal.SIGTERM, terminate)
    signal.signal(signal.SIGINT, terminate)
    server = ipp_blender.start(
        directory=args.ready_file.parent / "credentials",
        port=args.port,
        certificate=args.certificate,
        private_key=args.private_key,
        allowed_origins=args.allowed_origin,
        viewer_url=args.viewer_url,
    )
    args.ready_file.parent.mkdir(parents=True, exist_ok=True)
    import os

    args.ready_file.write_text(
        json.dumps(
            {
                "origin": server.origin,
                "token": server.token,
                "session": server.session,
                "revision": server.revision,
                "pid": os.getpid(),
                "certificate": str(server.certificate_paths[0]),
                "private_key": str(server.certificate_paths[1]),
                "blender_version": bpy.app.version_string,
            }
        )
    )
    last_sequence = None
    try:
        while running:
            server.pump()
            if args.control_file and args.control_file.is_file():
                try:
                    command = json.loads(args.control_file.read_text())
                except (ValueError, OSError):
                    command = {}
                sequence = command.get("sequence")
                if sequence is not None and sequence != last_sequence:
                    last_sequence = sequence
                    result = {"sequence": sequence}
                    try:
                        fixture["apply_command"](command)
                        server.sync()
                        result["revision"] = server.revision
                    except Exception as error:
                        result["error"] = str(error)
                    args.ready_file.with_suffix(".control.json").write_text(
                        json.dumps(result)
                    )
            # Background owns this loop; GUI mode uses bpy.app.timers instead.
            time.sleep(0.002)
    finally:
        ipp_blender.stop()


if __name__ == "__main__":
    main()
