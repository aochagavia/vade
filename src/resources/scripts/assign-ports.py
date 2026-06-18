#!/usr/bin/env python3
"""Assign ports for a vade deployment, substituting templated placeholders in the provided files

Usage:
    assign-ports.py PORT_COUNT ACTIVE_PORTS_FILE CANDIDATE_DIR APP_USER [TEMPLATED_FILE ...]

Each TEMPLATED_FILE is a path to a file that may contain port placeholders to substitute
"""

import glob
import os
import shutil
import sys

# Each vade app records its assigned ports in this file
ASSIGNED_PORTS_GLOB = "/opt/vade/apps/*/active-deployment/assigned-ports"
FIRST_PORT = 8000


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
    # `{{ vade.app.network.port }}` is shorthand for the first assigned port
    text = text.replace("{{ vade.app.network.port }}", str(ports[0]))
    # `{{ vade.app.network.ports[i] }}` refers to the i-th assigned port
    for i, port in enumerate(ports):
        text = text.replace("{{ vade.app.network.ports[" + str(i) + "] }}", str(port))
    return text


def main():
    reserve_count = int(sys.argv[1])
    active_ports_file = sys.argv[2]
    candidate_dir = sys.argv[3]
    app_user = sys.argv[4]
    templated_files = sys.argv[5:]

    ports = choose_ports(
        reserve_count,
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
            f.write(substitute_placeholders(content, ports))


if __name__ == "__main__":
    main()
