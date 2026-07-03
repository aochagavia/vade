# GoatCounter

You can deploy GoatCounter as follows:

```bash
# Grab the executable for the latest GoatCounter release
mkdir -p artifacts
tag=$(curl -s https://api.github.com/repos/arp242/goatcounter/releases/latest | grep -o '"tag_name": "[^"]*' | cut -d'"' -f4)
curl -sL "https://github.com/arp242/goatcounter/releases/download/$tag/goatcounter-$tag-linux-amd64.gz" | gunzip > artifacts/goatcounter
chmod +x artifacts/goatcounter

# Deploy
vade deploy my-site --set 'caddyfile.vars.domains=["<your-domain>"]'
pyinfra -y --user <ssh-user> <ssh-host> vadegen/execute.py
```
