# Make Caddy issue self-signed certificates (so the e2e test can make HTTPS requests)
from pyinfra import config
from pyinfra.operations import files

config.SUDO = True

files.block(
    name="Ensure local_certs global block is at the top of the Caddyfile",
    path="/etc/caddy/Caddyfile",
    content="{\n    local_certs\n}",
    before=True,
    after=True,
)
