"""Send outbound message to Telegram chat."""
import json
import sys
import urllib.request
import urllib.error


def send():
    data = json.load(sys.stdin)
    text = data.get("text", "")
    channel_id = data.get("channel_id", "")
    token = data.get("token", "")

    if not all([text, channel_id, token]):
        print(json.dumps({"error": "Missing required fields: text, channel_id, token"}))
        return

    url = f"https://api.telegram.org/bot{token}/sendMessage"
    payload = json.dumps({"chat_id": channel_id, "text": text, "parse_mode": "Markdown"}).encode()

    req = urllib.request.Request(url, data=payload, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            print(json.dumps({"ok": result.get("ok", False)}))
    except urllib.error.HTTPError as e:
        body = e.read().decode()
        print(json.dumps({"error": f"Telegram API error {e.code}: {body}"}))


if __name__ == "__main__":
    send()
