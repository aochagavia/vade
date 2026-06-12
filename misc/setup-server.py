# Initial server setup: create an operator user with passwordless sudo and SSH access, harden root
# login, enable unattended upgrades, and set up a ufw firewall. Meant to be applied as root.
import os
from io import StringIO

from pyinfra.operations import server, files, apt
from pyinfra.api.exceptions import DeployError

operator_username = os.environ.get("VADE_USERNAME") or "operator"

public_key_path = os.environ.get("VADE_PUBLIC_KEY_PATH")
if not public_key_path:
    raise DeployError("VADE_PUBLIC_KEY_PATH environment variable is missing!")
public_key = open(public_key_path).read().strip()

###
# Setup operator user
###

files.put(
    name="Setup passwordless-sudo",
    src=StringIO("%sudo ALL=(ALL) NOPASSWD: ALL\n"),
    dest="/etc/sudoers.d/vade",
    user="root",
    group="root",
    mode="440",
)

server.shell(
    name="Validate the sudoers drop-in",
    commands=["visudo -cf /etc/sudoers.d/vade"],
)

server.user(
    name="Create a regular user with sudo privileges and SSH access",
    user=operator_username,
    shell="/bin/bash",
    groups=["sudo"],
    append=True,
    create_home=True,
    public_keys=[public_key],
)

files.line(
    name="Disable password authentication for root",
    path="/etc/ssh/sshd_config",
    line=r"^#?PermitRootLogin",
    replace="PermitRootLogin prohibit-password",
)

###
# Setup unattended upgrades
###

apt.packages(
    name="Set up unattended upgrades",
    packages=["unattended-upgrades"],
    latest=True,
    update=True,
)

# Setup ufw

# note: as of this writing, pyinfra core has no ufw operation, so we use the CLI (idempotency is )

apt.packages(
    name="Install ufw",
    packages=["ufw"],
    latest=True,
    update=True,
)

server.shell(
    name="Configure ufw (allow SSH/HTTP/HTTPS, deny everything else)",
    commands=[
        "ufw allow OpenSSH",
        "ufw allow 80/tcp",
        "ufw allow 443/tcp",
        "ufw default deny",
        "ufw --force enable",
    ],
)
