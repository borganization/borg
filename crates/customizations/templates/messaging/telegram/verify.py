"""Verify Telegram webhook request (basic check — Telegram doesn't sign webhooks
by default, but we verify the payload structure)."""
import json
import sys


def verify():
    data = json.load(sys.stdin)
    body = data.get("body", {})

    # Telegram updates always have an "update_id" field
    if "update_id" in body:
        print(json.dumps({"valid": True}))
    else:
        print(json.dumps({"valid": False, "reason": "Missing update_id"}))


if __name__ == "__main__":
    verify()
