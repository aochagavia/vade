# Timer

A dummy app that showcases a vade deployment capable of background processing.
The app runs `touch /tmp/my-timer-was-here` once per hour, as configured through custom systemd
units in `vade.toml`.

Deploy as follows:

```bash
vade deploy my-site
pyinfra -y --user <ssh-user> <ssh-host> vadegen/execute.py
```
