"""Gmail tool — send, search, and read emails via Gmail API.

Requires GMAIL_API_KEY (API key or OAuth token).
For full OAuth flow, use Google's application default credentials.
"""
import json
import os
import sys
import urllib.request
import urllib.parse
import urllib.error
import base64
import email.mime.text


API_BASE = "https://gmail.googleapis.com/gmail/v1/users/me"


def get_headers():
    token = os.environ.get("GMAIL_API_KEY", "")
    if not token:
        return None
    return {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}


def send_email(to, subject, body):
    headers = get_headers()
    if not headers:
        return {"error": "GMAIL_API_KEY not set"}

    msg = email.mime.text.MIMEText(body)
    msg["to"] = to
    msg["subject"] = subject
    raw = base64.urlsafe_b64encode(msg.as_bytes()).decode()

    payload = json.dumps({"raw": raw}).encode()
    req = urllib.request.Request(f"{API_BASE}/messages/send", data=payload, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            return {"ok": True, "id": result.get("id", "")}
    except urllib.error.HTTPError as e:
        return {"error": f"Gmail API error {e.code}: {e.read().decode()}"}


def search_emails(query, limit=10):
    headers = get_headers()
    if not headers:
        return {"error": "GMAIL_API_KEY not set"}

    params = urllib.parse.urlencode({"q": query, "maxResults": limit})
    req = urllib.request.Request(f"{API_BASE}/messages?{params}", headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            messages = result.get("messages", [])
            return {"ok": True, "count": len(messages), "messages": messages}
    except urllib.error.HTTPError as e:
        return {"error": f"Gmail API error {e.code}: {e.read().decode()}"}


def read_email(message_id):
    headers = get_headers()
    if not headers:
        return {"error": "GMAIL_API_KEY not set"}

    req = urllib.request.Request(f"{API_BASE}/messages/{message_id}?format=full", headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            snippet = result.get("snippet", "")
            subject = ""
            for header in result.get("payload", {}).get("headers", []):
                if header["name"].lower() == "subject":
                    subject = header["value"]
                    break
            return {"ok": True, "id": message_id, "subject": subject, "snippet": snippet}
    except urllib.error.HTTPError as e:
        return {"error": f"Gmail API error {e.code}: {e.read().decode()}"}


def main():
    data = json.load(sys.stdin)
    action = data.get("action", "")

    if action == "send":
        result = send_email(data.get("to", ""), data.get("subject", ""), data.get("body", ""))
    elif action == "search":
        result = search_emails(data.get("query", ""), data.get("limit", 10))
    elif action == "read":
        result = read_email(data.get("message_id", ""))
    elif action == "test":
        headers = get_headers()
        if headers:
            result = {"ok": True, "service": "Gmail"}
        else:
            result = {"error": "GMAIL_API_KEY not set"}
    else:
        result = {"error": f"Unknown action: {action}. Use: send, search, read, test"}

    print(json.dumps(result))


if __name__ == "__main__":
    main()
