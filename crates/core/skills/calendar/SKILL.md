---
name: calendar
description: "Check schedule and events via icalBuddy (macOS)"
requires:
  bins: ["icalBuddy"]
---

# Calendar Skill

Use `icalBuddy` to query macOS Calendar (iCal) events and reminders.

## Setup

```bash
brew install ical-buddy
```

## Today's Events

```bash
icalBuddy eventsToday
icalBuddy -f eventsToday                          # formatted output
icalBuddy -nc eventsToday                          # no calendar name prefix
```

## Upcoming Events

```bash
icalBuddy eventsToday+3                            # next 3 days
icalBuddy eventsFrom:2026-03-14 to:2026-03-21      # date range
icalBuddy -n eventsToday+7                         # next 7 days, no relative dates
```

## Filtering

```bash
# Specific calendar only
icalBuddy -ic "Work" eventsToday+7

# Exclude calendars
icalBuddy -ec "Birthdays,Holidays" eventsToday

# Search event titles
icalBuddy -ea eventsToday+30 | grep -i "standup"
```

## Custom Formatting

```bash
# Compact one-line format
icalBuddy -nrd -nc -b "" -ps "/ | /" eventsToday

# Show only time and title
icalBuddy -npn -nc -iep "datetime,title" eventsToday

# Property separator customization
icalBuddy -ps "| — |" eventsToday
```

## Reminders

```bash
icalBuddy undoneReminders
icalBuddy -ic "Tasks" undoneReminders
```

## Notes

- Install via `brew install ical-buddy`
- Calendar names are case-sensitive in filters
- First run may prompt for calendar access permission
- Use `-b ""` to remove bullet point prefixes for cleaner parsing
- Dates use format `YYYY-MM-DD` in range queries
