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
    osascript -e "tell application \"Messages\"
      set targetService to 1st account whose service type = iMessage
      set targetBuddy to participant \"$TO\" of targetService
      send \"$MSG\" to targetBuddy
    end tell" 2>&1 && echo "{\"ok\": true, \"to\": \"$TO\"}" || echo "{\"error\": \"Failed to send iMessage\"}"
    ;;
  read)
    LIMIT=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('limit', 10))")
    osascript -e "
      set output to \"\"
      tell application \"Messages\"
        repeat with aChat in (chats)
          set msgs to messages of aChat
          set msgCount to count of msgs
          set startIdx to msgCount - $LIMIT + 1
          if startIdx < 1 then set startIdx to 1
          repeat with i from startIdx to msgCount
            set msg to item i of msgs
            set output to output & (sender of msg) & \": \" & (text of msg) & \"\n\"
          end repeat
        end repeat
      end tell
      return output
    " 2>&1 || echo '{"error": "Failed to read messages"}'
    ;;
  test)
    echo '{"ok": true, "platform": "macOS", "service": "iMessage"}'
    ;;
  *)
    echo '{"error": "Unknown action. Use: send, read, or test"}'
    exit 1
    ;;
esac
