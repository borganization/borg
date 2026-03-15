---
name: http
description: "HTTP requests via curl with JSON parsing using jq. Use when: calling REST APIs, downloading data, testing endpoints, or inspecting HTTP responses. Requires curl installed; jq recommended for JSON parsing."
requires:
  bins: ["curl"]
---

# HTTP Skill

Use `curl` to make HTTP requests and `jq` to parse JSON responses.

## GET Requests

```bash
curl -s https://api.example.com/items
curl -s https://api.example.com/items | jq '.'
curl -s -o /dev/null -w "%{http_code}" https://example.com   # status code only
```

## POST Requests

```bash
curl -s -X POST https://api.example.com/items \
  -H "Content-Type: application/json" \
  -d '{"name": "item", "value": 42}'
```

## Authentication

```bash
# Bearer token
curl -s -H "Authorization: Bearer $TOKEN" https://api.example.com/me

# Basic auth
curl -s -u user:password https://api.example.com/data
```

## Headers & Debugging

```bash
curl -s -I https://example.com                  # response headers only
curl -s -v https://example.com 2>&1 | head -30  # verbose with request/response headers
```

## JSON Parsing with jq

```bash
curl -s https://api.example.com/items | jq '.[0].name'
curl -s https://api.example.com/items | jq '.[] | {id, name}'
curl -s https://api.example.com/items | jq 'length'
curl -s https://api.example.com/items | jq '.[] | select(.status == "active")'
```

## File Downloads

```bash
curl -s -L -o output.zip https://example.com/file.zip
curl -s -L -O https://example.com/file.tar.gz   # keep original filename
```

## Notes

- Always use `-s` (silent) to suppress progress bars
- Use `-L` to follow redirects
- Use `-f` to fail on HTTP errors (non-2xx)
- Pipe through `jq '.'` for pretty-printed JSON
- If `jq` is not available, use `python3 -m json.tool` as a fallback
