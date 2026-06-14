#!/bin/bash
set -euo pipefail

VM_NAME=vade-test-vm

# By default we create a fresh VM and provision it from scratch. Passing
# --reuse-vm skips VM creation and server setup, reusing the existing VM.
REUSE_VM=false
for arg in "$@"; do
  case "$arg" in
    --reuse-vm) REUSE_VM=true ;;
    *) echo "Unknown argument: $arg" >&2; exit 1 ;;
  esac
done

# Assert that a string is present in an HTTP response body, exiting the script otherwise.
# Usage: assert_response_contains <check_name> <expected> <response>
assert_response_contains() {
  local check_name=$1
  local expected=$2
  local response=$3
  if echo "$response" | grep -qF "$expected"; then
    echo "✅ $check_name passed."
  else
    echo "❌ $check_name FAILED: expected to find '$expected' in the response body, but it was not present."
    echo "--- Actual response body ---"
    echo "$response"
    echo "----------------------------"
    exit 1
  fi
}

if [[ "$REUSE_VM" == false ]]; then
  # Remove any VM left over from a previous run
  sudo incus delete --force "$VM_NAME" 2>/dev/null || true

  # Launch a VM with our cloud-init config
  sudo incus launch images:debian/trixie/cloud "$VM_NAME" --vm \
    --config=cloud-init.user-data="$(cat test-vm/cloud-init.yaml)"
fi

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
PYINFRA_SSH=(-y --key ./test-vm/id_ed25519 --data ssh_strict_host_key_checking=off --data ssh_known_hosts_file=/dev/null)

if [[ "$REUSE_VM" == false ]]; then
  # Normal server setup
  VADE_PUBLIC_KEY_PATH=$(realpath test-vm/id_ed25519.pub) pyinfra --user root "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../misc/setup-server.py
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../misc/setup-caddy.py

  # Additional setup for testing (to use self-signed certs in Caddy)
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ./test-vm/patch-caddy-config.py
fi

###
# Vade setup
###

cargo run -- server-setup --out-dir ./vadegen
pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ./vadegen/execute.py

###
# Static website
###

# Deploy
cargo run -- deploy my-static-site --config ../examples/static-site/vade.toml --out-dir ../examples/static-site/vade-gen
pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/static-site/vade-gen/execute.py

# Check
RESPONSE=$(curl -fsSk --resolve static-site.example.com:443:"$VM_IP_ADDR" https://static-site.example.com/)
assert_response_contains "Static site check" "<h1>Hello World</h1>" "$RESPONSE"

###
# Basic python demo app
###

# Deploy
cargo run -- deploy my-python-no-deps --config ../examples/python-no-deps/vade.toml --out-dir ../examples/python-no-deps/vade-gen
pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/python-no-deps/vade-gen/execute.py

# Check
RESPONSE=$(curl -fsSk --resolve python-site.example.com:443:"$VM_IP_ADDR" https://python-site.example.com/)
assert_response_contains "Python demo site check" "Hello world" "$RESPONSE"

###
# Guestbook
###

# Create
cargo run -- create my-guestbook --out-dir ../examples/guestbook/vade-gen
pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/guestbook/vade-gen/execute.py

# Set secrets
sudo incus exec vade-test-vm -- sh -c 'printf "AUTH_USERNAME=foo\nAUTH_PASSWORD=123\n" > /opt/vade/apps/my-guestbook/secrets'

# Deploy, after compiling
just -f ../examples/guestbook/justfile compile
cargo run -- deploy my-guestbook --config ../examples/guestbook/vade.toml --out-dir ../examples/guestbook/vade-gen
pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/guestbook/vade-gen/execute.py

# Check GET
RESPONSE=$(curl -fsSk -u foo:123 --resolve guestbook.example.com:443:"$VM_IP_ADDR" https://guestbook.example.com/)
assert_response_contains "Guestbook GET check" '<h2>Sign the Guestbook:</h2>' "$RESPONSE"

# Check POST + message is actually persisted
SIGN_MESSAGE="Hello from the e2e test ($(date +%s)-$RANDOM)"
curl -fsSk -u foo:123 --resolve guestbook.example.com:443:"$VM_IP_ADDR" \
  --data-urlencode "name=E2E Tester" \
  --data-urlencode "message=$SIGN_MESSAGE" \
  https://guestbook.example.com/sign > /dev/null

RESPONSE=$(curl -fsSk -u foo:123 --resolve guestbook.example.com:443:"$VM_IP_ADDR" https://guestbook.example.com/)
assert_response_contains "Guestbook POST check" "$SIGN_MESSAGE" "$RESPONSE"

###
# Goatcounter
###

# Deploy, after downloading
cargo run -- deploy my-goatcounter --config ../examples/goatcounter/vade.toml --out-dir ../examples/goatcounter/vade-gen
pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/goatcounter/vade-gen/execute.py

# Check
RESPONSE=$(curl -fsSk --resolve goats.example.com:443:"$VM_IP_ADDR" https://goats.example.com/)
assert_response_contains "Goatcounter check" "<h1>Create your first site and user</h1>" "$RESPONSE"
