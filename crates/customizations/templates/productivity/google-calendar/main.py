"""Google Calendar tool — list, create, and delete events.

Requires GOOGLE_CALENDAR_TOKEN (OAuth bearer token).
"""
import json
import os
import sys
import urllib.request
import urllib.parse
import urllib.error
from datetime import datetime, timedelta, timezone


API_BASE = "https://www.googleapis.com/calendar/v3/calendars/primary"


def get_headers():
    token = os.environ.get("GOOGLE_CALENDAR_TOKEN", "")
    if not token:
        return None
    return {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}


def list_events(days=7):
    headers = get_headers()
    if not headers:
        return {"error": "GOOGLE_CALENDAR_TOKEN not set"}

    now = datetime.now(timezone.utc)
    time_min = now.isoformat()
    time_max = (now + timedelta(days=days)).isoformat()

    params = urllib.parse.urlencode({
        "timeMin": time_min,
        "timeMax": time_max,
        "singleEvents": "true",
        "orderBy": "startTime",
        "maxResults": 50,
    })

    req = urllib.request.Request(f"{API_BASE}/events?{params}", headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            events = [
                {
                    "id": e["id"],
                    "summary": e.get("summary", "(no title)"),
                    "start": e.get("start", {}).get("dateTime", e.get("start", {}).get("date", "")),
                    "end": e.get("end", {}).get("dateTime", e.get("end", {}).get("date", "")),
                }
                for e in result.get("items", [])
            ]
            return {"ok": True, "count": len(events), "events": events}
    except urllib.error.HTTPError as e:
        return {"error": f"Calendar API error {e.code}: {e.read().decode()}"}


def create_event(summary, start, end, description=""):
    headers = get_headers()
    if not headers:
        return {"error": "GOOGLE_CALENDAR_TOKEN not set"}

    event = {
        "summary": summary,
        "start": {"dateTime": start},
        "end": {"dateTime": end},
    }
    if description:
        event["description"] = description

    payload = json.dumps(event).encode()
    req = urllib.request.Request(f"{API_BASE}/events", data=payload, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            return {"ok": True, "id": result.get("id", ""), "link": result.get("htmlLink", "")}
    except urllib.error.HTTPError as e:
        return {"error": f"Calendar API error {e.code}: {e.read().decode()}"}


def delete_event(event_id):
    headers = get_headers()
    if not headers:
        return {"error": "GOOGLE_CALENDAR_TOKEN not set"}

    req = urllib.request.Request(f"{API_BASE}/events/{event_id}", headers=headers, method="DELETE")
    try:
        with urllib.request.urlopen(req):
            return {"ok": True, "deleted": event_id}
    except urllib.error.HTTPError as e:
        return {"error": f"Calendar API error {e.code}: {e.read().decode()}"}


def main():
    data = json.load(sys.stdin)
    action = data.get("action", "")

    if action == "list":
        result = list_events(data.get("days", 7))
    elif action == "create":
        result = create_event(
            data.get("summary", ""),
            data.get("start", ""),
            data.get("end", ""),
            data.get("description", ""),
        )
    elif action == "delete":
        result = delete_event(data.get("event_id", ""))
    elif action == "test":
        headers = get_headers()
        if headers:
            result = {"ok": True, "service": "Google Calendar"}
        else:
            result = {"error": "GOOGLE_CALENDAR_TOKEN not set"}
    else:
        result = {"error": f"Unknown action: {action}. Use: list, create, delete, test"}

    print(json.dumps(result))


if __name__ == "__main__":
    main()
