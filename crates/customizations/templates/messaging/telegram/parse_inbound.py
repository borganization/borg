"""Parse inbound Telegram webhook into normalized message format."""
import json
import sys


def parse():
    data = json.load(sys.stdin)
    body = data.get("body", {})

    message = body.get("message") or body.get("edited_message") or {}
    if not message:
        # Not a message update (could be callback_query, etc.)
        print(json.dumps({"skip": True}))
        return

    chat = message.get("chat", {})
    from_user = message.get("from", {})

    result = {
        "text": message.get("text", ""),
        "sender_id": str(from_user.get("id", "")),
        "sender_name": from_user.get("first_name", "Unknown"),
        "channel_id": str(chat.get("id", "")),
        "message_id": str(message.get("message_id", "")),
        "timestamp": message.get("date", 0),
    }
    print(json.dumps(result))


if __name__ == "__main__":
    parse()
