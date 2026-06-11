# e2e tests

Requirements (other than vade's own):

- `incus`: creates and tears down the test VM (see )
- `ansible`: provisions the VM (needs the `ansible.posix` and `community.general` collections)
- `curl`: makes requests to ensure the deployed apps work

## Incus setup

You can probably grab Incus from your package manager. After that, initialize it as follows:

```bash
sudo systemctl enable --now incus.socket incus.service
sudo incus admin init

# Assuming `incusbr0` is the bridge network, we need to allow traffic between it and the host
sudo ufw allow in on incusbr0          # DHCP/DNS to the host + VM -> host replies
sudo ufw route allow in on incusbr0    # VM -> internet
sudo ufw route allow out on incusbr0
```

## Running the tests

`./run_tests.sh`

## Upcoming tests

- Deployment scenario 1:
  - DONE: Deploy static app from examples, send a GET request and check that the response is what we expected
  - Scaffold static app + run works
  - Scaffold dynamic app + run works
  - Scaffold dynamic app on top of static app replaces it
  - Nuke?
- Deployment scenario 2:
  - Scaffold secrets for app and populate them
  - Scaffold dynamic app that reads secrets and check that they are returned
  - SSH into the machine and do something to ensure the next deploy fails. Run a deploy and ensure it rolls back cleanly.
