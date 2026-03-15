"""Verify Twilio webhook signature using HMAC-SHA1."""
import hashlib
import hmac
import json
import sys
import urllib.parse


def verify():
    data = json.load(sys.stdin)
    headers = data.get("headers", {})
    body = data.get("body", "")
    secret = data.get("secret", "")
    url = data.get("webhook_url", "")

    signature = headers.get("x-twilio-signature", headers.get("X-Twilio-Signature", ""))

    if not secret:
        # If no secret configured, warn but pass through
        print(json.dumps({"valid": True, "reason": "no auth token configured — verification skipped"}))
        return

    if not signature:
        print(json.dumps({"valid": False, "reason": "Missing X-Twilio-Signature header"}))
        return

    # Twilio signature validation: HMAC-SHA1 of URL + sorted POST params
    # See: https://www.twilio.com/docs/usage/security#validating-requests
    check_url = url
    if isinstance(body, dict):
        # Sort POST parameters and append to URL
        sorted_params = sorted(body.items())
        for key, value in sorted_params:
            check_url += str(key) + str(value)

    computed = hmac.new(
        secret.encode("utf-8"),
        check_url.encode("utf-8"),
        hashlib.sha1,
    ).digest()

    import base64
    expected = base64.b64encode(computed).decode("utf-8")

    if hmac.compare_digest(expected, signature):
        print(json.dumps({"valid": True}))
    else:
        print(json.dumps({"valid": False, "reason": "Signature mismatch"}))


if __name__ == "__main__":
    verify()
