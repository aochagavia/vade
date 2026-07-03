#!/bin/bash
set -euo pipefail

VM_NAME=vade-test-vm

# The tests we know how to run, in the order a full run executes them. Each
# entry `foo-bar` maps to a `test_foo_bar` function defined below.
ALL_TESTS=(static-site static-site-unchanged python-no-deps python-no-deps-overwrite guestbook guestbook-rollback goatcounter timer existing-user existing-systemd-unit invalid-systemd-unit invalid-caddyfile)

usage() {
  cat <<EOF
Usage: $0 [--reuse-vm] [test...]

By default we create a fresh VM, provision it from scratch, and run every
test.

Options:
  Python demo overwrite-vm   Skip VM creation and server setup, reusing the existing VM
  -h, --help   Show this help

Available tests:
  ${ALL_TESTS[*]}
EOF
}

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

###
# Argument parsing
###

REUSE_VM=false
SELECTED_TESTS=()
for arg in "$@"; do
  case "$arg" in
    --reuse-vm) REUSE_VM=true ;;
    -h|--help) usage; exit 0 ;;
    -*) echo "Unknown option: $arg" >&2; usage >&2; exit 1 ;;
    *)
      if [[ " ${ALL_TESTS[*]} " != *" $arg "* ]]; then
        echo "Unknown test: $arg" >&2
        usage >&2
        exit 1
      fi
      SELECTED_TESTS+=("$arg")
      ;;
  esac
done

# No test names given means run them all, in their canonical order.
if [[ ${#SELECTED_TESTS[@]} -eq 0 ]]; then
  SELECTED_TESTS=("${ALL_TESTS[@]}")
fi

###
# VM + server setup
###

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
# Tests
###

test_static-site() {
  # Deploy
  cargo run -- deploy my-static-site --config ../examples/static-site/vade.toml --out-dir ../examples/static-site/vadegen
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/static-site/vadegen/execute.py

  # Check
  sleep 0.2
  local response
  response=$(curl -fsSk --resolve static-site.example.com:443:"$VM_IP_ADDR" https://static-site.example.com/)
  assert_response_contains "Static site check" "<h1>Hello World</h1>" "$response"
}

test_static-site-unchanged() {
  test_static-site
  test_static-site

  # There should be hardlinks, meaning that the second deploy reused the artifacts from the first
  # one (instead of transfering them again over the network)
  links=$(sudo incus exec vade-test-vm -- stat -c %h /opt/vade/apps/my-static-site/active-deployment/artifacts/index.html)
  if [ "$links" -lt 2 ]; then
    echo "index.html has no hardlinks (nlink=$links)" >&2
    exit 1
  fi
}

test_python-no-deps() {
  # Deploy
  cargo run -- deploy my-python-no-deps --config ./resources/python-no-deps/vade.toml --out-dir ./resources/python-no-deps/vadegen
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ./resources/python-no-deps/vadegen/execute.py

  # Check
  sleep 0.2
  local response
  response=$(curl -fsSk --resolve python-site.example.com:443:"$VM_IP_ADDR" https://python-site.example.com/)
  assert_response_contains "Python demo site check" "Hello world" "$response"
}

test_python-no-deps-overwrite() {
  # Deploy the static site to the same app used by python-no-deps
  # Note: we use `--set` to have the Caddyfile target `python-site.example.com`
  cargo run -- deploy my-python-no-deps --config ../examples/static-site/vade.toml --out-dir ../examples/static-site/vadegen --set 'caddyfile.vars.domains=["python-site.example.com"]'
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/static-site/vadegen/execute.py

  # Check
  local response
  response=$(curl -fsSk --resolve python-site.example.com:443:"$VM_IP_ADDR" https://python-site.example.com/)
  assert_response_contains "Python demo overwrite check" "<h1>Hello World</h1>" "$response"
}

test_guestbook() {
  # Create
  cargo run -- create my-guestbook --out-dir ../examples/guestbook/vadegen
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/guestbook/vadegen/execute.py

  # Set secrets
  sudo incus exec vade-test-vm -- sh -c 'printf "AUTH_USERNAME=foo\nAUTH_PASSWORD=123\n" > /opt/vade/apps/my-guestbook/secrets'

  # Deploy, after compiling
  just -f ../examples/guestbook/justfile compile
  cargo run -- deploy my-guestbook --config ../examples/guestbook/vade.toml --out-dir ../examples/guestbook/vadegen
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/guestbook/vadegen/execute.py

  # Check GET
  sleep 0.2
  local response
  response=$(curl -fsSk -u foo:123 --resolve guestbook.example.com:443:"$VM_IP_ADDR" https://guestbook.example.com/)
  assert_response_contains "Guestbook GET check" '<h2>Sign the Guestbook:</h2>' "$response"

  # Check POST + message is actually persisted
  local sign_message
  sign_message="Hello from the e2e test ($(date +%s)-$RANDOM)"
  curl -fsSk -u foo:123 --resolve guestbook.example.com:443:"$VM_IP_ADDR" \
    --data-urlencode "name=E2E Tester" \
    --data-urlencode "message=$sign_message" \
    https://guestbook.example.com/sign > /dev/null

  response=$(curl -fsSk -u foo:123 --resolve guestbook.example.com:443:"$VM_IP_ADDR" https://guestbook.example.com/)
  assert_response_contains "Guestbook POST check" "$sign_message" "$response"
}

test_guestbook-rollback() {
  cargo run -- deploy my-guestbook --config resources/invalid-systemd-unit-vade.toml
  echo "Expecting the following deployment to fail..."
  if pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" "vadegen/execute.py"; then
    echo "❌ Existing systemd unit check FAILED: deployment succeeded but should have been rejected."
    exit 1
  fi

  # Check GET, which should still work because the new deployment rolled back
  sleep 0.2
  local response
  response=$(curl -fsSk -u foo:123 --resolve guestbook.example.com:443:"$VM_IP_ADDR" https://guestbook.example.com/)
  assert_response_contains "Guestbook GET check" '<h2>Sign the Guestbook:</h2>' "$response"
}

test_goatcounter() {
  # Deploy, after downloading
  cargo run -- deploy my-goatcounter --config ../examples/goatcounter/vade.toml --out-dir ../examples/goatcounter/vadegen
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/goatcounter/vadegen/execute.py

  # Check
  sleep 0.2
  local response
  response=$(curl -fsSk --resolve goats.example.com:443:"$VM_IP_ADDR" https://goats.example.com/)
  assert_response_contains "Goatcounter check" "<h1>Create your first site and user</h1>" "$response"
}

test_timer() {
  # Deploy
  cargo run -- deploy my-timer --config ../examples/timer/vade.toml --out-dir ../examples/timer/vadegen
  pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" ../examples/timer/vadegen/execute.py

  # Check
  sleep 0.2
  sudo incus exec vade-test-vm -- ls /tmp/my-timer-was-here
}

test_existing-user() {
  # Attempt to deploy to `operator`, which would assume a vade-managed user called `operator`
  cargo run -- deploy operator --config ../examples/timer/vade.toml --out-dir ../examples/timer/vadegen

  # The deployment must be rejected
  echo "Expecting the following deployment to fail..."
  if pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" "../examples/timer/vadegen/execute.py"; then
    echo "❌ Existing systemd unit check FAILED: deployment succeeded but should have been rejected."
    exit 1
  fi
  echo "✅ Existing systemd unit check passed."
}

test_existing-systemd-unit() {
  # Create an unmanaged unit file that collides with the one the app would install
  sudo incus file push resources/dummy.service vade-test-vm/etc/systemd/system/my-amazing-app.service

  # Attempt to deploy to `my-amazing-app`, which would try to install a `my-amazing-app.service` unit
  cargo run -- deploy my-amazing-app --config ../examples/timer/vade.toml --out-dir ../examples/timer/vadegen

  # The deployment must be rejected because `my-amazing-app.service` already exists and is
  # not managed by vade
  echo "Expecting the following deployment to fail..."
  if pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" "../examples/timer/vadegen/execute.py"; then
    echo "❌ Existing systemd unit check FAILED: deployment succeeded but should have been rejected."
    exit 1
  fi
  echo "✅ Existing systemd unit check passed."
}

test_invalid-systemd-unit() {
  cargo run -- deploy my-invalid-app --config resources/invalid-systemd-unit-vade.toml
  echo "Expecting the following deployment to fail..."
  if pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" "vadegen/execute.py"; then
    echo "❌ Existing systemd unit check FAILED: deployment succeeded but should have been rejected."
    exit 1
  fi
}

test_invalid-caddyfile() {
  cargo run -- deploy my-invalid-app --config resources/invalid-caddyfile-vade.toml
  echo "Expecting the following deployment to fail..."
  if pyinfra --user operator "${PYINFRA_SSH[@]}" "$VM_IP_ADDR" "vadegen/execute.py"; then
    echo "❌ Existing systemd unit check FAILED: deployment succeeded but should have been rejected."
    exit 1
  fi
}

###
# Run the selected tests
###

for name in "${SELECTED_TESTS[@]}"; do
  echo
  echo "### Running test: $name ###"
  "test_$name"
done

echo
echo "✅ All selected tests passed."
