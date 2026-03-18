"""Parse inbound WhatsApp message from Twilio webhook."""
import json
import sys
import urllib.parse


def parse():
    data = json.load(sys.stdin)
    body = data.get("body", "")

    # Twilio sends form-urlencoded body
    if isinstance(body, str):
        params = dict(urllib.parse.parse_qsl(body))
    elif isinstance(body, dict):
        params = body
    else:
        print(json.dumps({"skip": True}))
        return

    msg_body = params.get("Body", "")
    from_number = params.get("From", "").replace("whatsapp:", "")
    to_number = params.get("To", "").replace("whatsapp:", "")
    msg_sid = params.get("MessageSid", "")

    if not msg_body:
        print(json.dumps({"skip": True}))
        return

    result = {
        "text": msg_body,
        "sender_id": from_number,
        "sender_name": from_number,
        "channel_id": to_number,
        "message_id": msg_sid,
    }
    print(json.dumps(result))


if __name__ == "__main__":
    parse()
