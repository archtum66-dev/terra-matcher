#!/usr/bin/env python3
"""Check the recorded Terra mainnet settlements against the public NEAR chain.

Read-only. Standard library only. No keys, no funds, no account needed.

    python3 scripts/check-mainnet-tx.py                      # every docs/live/*terra-1b_result*.json
    python3 scripts/check-mainnet-tx.py docs/live/2026-11-07_terra-1b_result.json

For each record it asks a public NEAR RPC node for the transaction and checks that
  1. the transaction succeeded and went to the verifier `intents.near`,
  2. it was signed by the account the record names,
  3. it holds two `token_diff` events, one per account, and both intents were executed,
  4. each side gave exactly the recorded amount, and what each side received is what it
     gave minus the verifier fee (ceil(amount * fee_pips / 1e6)),
  5. nothing was created or lost: per token the two diffs plus the collected fees are zero,
  6. the events on chain are the events in the record.
Exit code 0 only if every check passes.
"""
import glob
import json
import os
import sys
import urllib.request

RPCS = ["https://rpc.mainnet.near.org", "https://free.rpc.fastnear.com"]
BASE = "nep141:wrap.near"
UNIT = {BASE: (10**24, "NEAR"), "usdc": (10**6, "USDC")}


def rpc_tx(tx_hash, sender):
    body = json.dumps({
        "jsonrpc": "2.0", "id": "check", "method": "tx",
        "params": {"tx_hash": tx_hash, "sender_account_id": sender, "wait_until": "FINAL"},
    }).encode()
    last = None
    for url in RPCS:
        try:
            req = urllib.request.Request(
                url, body, {"content-type": "application/json", "user-agent": "terra-check/1"})
            with urllib.request.urlopen(req, timeout=30) as r:
                answer = json.load(r)
            if "result" in answer:
                return answer["result"], url
            last = answer.get("error")
        except Exception as e:  # noqa: BLE001 - report and try the next node
            last = str(e)
    raise SystemExit(f"ERROR: no RPC node answered: {last}")


def events(result):
    out = []
    for r in result["receipts_outcome"]:
        for line in r["outcome"].get("logs", []):
            if line.startswith("EVENT_JSON:"):
                out.append(json.loads(line[len("EVENT_JSON:"):]))
    return out


def fmt(token, units):
    scale, name = UNIT[BASE] if token == BASE else UNIT["usdc"]
    sign = "-" if units < 0 else ""
    units = abs(units)
    return f"{sign}{units // scale}.{units % scale * 10**6 // scale:06d} {name}"


def check_record(path):
    rec = json.load(open(path, encoding="utf-8"))
    tx_hash = rec["tx_hash"]
    seller, buyer = rec["seller"], rec["buyer"]            # seller gives NEAR, buyer gives USDC
    signer = rec.get("signer", seller)
    give_base, give_quote = int(rec["base_yocto"]), int(rec["quote_units"])
    fee_pips = int(rec["fee_pips"])
    print(f"\nrecord: {os.path.relpath(path)}")
    print(f"tx:     {tx_hash}")
    print(f"        https://nearblocks.io/txns/{tx_hash}")
    result, node = rpc_tx(tx_hash, signer)
    print(f"node:   {node}")
    failed = []

    def check(ok, text):
        print(("PASS  " if ok else "FAIL  ") + text)
        if not ok:
            failed.append(text)

    tx = result["transaction"]
    all_ok = "SuccessValue" in result["status"] and not any(
        "Failure" in r["outcome"]["status"] for r in result["receipts_outcome"])
    check(all_ok, "transaction and all its receipts succeeded (final)")
    check(tx["receiver_id"] == "intents.near", "receiver is the verifier intents.near")
    check(tx["signer_id"] == signer, f"signed by {signer[:8]}...")

    ev = events(result)
    diffs = [e for e in ev if e["event"] == "token_diff"]
    done = [e for e in ev if e["event"] == "intents_executed"]
    by_account = {d["account_id"]: d for e in diffs for d in e["data"]}
    check(set(by_account) == {seller, buyer} and len(diffs) == 2,
          "two token_diff events, one for the seller account and one for the buyer account")
    check(sorted(d["account_id"] for e in done for d in e["data"]) == sorted([seller, buyer]),
          "intents_executed for both accounts")

    quote_token = next((t for d in by_account.values() for t in d["diff"] if t != BASE), None)
    if len(by_account) == 2 and quote_token:
        s, b = by_account[seller]["diff"], by_account[buyer]["diff"]
        fee = lambda amount: -(-amount * fee_pips // 10**6)            # ceil
        check(int(s[BASE]) == -give_base and int(b[quote_token]) == -give_quote,
              f"seller gave {fmt(BASE, give_base)}, buyer gave {fmt(quote_token, give_quote)} (as recorded)")
        check(int(b[BASE]) == give_base - fee(give_base) and int(s[quote_token]) == give_quote - fee(give_quote),
              f"each side received what it was owed minus the verifier fee ({fee_pips} pip): "
              f"buyer +{fmt(BASE, int(b[BASE]))}, seller +{fmt(quote_token, int(s[quote_token]))}")
        fees = {}
        for d in by_account.values():
            for t, v in d.get("fees_collected", {}).items():
                fees[t] = fees.get(t, 0) + int(v)
        check(all(int(s.get(t, 0)) + int(b.get(t, 0)) + fees.get(t, 0) == 0 for t in (BASE, quote_token)),
              "nothing created or lost: per token the two diffs plus the fees add up to zero")
    chain = [e for e in ev if e["event"] in ("token_diff", "intents_executed")]
    recorded = [json.loads(l[len("EVENT_JSON:"):]) for l in rec.get("events", [])]
    check(chain == recorded, "the token_diff and intents_executed events on chain are the events in the record")
    return failed


def main():
    root = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    paths = sys.argv[1:] or sorted(glob.glob(os.path.join(root, "docs", "live", "*terra-1b_result*.json")))
    if not paths:
        raise SystemExit("ERROR: no record found in docs/live/")
    print("Terra mainnet settlement check (read-only, public RPC, no keys)")
    failed = []
    for p in paths:
        failed += check_record(p)
    print("\nALL CHECKS PASSED" if not failed else f"\n{len(failed)} CHECK(S) FAILED")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
