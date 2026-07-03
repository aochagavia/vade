# Static site

A dummy static site, to showcase deployment with vade.

Deploy as follows:

```bash
vade deploy my-site --set 'caddyfile.vars.domains=["<your-domain>"]'
pyinfra -y --user <ssh-user> <ssh-host> vadegen/execute.py
```
