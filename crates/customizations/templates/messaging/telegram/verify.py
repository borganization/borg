"""Verify Telegram webhook request.

Telegram supports a secret_token parameter when setting up webhooks via setWebhook.
When configured, Telegram sends the token in the X-Telegram-Bot-Api-Secret-Token header.
See: https://core.telegram.org/bots/api#setwebhook
"""
import hashlib
import hmac
import json
import sys


def verify():
    data = json.load(sys.stdin)
    headers = data.get("headers", {})
    body = data.get("body", {})
    secret = data.get("secret", "")

    # Check basic payload structure
    if "update_id" not in body:
        print(json.dumps({"valid": False, "reason": "Missing update_id — not a valid Telegram update"}))
        return

    # If a webhook secret is configured, verify the header
    if secret:
        header_token = headers.get(
            "x-telegram-bot-api-secret-token",
            headers.get("X-Telegram-Bot-Api-Secret-Token", ""),
        )
        if not header_token:
            print(json.dumps({"valid": False, "reason": "Missing X-Telegram-Bot-Api-Secret-Token header"}))
            return
        if not hmac.compare_digest(secret, header_token):
            print(json.dumps({"valid": False, "reason": "Secret token mismatch"}))
            return

    print(json.dumps({"valid": True}))


if __name__ == "__main__":
    verify()
