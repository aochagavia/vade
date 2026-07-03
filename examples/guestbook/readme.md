# Guestbook

A guestbook app that showcases a vade deployment with secrets.

#### Deploying

First, create the app:

```bash
vade create my-site
pyinfra -y --user <ssh-user> <ssh-host> vadegen/execute.py
```

Then, change the contents of `/opt/vade/apps/<app-name>/secrets` to something like:

```
AUTH_USERNAME=john.doe
AUTH_PASSWORD=1234
```

Finally, deploy:

```bash
# Compile the binary and copy it to our artifacts dir
cargo build --release
cp target/release/guestbook artifacts/guestbook

# Deploy
vade deploy my-site --set 'caddyfile.vars.domains=["<your-domain>"]'
pyinfra -y --user <ssh-user> <ssh-host> vadegen/execute.py
```
