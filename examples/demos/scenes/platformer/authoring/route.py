"""Authored Platformer route shared by scene assembly and gallery asset builds."""

import json
import math
from pathlib import Path
import sys


def route_points() -> list[tuple[float, float, float]]:
    return [
        (-9, -9, 1.0),
        (9, -9, 1.0),
        (9, 0, 1.0),
        (9, 6, 4.0),
        (9, 9, 4.0),
        (-9, 9, 4.0),
        (-9, 6, 4.0),
        (-9, 0, 1.0),
    ]


def route_document() -> dict:
    points = route_points()
    waypoints = []
    for index, point in enumerate(points):
        following = points[(index + 1) % len(points)]
        runtime = (point[0], point[2], -point[1])
        runtime_following = (following[0], following[2], -following[1])
        dx = runtime_following[0] - runtime[0]
        dz = runtime_following[2] - runtime[2]
        waypoints.append(
            {
                "position": list(runtime),
                "headingRadians": math.atan2(dx, dz),
            }
        )
    return {
        "version": 1,
        "coordinateSystem": "ipp-runtime-y-up",
        "closed": True,
        "rootEntityId": "platformer-root",
        "meshForward": [0, 0, 1],
        "waypoints": waypoints,
        "modes": {
            "walk": {"clip": "Platformer_Walk", "speed": 2.2},
            "run": {"clip": "Platformer_Run", "speed": 4.4},
            "crawl": {"clip": "Platformer_Crawl", "speed": 1.0},
        },
    }


if __name__ == "__main__":
    Path(sys.argv[1]).write_text(json.dumps(route_document(), indent=2) + "\n")
