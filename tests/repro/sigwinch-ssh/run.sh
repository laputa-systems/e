#!/usr/bin/env bash
set -eo pipefail

script_dir=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$script_dir/../../.." && pwd)
cd "$repo_root"

if ! git check-ignore -q .work; then
    printf '%s\n' '.work must be ignored through .git/info/exclude before running this harness.' >&2
    exit 1
fi

for dependency in cargo ssh ssh-keygen tmux python3; do
    if ! command -v "$dependency" >/dev/null 2>&1; then
        printf 'missing required command: %s\n' "$dependency" >&2
        exit 1
    fi
done

sshd_bin=$(command -v sshd || true)
if [ -z "$sshd_bin" ] && [ -x /usr/sbin/sshd ]; then
    sshd_bin=/usr/sbin/sshd
fi
if [ -z "$sshd_bin" ]; then
    printf '%s\n' 'missing required command: sshd' >&2
    exit 1
fi

tmux_bin=$(command -v tmux)
ssh_bin=$(command -v ssh)
ssh_keygen_bin=$(command -v ssh-keygen)
ssh_user=$(id -un)
work_tmp="$repo_root/.work/tmp"
mkdir -p "$work_tmp"
export TMPDIR="$work_tmp"
tmp_dir=$(mktemp -d "$work_tmp/sigwinch-ssh.XXXXXX")
test_home="$tmp_dir/home"
tmux_config="$test_home/.config/tmux/tmux.conf"
outer_socket="$tmp_dir/outer.sock"
remote_socket="$tmp_dir/remote.sock"
sshd_pid=
keep_tmp="$SIGWINCH_KEEP_TMP"
cleanup_error=0

stop_tmux() {
    local socket=$1
    if "$tmux_bin" -S "$socket" display-message -p '#S' >/dev/null 2>&1; then
        if ! "$tmux_bin" -S "$socket" kill-server >/dev/null 2>&1; then
            printf 'could not stop tmux server at %s\n' "$socket" >&2
            cleanup_error=1
        fi
    fi
}

cleanup() {
    local status=$?
    trap - EXIT INT TERM
    set +e
    stop_tmux "$remote_socket"
    stop_tmux "$outer_socket"
    if [ -n "$sshd_pid" ] && kill -0 "$sshd_pid" 2>/dev/null; then
        if ! kill -TERM "$sshd_pid"; then
            printf 'could not stop sshd process %s\n' "$sshd_pid" >&2
            cleanup_error=1
        fi
        wait "$sshd_pid" 2>/dev/null
    fi
    if [ "$keep_tmp" = 1 ]; then
        printf 'kept harness files at %s\n' "$tmp_dir" >&2
    elif ! find "$tmp_dir" -depth -delete; then
        printf 'could not remove harness files at %s\n' "$tmp_dir" >&2
        cleanup_error=1
    fi
    if [ "$status" -eq 0 ] && [ "$cleanup_error" -ne 0 ]; then
        status=1
    fi
    exit "$status"
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [ -n "$E_TEST_BINARY" ]; then
    binary=$E_TEST_BINARY
    if [[ "$binary" != /* ]]; then
        binary="$repo_root/$binary"
    fi
else
    TMPDIR="$work_tmp" cargo build --locked --bin e
    binary="$repo_root/target/debug/e"
fi
if [ ! -x "$binary" ]; then
    printf 'editor binary is not executable: %s\n' "$binary" >&2
    exit 1
fi
editor_name=$(basename "$binary")

mkdir -m 700 -p "$test_home/.config/tmux"
cp "$script_dir/tmux.conf" "$tmux_config"
"$ssh_keygen_bin" -q -t ed25519 -N '' -C sigwinch-ssh-repro -f "$tmp_dir/host_ed25519"
"$ssh_keygen_bin" -q -t ed25519 -N '' -C sigwinch-ssh-repro -f "$tmp_dir/client_ed25519"
cp "$tmp_dir/client_ed25519.pub" "$tmp_dir/authorized_keys"
chmod 600 "$tmp_dir/authorized_keys"

port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
cat > "$tmp_dir/sshd_config" <<EOF
Port $port
AddressFamily inet
ListenAddress 127.0.0.1
HostKey "$tmp_dir/host_ed25519"
PidFile "$tmp_dir/sshd.pid"
AuthorizedKeysFile "$tmp_dir/authorized_keys"
StrictModes no
PubkeyAuthentication yes
AuthenticationMethods publickey
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin no
UsePAM no
AllowUsers $ssh_user
PermitTTY yes
PermitUserEnvironment no
AllowAgentForwarding no
AllowTcpForwarding no
X11Forwarding no
PermitTunnel no
LogLevel DEBUG1
EOF
"$sshd_bin" -t -f "$tmp_dir/sshd_config"
"$sshd_bin" -D -e -f "$tmp_dir/sshd_config" >"$tmp_dir/sshd.log" 2>&1 &
sshd_pid=$!

shell_quote() {
    python3 -c 'import shlex, sys; print(shlex.quote(sys.argv[1]))' "$1"
}

editor_command="env HOME=$(shell_quote "$test_home") $(shell_quote "$binary")"
remote_tmux_command="env HOME=$(shell_quote "$test_home") $(shell_quote "$tmux_bin") -S $(shell_quote "$remote_socket") -f $(shell_quote "$tmux_config") new-session -A -s nested -x 120 -y 40 -c $(shell_quote "$repo_root")"
ssh_command="$(shell_quote "$ssh_bin") -tt -F /dev/null -p $port -i $(shell_quote "$tmp_dir/client_ed25519") -o IdentityAgent=none -o IdentitiesOnly=yes -o BatchMode=yes -o ControlMaster=no -o PreferredAuthentications=publickey -o PasswordAuthentication=no -o KbdInteractiveAuthentication=no -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=$(shell_quote "$tmp_dir/known_hosts") -o GlobalKnownHostsFile=/dev/null -o ClearAllForwardings=yes $(shell_quote "$ssh_user@127.0.0.1")"

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    if [ -S "$outer_socket" ]; then
        printf '%s\n' 'outer tmux pane:' >&2
        "$tmux_bin" -S "$outer_socket" display-message -pt outer:1.0 'command=#{pane_current_command} dead=#{pane_dead} size=#{pane_width}x#{pane_height}' >&2 || true
        "$tmux_bin" -S "$outer_socket" capture-pane -p -t outer:1.0 -S -30 >&2 || true
    fi
    if [ -S "$remote_socket" ]; then
        printf '%s\n' 'nested tmux pane:' >&2
        "$tmux_bin" -S "$remote_socket" list-panes -a -F 'index=#{pane_index} command=#{pane_current_command} dead=#{pane_dead} status=#{pane_dead_status} size=#{pane_width}x#{pane_height}' >&2 || true
        "$tmux_bin" -S "$remote_socket" capture-pane -p -t nested:1.0 -S -30 >&2 || true
    fi
    if [ -f "$tmp_dir/sshd.log" ]; then
        printf '%s\n' 'sshd log:' >&2
        tail -n 40 "$tmp_dir/sshd.log" >&2
    fi
    exit 1
}

outer_state() {
    "$tmux_bin" -S "$outer_socket" display-message -pt outer:1.0 '#{pane_current_command} #{pane_dead}' 2>/dev/null || true
}

editor_state() {
    "$tmux_bin" -S "$remote_socket" display-message -pt nested:1.0 '#{pane_current_command} #{pane_dead}' 2>/dev/null || true
}

wait_for_outer_shell() {
    local attempt=0
    local state
    while [ "$attempt" -lt 200 ]; do
        state=$(outer_state)
        if [[ "$state" == *' 0' ]]; then
            return 0
        fi
        attempt=$((attempt + 1))
    done
    return 1
}

wait_for_outer_ssh() {
    local attempt=0
    local state
    while [ "$attempt" -lt 200 ]; do
        state=$(outer_state)
        if [ "$state" = 'ssh 0' ]; then
            return 0
        fi
        if [ "$state" = 'ssh 1' ]; then
            return 1
        fi
        attempt=$((attempt + 1))
    done
    return 1
}

wait_for_remote_shell() {
    local attempt=0
    local state
    while [ "$attempt" -lt 200 ]; do
        state=$(editor_state)
        if [[ "$state" == *' 0' ]]; then
            return 0
        fi
        attempt=$((attempt + 1))
    done
    return 1
}

assert_editor_alive() {
    local state
    state=$(editor_state)
    if [ "$state" != "$editor_name 0" ]; then
        fail "expected $editor_name to remain alive in the nested pane, got '$state'"
    fi
    state=$(outer_state)
    if [ "$state" != 'ssh 0' ]; then
        fail "expected the SSH client to remain alive in the outer pane, got '$state'"
    fi
}

wait_for_editor() {
    local attempt=0
    local state
    while [ "$attempt" -lt 200 ]; do
        state=$(editor_state)
        if [ "$state" = "$editor_name 0" ]; then
            return 0
        fi
        state=$(outer_state)
        if [ "$state" = 'ssh 1' ]; then
            return 1
        fi
        attempt=$((attempt + 1))
    done
    return 1
}

outer_size() {
    "$tmux_bin" -S "$outer_socket" display-message -pt outer:1.0 '#{pane_width}x#{pane_height}'
}

remote_client_size() {
    "$tmux_bin" -S "$remote_socket" display-message -pt nested '#{client_width}x#{client_height}' 2>/dev/null || true
}

wait_for_remote_client_size() {
    local expected=$1
    local attempt=0
    local actual
    while [ "$attempt" -lt 200 ]; do
        assert_editor_alive
        actual=$(remote_client_size)
        if [ "$actual" = "$expected" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
    done
    return 1
}

remote_pane_width() {
    "$tmux_bin" -S "$remote_socket" display-message -pt nested:1.0 '#{pane_width}' 2>/dev/null || true
}

wait_for_remote_pane_width() {
    local expected=$1
    local attempt=0
    local actual
    while [ "$attempt" -lt 200 ]; do
        assert_editor_alive
        actual=$(remote_pane_width)
        if [ "$actual" = "$expected" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
    done
    return 1
}

"$tmux_bin" -S "$outer_socket" -f "$tmux_config" new-session -d -s outer -x 120 -y 40 -c "$repo_root"
if ! wait_for_outer_shell; then
    fail 'the local tmux shell did not start'
fi
"$tmux_bin" -S "$outer_socket" send-keys -t outer:1.0 "$ssh_command" Enter
if ! wait_for_outer_ssh; then
    fail 'the SSH client did not start in the outer tmux pane'
fi
"$tmux_bin" -S "$outer_socket" send-keys -t outer:1.0 "$remote_tmux_command" Enter
if ! wait_for_remote_shell; then
    fail 'the nested tmux shell did not start over SSH'
fi
"$tmux_bin" -S "$remote_socket" send-keys -t nested:1.0 "$editor_command" Enter

if ! wait_for_editor; then
    fail 'the editor did not start in the nested tmux pane'
fi
assert_editor_alive

"$tmux_bin" -S "$outer_socket" split-window -h -t outer:1.0 -c "$repo_root"
for width in 24 92 40 110 15 72 60; do
    "$tmux_bin" -S "$outer_socket" resize-pane -t outer:1.0 -x "$width"
done
expected_size=$(outer_size)
if ! wait_for_remote_client_size "$expected_size"; then
    fail "remote tmux client did not receive outer pane size $expected_size"
fi
assert_editor_alive

for height in 16 39 20 40; do
    "$tmux_bin" -S "$outer_socket" resize-window -t outer:1 -y "$height"
done
expected_size=$(outer_size)
if ! wait_for_remote_client_size "$expected_size"; then
    fail "remote tmux client did not receive outer window size $expected_size"
fi
assert_editor_alive

"$tmux_bin" -S "$remote_socket" split-window -h -t nested:1.0
for width in 15 50 22 45 30; do
    "$tmux_bin" -S "$remote_socket" resize-pane -t nested:1.0 -x "$width"
done
if ! wait_for_remote_pane_width 30; then
    fail 'nested tmux did not apply the final pane width'
fi
assert_editor_alive

"$tmux_bin" -S "$remote_socket" send-keys -t nested:1.0 C-q
attempt=0
exit_state=
while [ "$attempt" -lt 200 ]; do
    exit_state=$(editor_state)
    if [ "$exit_state" != "$editor_name 0" ] && [ -n "$exit_state" ]; then
        break
    fi
    attempt=$((attempt + 1))
done
if [ "$exit_state" = "$editor_name 0" ] || [ -z "$exit_state" ]; then
    fail "editor did not exit cleanly after the resize run, got '$exit_state'"
fi

printf '%s\n' 'PASS: e survived outer tmux pane and window resizes over SSH, then nested tmux pane resizes.'
