"""Outlook tool — send, search, and read emails via Microsoft Graph API.

Requires MS_GRAPH_TOKEN (OAuth bearer token for Microsoft Graph).
"""
import json
import os
import sys
import urllib.request
import urllib.parse
import urllib.error


API_BASE = "https://graph.microsoft.com/v1.0/me"


def get_headers():
    token = os.environ.get("MS_GRAPH_TOKEN", "")
    if not token:
        return None
    return {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}


def send_email(to, subject, body):
    headers = get_headers()
    if not headers:
        return {"error": "MS_GRAPH_TOKEN not set"}

    payload = json.dumps({
        "message": {
            "subject": subject,
            "body": {"contentType": "Text", "content": body},
            "toRecipients": [{"emailAddress": {"address": to}}],
        }
    }).encode()

    req = urllib.request.Request(f"{API_BASE}/sendMail", data=payload, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req) as resp:
            return {"ok": True}
    except urllib.error.HTTPError as e:
        return {"error": f"Graph API error {e.code}: {e.read().decode()}"}


def search_emails(query, limit=10):
    headers = get_headers()
    if not headers:
        return {"error": "MS_GRAPH_TOKEN not set"}

    params = urllib.parse.urlencode({"$search": f'"{query}"', "$top": limit})
    req = urllib.request.Request(f"{API_BASE}/messages?{params}", headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            messages = [
                {"id": m["id"], "subject": m.get("subject", ""), "from": m.get("from", {}).get("emailAddress", {}).get("address", "")}
                for m in result.get("value", [])
            ]
            return {"ok": True, "count": len(messages), "messages": messages}
    except urllib.error.HTTPError as e:
        return {"error": f"Graph API error {e.code}: {e.read().decode()}"}


def read_email(message_id):
    headers = get_headers()
    if not headers:
        return {"error": "MS_GRAPH_TOKEN not set"}

    req = urllib.request.Request(f"{API_BASE}/messages/{message_id}", headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            return {
                "ok": True,
                "id": message_id,
                "subject": result.get("subject", ""),
                "body": result.get("bodyPreview", ""),
                "from": result.get("from", {}).get("emailAddress", {}).get("address", ""),
            }
    except urllib.error.HTTPError as e:
        return {"error": f"Graph API error {e.code}: {e.read().decode()}"}


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
            result = {"ok": True, "service": "Outlook"}
        else:
            result = {"error": "MS_GRAPH_TOKEN not set"}
    else:
        result = {"error": f"Unknown action: {action}. Use: send, search, read, test"}

    print(json.dumps(result))


if __name__ == "__main__":
    main()
