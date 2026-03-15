#!/usr/bin/env bash
# Send an iMessage reply via AppleScript with outbound sanitization and echo cache update.
set -euo pipefail

INPUT=$(cat)
CHANNEL_DIR="$(cd "$(dirname "$0")" && pwd)"
ECHO_CACHE="$CHANNEL_DIR/echo_cache.json"

# Extract fields and sanitize outbound text
RESULT=$(echo "$INPUT" | ECHO_CACHE="$ECHO_CACHE" python3 -c "
import json, sys, re, hashlib, time, os, tempfile

data = json.load(sys.stdin)
text = data.get('text', '')
sender_id = data.get('sender_id', '')

# Outbound sanitization: strip internal tags
text = re.sub(r'<thinking>.*?</thinking>', '', text, flags=re.DOTALL)
text = re.sub(r'<internal>.*?</internal>', '', text, flags=re.DOTALL)
text = re.sub(r'<relevant_memories>.*?</relevant_memories>', '', text, flags=re.DOTALL)
text = re.sub(r'\n{3,}', '\n\n', text)
text = text.strip()

# Update echo cache with outbound text hash
echo_cache_path = os.environ.get('ECHO_CACHE', '')
if echo_cache_path and text:
    try:
        with open(echo_cache_path, 'r') as f:
            cache = json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        cache = {'entries': []}

    h = hashlib.sha256(text.strip().lower().encode('utf-8')).hexdigest()[:16]
    cache.setdefault('entries', []).append({'hash': h, 'ts': time.time()})

    now = time.time()
    cache['entries'] = [e for e in cache['entries'] if now - e.get('ts', 0) < 10]

    dir_name = os.path.dirname(echo_cache_path)
    fd, tmp = tempfile.mkstemp(dir=dir_name, suffix='.tmp')
    with os.fdopen(fd, 'w') as f:
        json.dump(cache, f)
    os.rename(tmp, echo_cache_path)

# Output sender_id and text as JSON for safe shell handling
print(json.dumps({'sender_id': sender_id, 'text': text}))
")

SENDER_ID=$(echo "$RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin)['sender_id'])")
TEXT=$(echo "$RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin)['text'])")

if [ -z "$TEXT" ]; then
    echo '{"ok": true, "skipped": "empty text after sanitization"}'
    exit 0
fi

# Send via AppleScript
osascript - "$SENDER_ID" "$TEXT" <<'APPLESCRIPT' 2>&1
on run argv
    set recipientId to item 1 of argv
    set messageText to item 2 of argv
    tell application "Messages"
        set targetService to 1st account whose service type = iMessage
        set targetBuddy to participant recipientId of targetService
        send messageText to targetBuddy
    end tell
end run
APPLESCRIPT

if [ $? -eq 0 ]; then
    echo '{"ok": true}'
else
    echo '{"error": "Failed to send iMessage via AppleScript"}'
    exit 1
fi
