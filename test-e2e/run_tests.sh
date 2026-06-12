#!/bin/bash
set -euo pipefail

VM_NAME=vade-test-vm

# Remove any VM left over from a previous run
sudo incus delete --force "$VM_NAME" 2>/dev/null || true

# Launch a VM with our cloud-init config
sudo incus launch images:debian/trixie/cloud "$VM_NAME" --vm \
  --config=cloud-init.user-data="$(cat test-vm/cloud-init.yaml)"

# Wait for the incus agent to come up, then for cloud-init to finish provisioning
echo "Waiting for the VM agent..."
until sudo incus exec "$VM_NAME" -- true 2>/dev/null; do sleep 1; done
echo "Waiting for cloud-init to finish..."
sudo incus exec "$VM_NAME" -- cloud-init status --wait || true

# Grab the VM's IPv4 address on incusbr0
VM_IP_ADDR=$(sudo incus list "$VM_NAME" --format json \
  | jq -r '[.[0].state.network | to_entries[]
            | select(.key != "lo") | .value.addresses[]
            | select(.family == "inet") | .address] | first')

if [[ -z "$VM_IP_ADDR" ]]; then
  echo "Error: could not determine VM IP address for '$VM_NAME'" >&2
  exit 1
fi

# The VM is ephemeral and its IP may be reused across runs, so we ignore
# host keys entirely
PYINFRA_SSH="-y --key ./test-vm/id_ed25519 --data ssh_strict_host_key_checking=off --data ssh_known_hosts_file=/dev/null"

# Normal server setup
VADE_PUBLIC_KEY_PATH=$(realpath test-vm/id_ed25519.pub) pyinfra --user root $PYINFRA_SSH "$VM_IP_ADDR" ../misc/setup-server.py
pyinfra --user operator $PYINFRA_SSH "$VM_IP_ADDR" ../misc/setup-caddy.py

# Additional setup for testing (to use self-signed certs in Caddy)
pyinfra --user operator $PYINFRA_SSH "$VM_IP_ADDR" ./test-vm/patch-caddy-config.py

# Deploy a static website
cargo run -- deploy static-app-name --config ../examples/static-site/vade.json --out-dir ../examples/static-site/vade-gen
pyinfra --user operator $PYINFRA_SSH "$VM_IP_ADDR" ../examples/static-site/vade-gen/deploy.py

# Check that the deployment worked
RESPONSE=$(curl -fsSk --resolve static-site.example.com:443:"$VM_IP_ADDR" https://static-site.example.com/)
EXPECTED='<h1>Hello World</h1>'
if echo "$RESPONSE" | grep -qF "$EXPECTED"; then
  echo "✅ Deployment check passed."
else
  echo "❌ Deployment check FAILED: expected to find '$EXPECTED' in the response body, but it was not present."
  echo "--- Actual response body ---"
  echo "$RESPONSE"
  echo "----------------------------"
  exit 1
fi
