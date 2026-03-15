#!/usr/bin/env python3
"""Poll macOS Messages database for new incoming iMessages.

Reads state from state.json (last_rowid), queries ~/Library/Messages/chat.db
for new inbound messages, applies policy filtering, echo detection, and
reflection guards, then outputs a JSON array of normalized messages.
"""

import hashlib
import json
import os
import sqlite3
import sys
import tempfile
import time

CHANNEL_DIR = os.path.dirname(os.path.abspath(__file__))
STATE_FILE = os.path.join(CHANNEL_DIR, "state.json")
POLICY_FILE = os.path.join(CHANNEL_DIR, "policy.json")
ECHO_CACHE_FILE = os.path.join(CHANNEL_DIR, "echo_cache.json")
MESSAGES_DB = os.path.expanduser("~/Library/Messages/chat.db")

# Reflection guard: drop messages containing these markers
REFLECTION_MARKERS = ["<thinking>", "<internal>", "<relevant_memories>", "###", "assistant:"]

ECHO_TTL_SECONDS = 10
MAX_MESSAGES_PER_POLL = 50


def load_json(path, default):
    try:
        with open(path, "r") as f:
            return json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        return default


def save_json_atomic(path, data):
    """Write JSON atomically via temp file + rename."""
    dir_name = os.path.dirname(path)
    fd, tmp_path = tempfile.mkstemp(dir=dir_name, suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as f:
            json.dump(data, f)
        os.rename(tmp_path, path)
    except Exception:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass
        raise


def text_hash(text):
    return hashlib.sha256(text.strip().lower().encode("utf-8")).hexdigest()[:16]


def load_echo_cache():
    cache = load_json(ECHO_CACHE_FILE, {"entries": []})
    now = time.time()
    # Prune expired entries
    cache["entries"] = [
        e for e in cache.get("entries", [])
        if now - e.get("ts", 0) < ECHO_TTL_SECONDS
    ]
    return cache


def is_echo(cache, msg_text):
    h = text_hash(msg_text)
    return any(e.get("hash") == h for e in cache.get("entries", []))


def passes_reflection_guard(text):
    lower = text.lower()
    return not any(marker.lower() in lower for marker in REFLECTION_MARKERS)


def apply_policy(sender, text, policy):
    dm_policy = policy.get("dm_policy", "open")
    blocked = policy.get("blocked", [])
    allowlist = policy.get("allowlist", [])
    max_len = policy.get("max_message_length", 10000)

    if sender in blocked:
        return False

    if dm_policy == "disabled":
        return False

    if dm_policy == "allowlist" and sender not in allowlist:
        return False

    if len(text) > max_len:
        return False

    return True


def main():
    state = load_json(STATE_FILE, {"last_rowid": 0})
    last_rowid = state.get("last_rowid", 0)

    policy = load_json(POLICY_FILE, {"dm_policy": "open"})
    echo_cache = load_echo_cache()

    # Open Messages database read-only
    try:
        db_uri = f"file:{MESSAGES_DB}?mode=ro"
        conn = sqlite3.connect(db_uri, uri=True)
    except sqlite3.OperationalError as e:
        print(json.dumps([]), flush=True)
        print(f"Cannot open Messages DB: {e}", file=sys.stderr)
        return

    try:
        cursor = conn.execute(
            """
            SELECT m.ROWID, m.text, m.date, h.id as sender, m.cache_roomnames
            FROM message m
            JOIN handle h ON m.handle_id = h.ROWID
            WHERE m.ROWID > ? AND m.is_from_me = 0 AND m.text IS NOT NULL
            ORDER BY m.ROWID ASC
            LIMIT ?
            """,
            (last_rowid, MAX_MESSAGES_PER_POLL),
        )
        rows = cursor.fetchall()
    except sqlite3.OperationalError as e:
        print(json.dumps([]), flush=True)
        print(f"Query error: {e}", file=sys.stderr)
        conn.close()
        return

    conn.close()

    messages = []
    new_max_rowid = last_rowid

    for rowid, text, date, sender, room in rows:
        new_max_rowid = max(new_max_rowid, rowid)

        if not text or not text.strip():
            continue

        # Policy check
        if not apply_policy(sender, text, policy):
            continue

        # Echo detection
        if is_echo(echo_cache, text):
            continue

        # Reflection guard
        if not passes_reflection_guard(text):
            continue

        messages.append({
            "sender_id": sender,
            "text": text.strip(),
            "channel_id": room or sender,
        })

    # Update state with new high-water mark
    if new_max_rowid > last_rowid:
        save_json_atomic(STATE_FILE, {"last_rowid": new_max_rowid})

    # Save pruned echo cache
    save_json_atomic(ECHO_CACHE_FILE, echo_cache)

    print(json.dumps(messages), flush=True)


if __name__ == "__main__":
    main()
