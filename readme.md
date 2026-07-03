# Vade

> A command-line tool to deploy applications on Linux servers

Vade (short for _va_nilla _de_ploy) is a lightweight tool for single-server deployments. It builds
directly on top of existing Linux abstractions (ssh, files, users, systemd, etc), instead of
reinventing the wheel. As a side-effect, you have access to the full arsenal of Linux tools when
setting up a deployment and when troubleshooting.

Status: experimental, meant for hobby usage only. Expect bugs.

## Why?

Like many other software projects, vade was born out of frustration with existing solutions.
Most tools out there are pretty heavy for a hobby setup, which is my current use case.
By no means do I want Kubernetes in my hobby VPS, and even lighter alternatives
like [Dokku](https://dokku.com/) feel more bloated than strictly required. What if we could rely
on proven Linux fundamentals instead?

## Quick start

#### Installation

For simplicity, let's assume your server has already been set up (see [server setup](#server-setup)),
so we only have to care about the client side. You can install the `vade` CLI by running the following command:

```bash
cargo install --locked vade-cli
```

Next to that, you need to have `pyinfra` and `rsync` installed on your system[^pixi].

#### Building blocks

A vade application is built from:

- **Artifacts**: zero or more files of any kind (HTML files, binaries, etc).
- Zero or more **systemd unit files** of any kind (services, timers, etc).
- An optional **Caddyfile**, wiring your application to the Caddy reverse proxy so it gets exposed to the outside world over HTTP.

These building blocks are configured through a `vade.toml` file, which vade loads on startup
(more on this [later](#anatomy-of-a-vadetoml-file)). If your application follows a common pattern
(e.g., static site, reverse-proxied web application), you will most likely get to skip writing
systemd unit files and Caddyfiles by hand, since vade ships built-in templates you can reuse.

#### Example apps

Artifacts, systemd units, and an optional Caddyfile are quite versatile. They allow you to deploy a
wide range of applications, such as:

- A static website (see [examples/static-site/vade.toml](./examples/static-site/vade.toml))
- A self-built webapp (see [examples/guestbook/vade.toml](./examples/guestbook/vade.toml))
- A third-party webapp like [GoatCounter](https://www.goatcounter.com/) (see [examples/goatcounter/vade.toml](./examples/goatcounter/vade.toml))
- A background-processing program on a schedule (see [examples/timer/vade.toml](./examples/timer/vade.toml))

Check out their respective `vade.toml` files for details.

#### Your first deployment

Let's deploy the static site from the examples. For this you will need to:

1. Have a domain name pointing to your server.
2. Follow the steps outlined below, replacing the `<your-domain>`, `<ssh-user>` and `<ssh-host>` placeholders with suitable values.

Here we go!

```bash
# Clone the vade repository and cd into the static site directory
git clone https://github.com/aochagavia/vade.git
cd vade/examples/static-site

# Prepare the deployment of the app, which will be called `my-site` on the server.
#
# This step is fully local to your machine, it does not communicate with
# your server at all. It merely generates a deployment script and some files
# under `vadegen`.
#
# The `--set` flag overrides the domain name that `vade.toml` originally
# configured for the Caddyfile.
vade deploy my-site --set 'caddyfile.vars.domains=["<your-domain>"]'

# Optional: inspect `vadegen/execute.py`

# Actually deploy to the server
pyinfra -y --user <ssh-user> <ssh-host> vadegen/execute.py
```

After running this to completion, you can visit `https://<your-domain>` from your browser and see
"Hello world" appear on the screen!

As you can deduce from the above, vade itself never talks directly to the server, but
delegates communication to [pyinfra](https://pyinfra.com/) instead (think
[Ansible](https://github.com/ansible/ansible), but faster and better suited for code generation).
This gives you the opportunity to fully understand the exact deployment procedure, should you need
to.

#### Server-side app structure

From the perspective of the server, a deployed application lives under
`/opt/vade/apps/<app-name>` and has the following components:

- `active-deployment`: a directory containing the artifacts, Caddyfile and systemd units for the current deployment (note that systemd units get installed to `/etc/systemd/system`, so the unit files in `active-deployment` are inactive copies).
- `storage`: a directory where the app may store files that need to be persisted across runs and deployments (e.g., a SQLite database).
- `secrets`: a file where you may configure secrets and environment variables, also independent from runs and deployments, for injection into systemd-managed processes (e.g., through the `EnvironmentFile` option). The file is only read/writable by the root user, as a defense-in-depth mechanism.

Besides the filesystem structure mentioned above, a vade app gets its own system user (with the
same name as the app). This user owns the files mentioned above (except `secrets`) and runs your
app (though you can deviate from this, if you define your own systemd unit files rather than
using the built-in ones).

Finally, artifacts are meant to be immutable once they reach the server, as vade uses hardlinks to
avoid duplicating artifacts across deployments.

#### Creating without deploying

For apps that require secrets, deploying them before any secrets are configured is likely to result
in failure. In such cases, you can:

1. Run `vade create <app-name>` (followed by `pyinfra`) to initialize the server-side app structure.
2. SSH into the server and edit `/opt/vade/apps/<app-name>/secrets` to your heart's delight.
3. Run `vade deploy <app-name>` (followed by `pyinfra`) to actually deploy.

You can test this out by deploying the [example guestbook app](./examples/guestbook/readme.md),
which requires secrets to work properly.

#### Removing apps

Removing apps is analogous to creating them:

```bash
vade destroy <app-name>
pyinfra -y --user <ssh-user> <ssh-host> vadegen/execute.py
```

Warning: this will remove all server-side state related to your app (i.e., the whole
`/opt/vade/apps/<app-name>` directory, any installed systemd units, and the Linux user).

## Vade in depth

#### Anatomy of a `vade.toml` file

Let's have a look at the `vade.toml` file used by the [GoatCounter example](./examples/goatcounter/).
The snippet below is copied from [here](./examples/goatcounter/vade.toml) and enriched with comments
to illustrate the meaning of each part.

```toml
# This app's artifacts can be found under the `artifacts` dir, resolved
# relative to the `vade.toml` file
[artifacts]
path = "artifacts"

# This app has a Caddyfile, based on a builtin template called
# `reverse-proxy`. The template requires the user to provide a `domains`
# variable consisting of a list of domain names. If you look at the Caddyfile
# template (linked after this code block), you will see that Caddy will route
# requests addressed at any of those domains to the port used by the app.
[caddyfile.template]
builtin = "reverse-proxy"
vars = {
  domains = ["goats.example.com"]
}

# This app has one systemd unit, based on a builtin template called
# `webapp.service`. The template requires the user to provide an `exec_start`
# variable consisting of a string. If you look at the systemd unit template
# (linked after this code block), you will see that the contents of
# `exec_start` are assigned to the unit's `ExecStart` option. In other words,
# `exec_start` tells systemd how to run your app.
[[systemd-unit]]
[systemd-unit.template]
builtin = "webapp.service"
vars = {
  exec_start = "{{ vade.app.paths.artifacts }}/goatcounter serve -listen :{{ port('main') }}"
}

# Note how the value of `exec_start` is itself templated (more on that later):
# - `vade.app.paths.artifacts` resolves to the path on the server where the
#   currently deployed artifacts are stored.
# - `port('main')` resolves to the port assigned to that name during deployment.
```

Links:

- [reverse-proxy](./src/resources/caddyfile-templates/reverse-proxy.j2) Caddyfile template.
- [webapp.service](./src/resources/systemd-unit-templates/webapp.service.j2) systemd unit template.

Misc:

- Next to `builtin` templates, you can have `file` templates (i.e., a path to a file) and
  `inline` templates (i.e., a string to be used as the Caddyfile or system unit).
- When you have more than one systemd unit for the same app, you need to make sure they have
  different names (otherwise vade will refuse to deploy). By default, systemd unit files are named
  `<app-name>.service`. You can influence the final name in two ways, which can be combined:
  - Setting the unit's `file-extension` property, which changes the name to
    `<app-name>.<file-extension>`.
  - Setting the unit's `filename-suffix` property, which changes the name to
    `<app-name>-<filename-suffix>.<file-extension>`.

#### Anatomy of the `vadegen` output directory

When running `vade deploy <app-name>`, vade generates a `vadegen` directory
with the following files:

- One `execute.py` file, to be run with `pyinfra`. That script is what actually runs the deployment against the server.
- The app's Caddyfile, if one was configured in `vade.toml`.
- The app's systemd unit files, if they were configured in `vade.toml`.

The Caddyfile and systemd unit files are written to `vadegen` after rendering. That means you will
no longer find in them any templating expressions. The only exception is
`{{ port('<name>') }}`, which gets rendered on the server (more on that [here](#more-on-templating)).

If you want to know the exact operations that a deployment will run on your
server, run `vade deploy` and have a look at the generated
`vadegen/execute.py`.

#### More on templating

The underlying templating engine is [minijinja](https://github.com/mitsuhiko/minijinja/tree/main).
When creating templated files, you will have access to minijinja's built-in
features and to the following variables:

- `vade.app.name`: the app's name.
- `vade.app.username`: the app's associated Linux username (currently the same as `vade.app.name`).
- `vade.app.paths.secrets`: the path to the app's secrets file.
- `vade.app.paths.storage`: the path to the app's storage directory.
- `vade.app.paths.artifacts`: the path to the app's currently deployed artifacts directory.

Additionally, the `port(<name>)` function can be used to refer to a port by its name. If you use
the same name across files in the same app (e.g., once in the Caddyfile, once in the systemd unit),
vade will make sure both get replaced with the exact same port number. That way, you can
reverse-proxy traffic from Caddy to your running application without having to manually assign
port numbers.

#### Port assignment logic

Vade assumes there is a "reasonable amount" of TCP ports available, from port 8000 and up. When
deploying an application, it counts the number of ports that the app needs (based on the app's
`{{ port('name') }}` expressions), finds available port numbers, rewrites each instance of
`{{ port('name') }}` to the corresponding port number, and finally writes an app-specific
`assigned-ports` file that lists the assigned port numbers.

Finding the next available port number is a matter of looking at the `assigned-ports` of each
deployed application, then finding the port that is closest to 8000 and has not yet been assigned.
The specific code lives in [assign-ports.py](./src/resources/scripts/assign-ports.py), which gets
transferred to the server as part of the `server-setup` command.

As a corollary, make sure your server is not running software that listens on TCP ports close to
8000, as that would result in conflicts when vade assigns those ports to its own apps.

## Server setup

Vade should work on any Linux server with a writable filesystem, as long as the following
dependencies are present:

- `systemd`
- `rsync`
- `caddy`
- `python`

There are two further setup steps:

1. You need to configure Caddy
   so it automatically picks up your deployed app's Caddyfiles. For that purpose, the main Caddy
   configuration file (usually at `/etc/caddy/Caddyfile`) should contain the line
   `import /opt/vade/apps/*/active-deployment/Caddyfile`. See
   [misc/setup-caddy.py](./misc/setup-caddy.py) for a pyinfra script that installs and configures
   Caddy for you.
2. Before running vade deployments, you need to run the server-setup script as follows:

```bash
vade server-setup
pyinfra -y --user <ssh-user> <ssh-host> vadegen/execute.py
```

## Design considerations

Vade is tuned for scenarios where you:

- Are deploying to a single server, not to a fleet.
- Can tolerate a few seconds of downtime between deployments.
- Want TLS certificates to be automatically obtained and renewed.
- Want full access to your infrastructure, with few abstractions in the way of the tools you know.
- Don't need to restore old deployments.

As always, this comes with limitations:

- If you have binary artifacts, you are responsible for ensuring they can actually run on the
  server. For instance, they should be Linux binaries and target the CPU architecture used by
  your server.
- If you are deploying anything more complex than a static website, you need to ensure
  your app's dependencies are present on the server as well. That could mean shipping a
  self-contained binary (Go style), shipping your dependencies alongside your binary,
  installing dependencies globally on the server, etc.
- If your artifacts are the output of a build, you need to ensure they are up-to-date before
  deploying. Since vade is unaware of build steps, you might accidentally forget to rebuild and
  end up deploying an outdated version of your app.
- Containers are not a first-class citizen in vade, although it might be possible to deploy them
  using [podman quadlets](https://docs.podman.io/en/latest/markdown/podman-quadlet.1.html). If you
  manage to get them working, do let me know so we can add an example to the repo and point to it
  in this readme.

[pixi]: You might want to use `pixi global install <package>` for stuff that is not available through your package manager,
see the [pixi homepage](https://pixi.prefix.dev/latest/) for details.
