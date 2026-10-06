"""Planning helper shared by the catalog and CLI tests.

Importers put `tools/` on `sys.path` first, as each test module does.
"""

from pipeline.cli import make_plan, parser


def plan(*args):
    return make_plan(parser().parse_args(args))
