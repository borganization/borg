"""Notion tool — search, create pages, and query databases.

Requires NOTION_API_KEY (integration token).
"""
import json
import os
import sys
import urllib.request
import urllib.error


API_BASE = "https://api.notion.com/v1"
NOTION_VERSION = "2022-06-28"


def get_headers():
    token = os.environ.get("NOTION_API_KEY", "")
    if not token:
        return None
    return {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
        "Notion-Version": NOTION_VERSION,
    }


def search(query):
    headers = get_headers()
    if not headers:
        return {"error": "NOTION_API_KEY not set"}

    payload = json.dumps({"query": query, "page_size": 10}).encode()
    req = urllib.request.Request(f"{API_BASE}/search", data=payload, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            results = [
                {
                    "id": r["id"],
                    "type": r["object"],
                    "title": _extract_title(r),
                }
                for r in result.get("results", [])
            ]
            return {"ok": True, "count": len(results), "results": results}
    except urllib.error.HTTPError as e:
        return {"error": f"Notion API error {e.code}: {e.read().decode()}"}


def create_page(title, content="", parent_id=""):
    headers = get_headers()
    if not headers:
        return {"error": "NOTION_API_KEY not set"}

    page = {
        "properties": {"title": [{"text": {"content": title}}]},
        "children": [],
    }

    if parent_id:
        page["parent"] = {"page_id": parent_id}
    else:
        return {"error": "parent_id required for create_page"}

    if content:
        page["children"].append({
            "object": "block",
            "type": "paragraph",
            "paragraph": {"rich_text": [{"text": {"content": content}}]},
        })

    payload = json.dumps(page).encode()
    req = urllib.request.Request(f"{API_BASE}/pages", data=payload, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            return {"ok": True, "id": result.get("id", ""), "url": result.get("url", "")}
    except urllib.error.HTTPError as e:
        return {"error": f"Notion API error {e.code}: {e.read().decode()}"}


def read_page(page_id):
    headers = get_headers()
    if not headers:
        return {"error": "NOTION_API_KEY not set"}

    req = urllib.request.Request(f"{API_BASE}/pages/{page_id}", headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            return {"ok": True, "id": page_id, "title": _extract_title(result), "url": result.get("url", "")}
    except urllib.error.HTTPError as e:
        return {"error": f"Notion API error {e.code}: {e.read().decode()}"}


def query_database(database_id):
    headers = get_headers()
    if not headers:
        return {"error": "NOTION_API_KEY not set"}

    payload = json.dumps({"page_size": 20}).encode()
    req = urllib.request.Request(f"{API_BASE}/databases/{database_id}/query", data=payload, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            result = json.loads(resp.read())
            results = [{"id": r["id"], "title": _extract_title(r)} for r in result.get("results", [])]
            return {"ok": True, "count": len(results), "results": results}
    except urllib.error.HTTPError as e:
        return {"error": f"Notion API error {e.code}: {e.read().decode()}"}


def _extract_title(obj):
    props = obj.get("properties", {})
    for prop in props.values():
        if prop.get("type") == "title":
            titles = prop.get("title", [])
            if titles:
                return titles[0].get("text", {}).get("content", "")
    return "(untitled)"


def main():
    data = json.load(sys.stdin)
    action = data.get("action", "")

    if action == "search":
        result = search(data.get("query", ""))
    elif action == "create_page":
        result = create_page(data.get("title", ""), data.get("content", ""), data.get("parent_id", ""))
    elif action == "read_page":
        result = read_page(data.get("page_id", ""))
    elif action == "query_db":
        result = query_database(data.get("parent_id", ""))
    elif action == "test":
        headers = get_headers()
        if headers:
            result = {"ok": True, "service": "Notion"}
        else:
            result = {"error": "NOTION_API_KEY not set"}
    else:
        result = {"error": f"Unknown action: {action}. Use: search, create_page, read_page, query_db, test"}

    print(json.dumps(result))


if __name__ == "__main__":
    main()
