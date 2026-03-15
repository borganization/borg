"""Send outbound WhatsApp message via Twilio API."""
import json
import sys
import urllib.request
import urllib.parse
import base64


def send():
    data = json.load(sys.stdin)
    text = data.get("text", "")
    sender_id = data.get("sender_id", "")
    token = data.get("token", "")  # TWILIO_ACCOUNT_SID
    secret = data.get("secret", "")  # TWILIO_AUTH_TOKEN
    from_number = data.get("channel_id", "")

    if not all([text, sender_id, token]):
        print(json.dumps({"error": "Missing required fields"}))
        return

    url = f"https://api.twilio.com/2010-04-01/Accounts/{token}/Messages.json"
    payload = urllib.parse.urlencode({
        "To": f"whatsapp:{sender_id}",
        "From": f"whatsapp:{from_number}",
        "Body": text,
    }).encode()

    auth = base64.b64encode(f"{token}:{secret}".encode()).decode()
    req = urllib.request.Request(url, data=payload, headers={
        "Authorization": f"Basic {auth}",
        "Content-Type": "application/x-www-form-urlencoded",
    })

    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            print(json.dumps({"ok": True, "sid": result.get("sid", "")}))
    except urllib.error.HTTPError as e:
        body = e.read().decode()
        print(json.dumps({"error": f"Twilio API error {e.code}: {body}"}))


if __name__ == "__main__":
    send()
