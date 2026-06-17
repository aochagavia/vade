# e2e tests

Requirements (other than vade's own):

- `incus`: creates and tears down the test VM
- `curl`: makes requests to ensure the deployed apps work

## Incus setup

You can probably grab Incus from your package manager. After that, initialize it as follows:

```bash
sudo systemctl enable --now incus.socket incus.service
sudo incus admin init

# Assuming `incusbr0` is the bridge network, we need to allow traffic between it and the host
# See https://linuxcontainers.org/incus/docs/main/howto/network_bridge_firewalld/#ufw-add-rules-for-the-bridge for details
sudo ufw allow in on incusbr0
sudo ufw route allow in on incusbr0
sudo ufw route allow out on incusbr0
```

## Running the tests

```bash
./run_tests.sh                              # full run, fresh VM
./run_tests.sh --reuse-vm                   # full run, reused VM
./run_tests.sh --reuse-vm guestbook timer   # selection run, reused VM

# We currently do not shut down the VM after testing, so you need to do that manually
sudo incus delete --force vade-test-vm
```

## Upcoming tests

- Nuke app removes it, leaving no traces (no user, no files, no stray systemd units).
