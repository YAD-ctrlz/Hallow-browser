# Stand-in for mozbuild.base (see __init__.py): the source tree is the PGO
# kit made by tools/pgo-kit.sh, named by HALLOW_PGO_TOPSRCDIR, and the
# browser is passed with --binary.
import os


class BinaryNotFoundException(Exception):
    def help(self):
        return "Pass the instrumented browser with --binary."


class MozbuildObject:
    def __init__(self, topsrcdir):
        self.topsrcdir = topsrcdir

    @classmethod
    def from_environment(cls, **kwargs):
        return cls(os.environ["HALLOW_PGO_TOPSRCDIR"])

    def get_binary_path(self, where=None):
        raise BinaryNotFoundException("no binary given")
