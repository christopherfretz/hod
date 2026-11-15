#!/usr/bin/env python3
"""lane-token.py [-n N] <command>...: run <command> holding N tokens
(default 1) of the GNU make jobserver that MAKEFLAGS names
(`--jobserver-auth=fifo:<path>`, make 4.4's form), and give them back
when it ends (WP-222).

ninja is not a jobserver client, so the LLVM builds' compiler wrappers
(wrap.sh) and the test runners (lit.sh, unittests.sh) take their tokens
here: a ninja of `-j 6` then runs as many compilers as the pool gives
it, like a make. The command inherits MAKEFLAGS, so a client inside it
(hod) counts the first token as its implicit one and takes more only
when they are idle. Without a fifo in MAKEFLAGS the command runs
directly. A token is given back on every exit, also when the runner is
stopped by SIGINT, SIGTERM or SIGHUP (the signal is passed on to the
command first); a SIGKILL loses it, as a killed make job does (the
pool's refill loop restores it)."""
import os
import signal
import subprocess
import sys


def fifo_from_makeflags(flags):
    path = None
    for word in flags.split():
        if word.startswith("--jobserver-auth="):
            path = word[len("--jobserver-auth="):]
    if path and path.startswith("fifo:") and len(path) > 5:
        return path[5:]
    return None


def main():
    args = sys.argv[1:]
    n = 1
    if len(args) >= 2 and args[0] == "-n":
        n = int(args[1])
        args = args[2:]
    if not args:
        print(__doc__.splitlines()[0], file=sys.stderr)
        return 2
    fifo = fifo_from_makeflags(os.environ.get("MAKEFLAGS", ""))
    if fifo is None or n < 1:
        os.execvp(args[0], args)
    child = None
    held = []
    fd = None

    def on_signal(signum, _frame):
        if child is not None and child.poll() is None:
            child.send_signal(signum)
        raise KeyboardInterrupt

    for s in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(s, on_signal)
    rc = 1
    try:
        fd = os.open(fifo, os.O_RDWR)
        while len(held) < n:
            b = os.read(fd, 1)
            if b:
                held.append(b)
        child = subprocess.Popen(args)
        rc = child.wait()
    except KeyboardInterrupt:
        if child is not None:
            try:
                rc = child.wait(timeout=30)
            except subprocess.TimeoutExpired:
                child.kill()
                rc = child.wait()
        rc = rc if rc else 130
    finally:
        if fd is not None:
            for b in held:
                os.write(fd, b)
            os.close(fd)
    return 128 - rc if rc < 0 else rc


if __name__ == "__main__":
    sys.exit(main())
