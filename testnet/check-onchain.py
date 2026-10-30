#!/usr/bin/env python3
"""Check that the escrow contract on NEAR testnet is the file listed in this repository.

Reads the SHA-256 of escrow.wasm from testnet/wasm/SHA256SUMS.txt, writes it in base58
(the form NEAR uses for code hashes) and compares it with the code hash that the public
testnet RPC reports for the escrow account. Standard library only; one read-only request.

Usage:  python3 testnet/check-onchain.py [account_id] [rpc_url]
Exit code 0 = hashes equal, 1 = different, 2 = RPC or file problem.
"""
import json
import pathlib
import sys
import urllib.request

ACCOUNT = sys.argv[1] if len(sys.argv) > 1 else "ag2610-escrow.testnet"
RPC = sys.argv[2] if len(sys.argv) > 2 else "https://test.rpc.fastnear.com"
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def base58(raw: bytes) -> str:
    n = int.from_bytes(raw, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = ALPHABET[r] + out
    return "1" * (len(raw) - len(raw.lstrip(b"\0"))) + out


def main() -> int:
    sums = pathlib.Path(__file__).resolve().parent / "wasm" / "SHA256SUMS.txt"
    try:
        hex_hash = next(
            line.split()[0] for line in sums.read_text().splitlines() if line.endswith("escrow.wasm")
        )
        expected = base58(bytes.fromhex(hex_hash))
    except (OSError, StopIteration, ValueError) as err:
        print("cannot read the checksum file:", err)
        return 2
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "query",
            "params": {"request_type": "view_account", "finality": "final", "account_id": ACCOUNT},
        }
    ).encode()
    request = urllib.request.Request(RPC, body, {"content-type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            chain = json.load(response)["result"]["code_hash"]
    except Exception as err:  # network, JSON or missing account
        print("RPC request failed:", err)
        return 2
    print("account         ", ACCOUNT)
    print("sha256 (hex)    ", hex_hash)
    print("expected (b58)  ", expected)
    print("on chain (b58)  ", chain)
    if chain == expected:
        print("OK: the contract on chain is the file listed in SHA256SUMS.txt")
        return 0
    print("DIFFERENT")
    return 1


if __name__ == "__main__":
    sys.exit(main())
