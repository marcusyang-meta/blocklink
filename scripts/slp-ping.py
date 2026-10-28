#!/usr/bin/env python3
"""Minimal Minecraft Server List Ping (SLP) client.

Usage: slp-ping.py <host> <port> [timeout-seconds]

Performs the handshake + status request over TCP, prints the server's
status JSON to stdout, and exits nonzero on any failure or timeout.
"""
import json
import socket
import sys


def encode_varint(value):
    data = bytearray()
    value &= 0xFFFFFFFF
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            data.append(byte | 0x80)
        else:
            data.append(byte)
            break
    return bytes(data)


def read_exact(sock, count):
    buf = bytearray()
    while len(buf) < count:
        chunk = sock.recv(count - len(buf))
        if not chunk:
            raise ConnectionError("connection closed by server")
        buf.extend(chunk)
    return bytes(buf)


def read_varint(sock):
    result = 0
    shift = 0
    while True:
        byte = read_exact(sock, 1)[0]
        result |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return result
        shift += 7
        if shift > 35:
            raise ValueError("varint too long")


def send_packet(sock, packet_id, payload=b""):
    body = encode_varint(packet_id) + payload
    sock.sendall(encode_varint(len(body)) + body)


def main():
    if len(sys.argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    host = sys.argv[1]
    port = int(sys.argv[2])
    timeout = float(sys.argv[3]) if len(sys.argv) > 3 else 10.0
    try:
        with socket.create_connection((host, port), timeout=timeout) as sock:
            sock.settimeout(timeout)
            # Handshake: protocol 767, server address, port, next state = status (1)
            payload = bytearray()
            payload.extend(encode_varint(767))
            encoded_host = host.encode("utf-8")
            payload.extend(encode_varint(len(encoded_host)))
            payload.extend(encoded_host)
            payload.extend(port.to_bytes(2, "big"))
            payload.extend(encode_varint(1))
            send_packet(sock, 0x00, bytes(payload))
            # Status request (empty payload)
            send_packet(sock, 0x00)
            # Status response: packet id 0x00 + JSON string
            _length = read_varint(sock)
            packet_id = read_varint(sock)
            if packet_id != 0x00:
                raise ValueError(f"unexpected packet id {packet_id}")
            raw = read_exact(sock, read_varint(sock))
            status = json.loads(raw.decode("utf-8"))
    except Exception as exc:  # noqa: BLE001 - CLI reports any failure uniformly
        print(f"slp-ping: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(status))
    return 0


if __name__ == "__main__":
    sys.exit(main())
