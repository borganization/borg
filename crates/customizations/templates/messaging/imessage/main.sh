#!/usr/bin/env bash
# iMessage tool — send and read messages via AppleScript (macOS only)
set -euo pipefail

INPUT=$(cat)
ACTION=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('action',''))")

case "$ACTION" in
  send)
    TO=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('to',''))")
    MSG=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('message',''))")
    if [ -z "$TO" ] || [ -z "$MSG" ]; then
      echo '{"error": "Missing required fields: to, message"}'
      exit 1
    fi
    osascript - "$TO" "$MSG" <<'APPLESCRIPT' 2>&1 && echo "{\"ok\": true}" || echo '{"error": "Failed to send iMessage"}'
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
    ;;
  read)
    LIMIT=$(echo "$INPUT" | python3 -c "import sys,json; v=json.load(sys.stdin).get('limit',10); print(int(v)) if str(v).isdigit() else print(10)")
    osascript - "$LIMIT" <<'APPLESCRIPT' 2>&1 || echo '{"error": "Failed to read messages"}'
on run argv
  set msgLimit to item 1 of argv as integer
  set output to ""
  tell application "Messages"
    repeat with aChat in (chats)
      set msgs to messages of aChat
      set msgCount to count of msgs
      set startIdx to msgCount - msgLimit + 1
      if startIdx < 1 then set startIdx to 1
      repeat with i from startIdx to msgCount
        set msg to item i of msgs
        set output to output & (sender of msg) & ": " & (text of msg) & linefeed
      end repeat
    end repeat
  end tell
  return output
end run
APPLESCRIPT
    ;;
  test)
    echo '{"ok": true, "platform": "macOS", "service": "iMessage"}'
    ;;
  *)
    echo '{"error": "Unknown action. Use: send, read, or test"}'
    exit 1
    ;;
esac
