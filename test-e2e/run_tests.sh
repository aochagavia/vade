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

# Normal server setup
VADE_PUBLIC_KEY_PATH=$(realpath test-vm/id_ed25519.pub) ansible-playbook ../misc/setup-server.yml -i "$VM_IP_ADDR," -u root --private-key=./test-vm/id_ed25519
ansible-playbook ../misc/setup-caddy.yml -i "$VM_IP_ADDR," -u operator --private-key=./test-vm/id_ed25519

# Additional setup for testing (to use self-signed certs in Caddy)
ansible-playbook ./test-vm/patch-caddy-config.yml -i "$VM_IP_ADDR," -u operator --private-key=./test-vm/id_ed25519

# Deploy a static website
cargo run -- deploy static-app-name --config ../examples/static-site/vade.json --out-dir ../examples/static-site/vade-gen
ansible-playbook ../examples/static-site/vade-gen/playbook.yml -i "$VM_IP_ADDR," -u operator --private-key=./test-vm/id_ed25519

# Check that the deployment worked
curl -k --resolve static-site.example.com:443:"$VM_IP_ADDR" https://static-site.example.com/
