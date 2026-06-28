#!/usr/bin/env python3
"""Assign ports for a vade deployment, substituting templated placeholders in the provided files

Usage:
    assign-ports.py ACTIVE_PORTS_FILE CANDIDATE_DIR APP_USER [TEMPLATED_FILE ...]

Each TEMPLATED_FILE is a path to a file that may contain port placeholders to substitute
"""

import glob
import os
import re
import shutil
import sys

# Each vade app records its assigned ports in this file
ASSIGNED_PORTS_GLOB = "/opt/vade/apps/*/active-deployment/assigned-ports"
FIRST_PORT = 8000

# A port placeholder looks like `port("<name>")` (emitted by the `port`
# jinja function). This captures the `<name>` part.
NAMED_PORT_RE = re.compile(r'{{ port\("([^"]+)"\) }}')


def read_ports(path):
    """Return the ports listed in `path` (one per line), or [] if it does not exist."""
    try:
        with open(path) as f:
            return [int(line) for line in f if line.strip()]
    except FileNotFoundError:
        return []


def gather_taken_ports():
    """Return the set of ports already assigned to active apps."""
    taken = set()
    for path in glob.glob(ASSIGNED_PORTS_GLOB):
        taken.update(read_ports(path))
    return taken


def gather_named_ports(templated_files):
    """Return a list of port names that appear in the templated files, ordered alphabetically"""
    names = set()
    for path in templated_files:
        with open(path) as f:
            content = f.read()

        names.update(NAMED_PORT_RE.findall(content))

    return sorted(names)

def choose_ports(reserve_count, active_ports, taken):
    """Pick the ports for this deployment."""
    # Reuse previously assigned ports if the count hasn't changed
    if len(active_ports) == reserve_count:
        return active_ports

    chosen = []
    port = FIRST_PORT
    while len(chosen) < reserve_count:
        if port not in taken:
            chosen.append(port)
        port += 1
    return chosen


def substitute_placeholders(text, ports):
    """Replace the port placeholders in `text` with concrete port numbers."""
    for name, port in ports:
        text = text.replace('{{ port("' + name + '") }}', str(port))
    return text


def main():
    active_ports_file = sys.argv[1]
    candidate_dir = sys.argv[2]
    app_user = sys.argv[3]
    templated_files = sys.argv[4:]

    port_names = gather_named_ports(templated_files)
    ports = choose_ports(
        len(port_names),
        read_ports(active_ports_file),
        gather_taken_ports(),
    )

    # Record the assignment so other apps are aware of it
    candidate_ports_file = os.path.join(candidate_dir, "assigned-ports")
    with open(candidate_ports_file, "w") as f:
        for port in ports:
            f.write(str(port) + "\n")
    shutil.chown(candidate_ports_file, app_user, app_user)
    os.chmod(candidate_ports_file, 0o644)

    # Bake the chosen ports into the candidate's templated files.
    for path in templated_files:
        with open(path) as f:
            content = f.read()
        with open(path, "w") as f:
            f.write(substitute_placeholders(content, zip(port_names, ports)))


if __name__ == "__main__":
    main()
