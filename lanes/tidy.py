#!/usr/bin/env python3
"""Keeps the maintainer's checkouts of this repository from growing: this checkout and each of its
git worktrees. A daily systemd user timer on the lane machine runs it on the host; it only deletes
files, it builds and runs nothing.

  python3 lanes/tidy.py            # delete
  python3 lanes/tidy.py --dry-run  # only list what would go

Two things go.

Cargo build output no build has used for UNIT_DAYS days. A unit, one crate built one way (its
flags, target, profile, compiler), keeps .fingerprint/<package>-<hash>/ in its profile dir
(target/debug, target/<triple>/debug, ...), and Cargo reads it on every build the unit is part of;
the unit's outputs carry the same hash: deps/*-<hash>*, build/<package>-<hash>/,
examples/*-<hash>*. A version bump, a feature or flag change or a new compiler makes new units and
leaves the old ones behind for good: on 12.11.2026 target/ held 54 GB, 5,808 units in target/debug
alone, 20 builds of the app's test executable at 340 MB each among them. A file's atime says when
it was last read (to within a day on a relatime mount), so a unit whose fingerprint nothing has
read or written for UNIT_DAYS days goes, outputs and all, and so does an incremental cache
untouched for as long; a build that needs one again builds it again. Anything that reads every
file (grep -r over the checkout) makes all of it look used for UNIT_DAYS more days, so a target dir
still over TARGET_CAP_GB after the sweep goes whole. Cargo locks .cargo-lock in a profile dir while
it builds there: the sweep takes the same lock and leaves a busy dir to the next run. On a noatime
mount reads leave no trace, so only the cap applies.

Scratch in .tmp/ nothing has written for SCRATCH_DAYS days; reads don't count here, a search
through the checkout reads everything. A top-level entry goes whole, unless .tmp/KEEP names it: one
path relative to .tmp/ per line, # starts a comment. A path with a slash (remote/lc) keeps that
one, and the rest of its top-level entry is judged one level down the same way.
"""

import argparse
import fcntl
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UNIT_DAYS = 14
TARGET_CAP_GB = 20
SCRATCH_DAYS = 30
DAY = 86400
# A unit's hash at the end of its fingerprint dir's name and of its outputs' names (before the
# extension): libitem_parser-0274698eb08432f1.rlib, poe2_oracle-46da891be623ee2e, anyhow-02472fc0cb68ba3c.
UNIT_HASH = re.compile(r"-([0-9a-f]{16})(?=\.|$)")
TRASH = ".tidy-trash-"


def last_touch(path, reads):
    """When anything under `path` (or `path`, a file) was last written, or read too if `reads`: the
    latest mtime or ctime of its files (a copy or an unpacked archive keeps old mtimes, but its
    ctime is when it landed), and atime if `reads`; with no file in it, the latest of its dirs'.
    A dir's own times don't count otherwise: this sweep lists dirs, which sets their atime, and
    deletes in them, which sets their mtime and ctime."""

    def latest(st):
        return max(st.st_mtime, st.st_ctime, st.st_atime if reads else 0.0)

    try:
        own = path.lstat()
    except FileNotFoundError:
        return 0.0
    if not path.is_dir() or path.is_symlink():
        return latest(own)
    files, dirs = 0.0, max(own.st_mtime, own.st_ctime)
    for top, subdirs, names in os.walk(path):
        for name in names:
            try:
                files = max(files, latest(os.lstat(os.path.join(top, name))))
            except FileNotFoundError:
                continue
        for name in subdirs:
            try:
                st = os.lstat(os.path.join(top, name))
            except FileNotFoundError:
                continue
            dirs = max(dirs, st.st_mtime, st.st_ctime)
    return files or dirs


def size(path, seen):
    """Bytes `path` takes on disk, counting a hard-linked file once across calls sharing `seen`
    (Cargo hard-links the binaries it copies out of deps/)."""
    paths = [path] if not path.is_dir() or path.is_symlink() else (
        Path(top, name) for top, _, names in os.walk(path) for name in names
    )
    total = 0
    for file in paths:
        try:
            st = file.lstat()
        except FileNotFoundError:
            continue
        if st.st_nlink > 1:
            if (st.st_dev, st.st_ino) in seen:
                continue
            seen.add((st.st_dev, st.st_ino))
        total += st.st_blocks * 512
    return total


def human(n):
    return f"{n / 2**30:.1f} GB" if n >= 2**30 else f"{n / 2**20:.0f} MB"


def remove(path):
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path)
    else:
        path.unlink(missing_ok=True)


def lock(profile):
    """Cargo's own lock on a profile dir, taken without waiting: its descriptor, or None while a
    build holds it."""
    fd = os.open(profile / ".cargo-lock", os.O_RDWR | os.O_CREAT, 0o644)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(fd)
        return None
    return fd


def reads_leave_atime(path):
    """Whether the filesystem holding `path` records reads: not mounted noatime."""
    best, options = "", ""
    with open("/proc/self/mounts") as mounts:
        for line in mounts:
            fields = line.split()
            point = fields[1].replace("\\040", " ")
            inside = str(path) == point or str(path).startswith(point.rstrip("/") + "/")
            if inside and len(point) > len(best):
                best, options = point, fields[3]
    return "noatime" not in options.split(",")


def checkouts():
    listing = subprocess.run(
        ["git", "-C", str(ROOT), "worktree", "list", "--porcelain"],
        capture_output=True, text=True, check=True,
    ).stdout
    paths = [Path(line[len("worktree "):]) for line in listing.splitlines() if line.startswith("worktree ")]
    return [path for path in paths if path.is_dir()]


def profile_dirs(target):
    """target/<profile> and target/<triple>/<profile>: the dirs with a .fingerprint/."""
    found = [*target.glob("*/.fingerprint"), *target.glob("*/*/.fingerprint")]
    return sorted(fingerprints.parent for fingerprints in found if fingerprints.is_dir())


def target_dirs(checkout):
    """The checkout's Cargo target dirs, outermost only: target/ and any under .tmp/ up to
    .tmp/<name>/target. Cargo marks one with CACHEDIR.TAG, and so it does its own cache in
    CARGO_HOME, which has no profile dir."""
    found = []
    for top, subdirs, names in os.walk(checkout):
        here = Path(top)
        if "CACHEDIR.TAG" in names and profile_dirs(here):
            found.append(here)
            subdirs.clear()
        elif here == checkout:
            subdirs[:] = [name for name in subdirs if name != ".git"]
        elif len(here.relative_to(checkout).parts) >= 3:
            subdirs.clear()
    return found


def stale_units(profile, cutoff):
    """What goes from one profile dir: the fingerprint dirs, outputs and incremental caches of the
    units unused since `cutoff`."""
    fingerprints = list((profile / ".fingerprint").iterdir())
    stale = set()
    for fingerprint in fingerprints:
        match = UNIT_HASH.search(fingerprint.name)
        if match and last_touch(fingerprint, reads=True) < cutoff:
            stale.add(match.group(1))
    victims = []
    for folder in (profile / ".fingerprint", profile / "deps", profile / "build", profile / "examples"):
        if folder.is_dir():
            for entry in folder.iterdir():
                match = UNIT_HASH.search(entry.name)
                if match and match.group(1) in stale:
                    victims.append(entry)
    incremental = profile / "incremental"
    if incremental.is_dir():
        victims += [cache for cache in incremental.iterdir() if last_touch(cache, reads=True) < cutoff]
    return len(stale), victims


def keep_list(tmp):
    keep = set()
    if (tmp / "KEEP").is_file():
        for line in (tmp / "KEEP").read_text().splitlines():
            entry = line.split("#", 1)[0].strip().strip("/")
            if entry:
                keep.add(entry)
    return keep


def stale_scratch(folder, prefix, keep, cutoff):
    """What goes from .tmp/ (`folder` and `prefix` one level down): (entry, when last touched)."""
    victims = []
    for entry in sorted(folder.iterdir()):
        relative = prefix + entry.name
        if relative in keep or relative == "KEEP":
            continue
        if any(kept.startswith(relative + "/") for kept in keep):
            if entry.is_dir() and not entry.is_symlink():
                victims += stale_scratch(entry, relative + "/", keep, cutoff)
            continue
        touched = last_touch(entry, reads=False)
        # A whole target dir that a run moved here but didn't get to delete goes at once.
        if entry.name.startswith(TRASH) or touched < cutoff:
            victims.append((entry, touched))
    return victims


def wipe(target, trash):
    """Moves a whole target dir out of Cargo's way under the locks of all its profile dirs, then
    deletes it; False when a build holds one of them."""
    fds = []
    try:
        for profile in profile_dirs(target):
            fd = lock(profile)
            if fd is None:
                return False
            fds.append(fd)
        trash.parent.mkdir(exist_ok=True)
        target.rename(trash)
    finally:
        for fd in fds:
            os.close(fd)
    shutil.rmtree(trash)
    return True


def tidy(checkout, dry_run, now):
    """Tidies one checkout; returns the bytes freed (or that would be) and whether all went well."""
    print(checkout)
    did = "would delete" if dry_run else "deleted"
    freed, ok, seen = 0, True, set()

    tmp = checkout / ".tmp"
    scratch = stale_scratch(tmp, "", keep_list(tmp), now - SCRATCH_DAYS * DAY) if tmp.is_dir() else []
    for entry, touched in scratch:
        bytes_ = size(entry, seen)
        try:
            if not dry_run:
                remove(entry)
        except OSError as error:
            print(f"  ! {entry.relative_to(checkout)}: {error}")
            ok = False
            continue
        freed += bytes_
        print(f"  {did} {entry.relative_to(checkout)}: {human(bytes_)}, untouched {(now - touched) / DAY:.0f} days")

    atime = reads_leave_atime(checkout)
    if not atime:
        print(f"  ! mounted noatime: reads leave no trace, so only the cap of {TARGET_CAP_GB} GB applies")
    for target in target_dirs(checkout):
        if any(target.is_relative_to(entry) for entry, _ in scratch):
            continue
        going = 0
        for profile in profile_dirs(target) if atime else []:
            fd = None if dry_run else lock(profile)
            if not dry_run and fd is None:
                print(f"  {profile.relative_to(checkout)}: a build holds it, left to the next run")
                continue
            try:
                units, victims = stale_units(profile, now - UNIT_DAYS * DAY)
                bytes_ = sum(size(victim, seen) for victim in victims)
                if not dry_run:
                    for victim in victims:
                        remove(victim)
            except OSError as error:
                print(f"  ! {profile.relative_to(checkout)}: {error}")
                ok = False
                continue
            finally:
                if fd is not None:
                    os.close(fd)
            if victims:
                going += bytes_
                print(f"  {did} {units} units unused for {UNIT_DAYS} days from {profile.relative_to(checkout)}: {human(bytes_)}")
        freed += going
        name = target.relative_to(checkout)
        # A dry run deleted nothing: what would be left is the target dir less what would go.
        left = size(target, set()) - (going if dry_run else 0)
        if left <= TARGET_CAP_GB * 2**30:
            print(f"  {name}: {human(left)}")
        elif dry_run or wipe(target, tmp / f"{TRASH}{os.getpid()}"):
            freed += left
            print(f"  {did} {name} whole: {human(left)}, over the cap of {TARGET_CAP_GB} GB")
        else:
            print(f"  {name}: over the cap of {TARGET_CAP_GB} GB, but a build holds it; left to the next run")
    return freed, ok


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--dry-run", action="store_true", help="only list what would go")
    args = parser.parse_args()
    # One run at a time: the timer's and one by hand.
    with open(__file__) as me:
        try:
            fcntl.flock(me, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            sys.exit("tidy: another run is in progress")
        now, total, ok = time.time(), 0, True
        for checkout in checkouts():
            freed, fine = tidy(checkout, args.dry_run, now)
            total, ok = total + freed, ok and fine
        print(f"{'would free' if args.dry_run else 'freed'} {human(total)}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
