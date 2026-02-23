# Vade

> A command-line tool to deploy web applications on Linux servers

Vade (short for _va_nilla _de_ploy) is a lightweight tool for single-server deployments. It builds directly on top of
existing Linux abstractions (e.g., ssh, files, users, systemd, etc), instead of reinventing the
wheel. As a side-effect, you have access to the full arsenal of Linux tools when setting up a
deployment and when troubleshooting.

Following this minimalistic philosophy, vade never talks directly to the server, but
delegates communication to ansible instead. If you squint a little, you could say vade is
an ansible playbook generator with built-in conventions and utility commands. It makes choices about
how apps are deployed, how secrets are stored, how TLS certificates are handled, etc.

### Is it for me?

If you have Linux experience and are tired of chasing every year's shiny new hype, vade is
your friend. More specifically, vade might fit the bill when you:

- Are deploying to a single server, not to a fleet.
- Can tolerate reasonable downtime between deployments.
- Want full access to your infrastructure, with few abstractions in the way of the tools you know.
- Want to avoid over-engineering, without accidentally giving up engineering.
- Feel attracted to existing lightweight deployment approaches (e.g., [dokku](https://dokku.com/),
  [coolify](https://coolify.io), etc), but would rather avoid third-party dependencies that touch so
  many parts of your system.

### Status

Highly experimental. Use at your own risk.

## Usage

#### Whirlwind tour

Assuming your server and client machines satisfy the [requirements](#requirements), you can deploy a
static site by running the script below. Make sure to replace `<your-domain>` and `<username>` by
suitable values before running the script.

```bash
# Install the vade CLI from source
cargo install vade-cli

# Initialize a "Hello world" static website (in the current working directory)
vade scaffold static

# Set the domain to something you own instead of `example.com`
sed -i 's/example\.com/<your-domain>/g' infra/Caddyfile

# Generate the ansible playbook and related files (at `./vade-gen`)
vade deploy infra/vade.json my-static-site

# Optional: inspect the generated playbook to ensure it's not going to destroy anything you care about

# Actually deploy to the server!
ansible-playbook vade-gen/playbook.yml -i "<your-domain>," -u <username>
```

After running this to completion, and assuming you have set the necessary DNS records, you can visit
`https://<your-website>` from your browser and see "Hello world" appear on the screen!

#### Day-to-day operations

TBD

## Requirements

#### Server

Vade should work on any Linux server with a writable filesystem, as long as the following
dependencies are present:

- `systemd`
- `rsync`
- `caddy` (the HTTP server)
- `python` (necessary for ansible connections to the server).

None of these dependencies require additional configuration, except Caddy. Its configuration file
(usually at `/etc/caddy/Caddyfile`) should contain the line `import /opt/vade/apps/*/active-deployment/Caddyfile`,
instructing Caddy to automatically pick up the routing configuration of deployed web applications.

#### Client

The vade CLI does not have system dependencies, but since it generates ansible playbooks
you will need:

- `ansible`
- `rsync` (used by ansible when synchronizing directories)

## Applications

### Application structure (server)

A deployed application always has:

- Its own system user (i.e., what you get by running `useradd --system <app>`).
- Its own root directory, at `/opt/vade/apps/<app>`.
- An active deployment, at `/opt/vade/apps/<app>/active-deployment`.

Next to that, an application can have:

- `/opt/vade/apps/<app>/storage`, a directory for persistent storage.
- `/opt/vade/apps/<app>/secrets`, a file containing secrets (see [configuring secrets](#configuring-secrets)
  for details).
- `/opt/vade/apps/<app>/active-deployment/artifacts`, a directory for the currently deployed artifacts (e.g.,
  your application's binary).
- `/opt/vade/apps/<app>/active-deployment/Caddyfile`, the current Caddy configuration file (e.g., specifying
  how HTTP requests are routed to your application).
- `/etc/systemd/system/<app>.service`, the currently deployed systemd unit file, governing
  how the application runs.

The components mentioned above are automatically provisioned by vade. They make it
possible to create:

- Static sites (see [`examples/static-site`](./examples/static-site)): all it takes is a set of
  files under `artifacts` and a suitable `Caddyfile`.
- Web applications (see [`examples/guestbook`](./examples/guestbook)): the app's entrypoint and
  dependencies are bundled together under `artifacts`, persistent storage is available under
  `storage`, and secrets are provided in `secrets`. This assumes a suitable systemd unit file and
  `Caddyfile`.
- Containerized web applications: we do not support it yet, but deploying containers would be
  feasible using [podman
  quadlets](https://docs.podman.io/en/latest/markdown/podman-quadlet.1.html). It should be a matter
  of using a proper quadlet file and `Caddyfile`.

### Application structure (local)

Locally, an application is defined by a `vade.json` file like the one shown below:

```json
{
  "artifacts_dir": "artifacts",
  "caddyfile_path": "infra/Caddyfile",
  "systemd_unit_path": "infra/app.service.j2"
}
```

As you can see, these paths are the local counterparts to the server's
`/opt/vade/apps/<app>/active-deployment` from the previous section. Each path is optional, so you get to
assemble whatever combination floats your boat. Some combinations make no sense, though, and the CLI
will warn you in that case instead of proceeding.

Note: the Caddyfile and systemd unit files you use can use jinja for templating. Refer to the
source code for the exact variables that are available.

### Application lifecycle

### Local scaffolding

To create an application fully from scratch, you can run `vade scaffold` on your local
machine. See `vade scaffold --help` for details on the available options. All of them will
create a `vade.json` file and any files referenced by it.

#### Configuring secrets

The systemd unit generated by `vade scaffold webapp` loads secrets from
`/opt/vade/apps/<app>/secrets` using systemd's `EnvironmentFile` feature. You may also load secrets through
other means, of course, but this section assumes you are using this mechanism.

The `/opt/vade/apps/<app>/secrets` file should adhere to the syntax accepted by `EnvironmentFile`, showcased below:

```
DATABASE_HOST=localhost
DATABASE_PORT=5432
API_KEY=abc123
QUOTED_VALUE="e=mc²"
```

For additional security, the secrets file should be owned by the root user, and only be readable /
writable by root[^secrets-file-root]. If you need to configure secrets before the first deployment,
you can execute `vade setup <app> --secrets` and run the resulting playbook. The file will
then be created on the server, with the proper permissions.

To avoid reinventing the wheel, vade does not provide commands for secrets management.
Since the secrets file is a regular text file, the recommended way to manipulate secrets is to ssh
into your machine and modify the file using the tools you like best. When modifying secrets, note
that changes to the file will not have an effect until you restart the service (`systemctl restart
<app>.service`).

#### Deployment

Deployment playbooks are generated through `vade deploy <app>`. Leaving aside any setup steps and
sanity checks, a deployment works as follows.

**Initialize deployment candidate:**

- Create `/opt/vade/apps/<app>/candidate-deployment` (previously deleting it if it already existed).
- If present, mirror the local artifacts to `/opt/vade/apps/<app>/candidate-deployment/artifacts`.
- If present, upload the local systemd unit to `/opt/vade/apps/<app>/candidate-deployment/app.service.backup`.
- If present, upload the local `Caddyfile` to `/opt/vade/apps/<app>/candidate-deployment/Caddyfile`.

**Promote deployment candidate to active deployment:**

- If present on the server, disable the application's systemd unit (at
  `/etc/systemd/system/<app>.service`) and stop the service. Then delete the unit file.
- Rename `/opt/vade/apps/<app>/active-deployment` to `/opt/vade/apps/<app>/previous-deployment` (fully overwriting the
  target directory if it already existed).
- Rename `/opt/vade/apps/<app>/candidate-deployment` to `/opt/vade/apps/<app>/active-deployment`.
- If the candidate deployment has a systemd unit, copy it to `/etc/systemd/system/<app>.service`,
  run `systemctl daemon-reload`, enable it, and start the service.
- Run `systemctl reload caddy` so Caddy picks up the new `Caddyfile`.

**Rollback (in case of failure)**:

- Delete the broken `/opt/vade/apps/<app>/active-deployment`.
- Attempt to promote `/opt/vade/apps/<app>/previous-deployment`.

Note: if rolling back fails, vade assumes something is Very Wrong and will not attempt
further fixes. It is up to the user to diagnose and fix the root issue (e.g., ssh into the machine
to fix whatever is broken, then trigger a new deploy as usual).

### Dependency management

Since vade is not tied to a specific mechanism of shipping dependencies, you are
responsible for choosing one when you deploy an application. Some options are:

- Ship self-contained binaries that have no dependencies at all. Our [`guestbook example`](./examples/guestbook)
  does just that: the `just compile` command compiles the Rust program and places the resulting
  binary in `artifacts/guestbook`.
- Install dependencies globally on the host, so they are available for all applications. Our
  [`python-no-deps`](./examples/python-no-deps/) example does that (it assumes there is a suitable
  python interpreter installed on the server).
- Bundle dependencies with your deployment artifacts, so they are available for that specific
  application.
- Do something creative, like automatically installing dependencies on the server when an
  application runs.
- Once we support podman quadlets: bundle your application together with its dependencies in a container image. Ship it over ssh to
  the server (assuming it is possible, as explained in [this Red Hat blog
  post](https://www.redhat.com/en/blog/podman-transfer-container-images-without-registry)). Use a podman quadlet to run it.

[^secrets-file-root]: This is a defense-in-depth mechanism. Even in case of a serious security
    breach, such as an attacker having remote code execution through the deployed application,
    attempts to access the secrets file would fail.
