#!/usr/bin/env python3
"""Reserve ports for a vade deployment and substitute them into its templated files.

Usage:
    reserve-ports.py RESERVE_COUNT ACTIVE_PORTS_FILE CANDIDATE_DIR APP_USER
"""

import glob
import os
import shutil
import sys

# Each vade app records the ports it reserved in this file
RESERVED_PORTS_GLOB = "/opt/vade/apps/*/active-deployment/reserved-ports"
FIRST_PORT = 8000

# Files in the candidate deployment that may contain port placeholders.
TEMPLATED_FILES = ["Caddyfile", "app.service.backup"]


def read_ports(path):
    """Return the ports listed in `path` (one per line), or [] if it does not exist."""
    try:
        with open(path) as f:
            return [int(line) for line in f if line.strip()]
    except FileNotFoundError:
        return []


def gather_taken_ports():
    """Return the set of ports already reserved by any app's active deployment."""
    taken = set()
    for path in glob.glob(RESERVED_PORTS_GLOB):
        taken.update(read_ports(path))
    return taken


def choose_ports(reserve_count, active_ports, taken):
    """Pick the ports for this deployment.

    Reuse the active deployment's ports if it already reserved exactly the number we need
    (this keeps an app's ports stable across redeploys). Otherwise hand out the lowest
    free ports, starting at FIRST_PORT.
    """
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
    # `{{ APP_PORT }}` is shorthand for the first reserved port.
    text = text.replace("{{ APP_PORT }}", str(ports[0]))
    # `{{ APP_PORTS[i] }}` refers to the i-th reserved port.
    for i, port in enumerate(ports):
        text = text.replace("{{ APP_PORTS[" + str(i) + "] }}", str(port))
    return text


def main():
    reserve_count = int(sys.argv[1])
    active_ports_file = sys.argv[2]
    candidate_dir = sys.argv[3]
    app_user = sys.argv[4]

    ports = choose_ports(
        reserve_count,
        read_ports(active_ports_file),
        gather_taken_ports(),
    )

    # Record the reservation so other apps are aware of it
    candidate_ports_file = os.path.join(candidate_dir, "reserved-ports")
    with open(candidate_ports_file, "w") as f:
        for port in ports:
            f.write(str(port) + "\n")
    shutil.chown(candidate_ports_file, app_user, app_user)
    os.chmod(candidate_ports_file, 0o644)

    # Bake the chosen ports into the candidate's templated files.
    for name in TEMPLATED_FILES:
        path = os.path.join(candidate_dir, name)
        if not os.path.exists(path):
            continue
        with open(path) as f:
            content = f.read()
        with open(path, "w") as f:
            f.write(substitute_placeholders(content, ports))


if __name__ == "__main__":
    main()
