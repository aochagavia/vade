# Install the Caddy web server from its official apt repository and point it at vade-managed
# Caddyfiles. Meant to be applied as a non-root user with passwordless sudo rights.
from io import StringIO

from pyinfra import config, host
from pyinfra.operations import server, files, apt
from pyinfra.facts.files import File

config.SUDO = True

apt.packages(
    name="Install dependencies for Caddy",
    packages=["curl", "debian-keyring", "debian-archive-keyring", "apt-transport-https"],
    latest=True,
    update=True,
)

if not host.get_fact(File, path="/usr/share/keyrings/caddy-stable-archive-keyring.gpg"):
    server.shell(
        name="Download and install Caddy GPG key",
        commands=[
            "curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' "
            "| gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg"
        ],
    )

files.file(
    name="Set permissions on Caddy GPG keyring",
    path="/usr/share/keyrings/caddy-stable-archive-keyring.gpg",
    mode="644",
)

if not host.get_fact(File, path="/etc/apt/sources.list.d/caddy-stable.list"):
    server.shell(
        name="Add Caddy repository to sources list",
        commands=[
            "curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' "
            "| tee /etc/apt/sources.list.d/caddy-stable.list"
        ],
    )

files.file(
    name="Set permissions on Caddy sources list",
    path="/etc/apt/sources.list.d/caddy-stable.list",
    mode="644",
)

apt.packages(
    name="Install Caddy",
    packages=["caddy"],
    update=True,
)

files.put(
    name="Configure Caddy to automatically pick up deployed Caddyfiles",
    src=StringIO("import /opt/vade/apps/*/active-deployment/Caddyfile\n"),
    dest="/etc/caddy/Caddyfile",
    user="root",
    group="root",
    mode="644",
)
