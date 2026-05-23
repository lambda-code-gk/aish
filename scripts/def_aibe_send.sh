export AIBE_SOCKET="${AIBE_SOCKET_PATH:-$HOME/.local/share/aibe/run.sock}"

aibe_send() {
  python3 - "$1" "$AIBE_SOCKET" <<'PY'
import socket, sys
payload = sys.argv[1].strip() + "\n"
path = sys.argv[2]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(path)
s.sendall(payload.encode())
buf = b""
while b"\n" not in buf:
    chunk = s.recv(65536)
    if not chunk:
        break
    buf += chunk
sys.stdout.write(buf.decode())
s.close()
PY
}
