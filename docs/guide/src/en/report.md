# Reporting a problem

PoE2 Oracle has its own window for writing to the developer: about a problem, an idea, an item it
read or priced wrong, or a crash. It sends your report to oracle.pushka.biz, the project's own
service, which passes it on to the developer. You need no account, on GitHub or anywhere else.

## Opening the report window

- The tray icon's menu → **Report a problem or idea…**
- The [settings](settings.md#help), section **Help**: **Write to the developer** in the row
  **Report a problem or idea**.
- On the price panel, the link **report a problem** under the item's name, when an item was read
  or priced wrong. Its tooltip says: "Tell the developer what's wrong with this item; its text is
  attached".
- The button **Report a problem** under an item the app couldn't read (see
  [An item is not recognised](troubleshooting.md#an-item-is-not-recognised)).
- After a crash, the next start opens it by itself: see [After a crash](#after-a-crash).

The two item entry points open the window with the item attached. The window stays on top of the
game like the settings window, can be resized, and follows the
[Interface scale](settings.md#interface-scale). Opening it hides the price panel; opening it again
brings the open window to the front.

## Writing the report

A switch in the window picks what the report is about:

| Kind | When | The box asks |
|---|---|---|
| **Problem** | Something doesn't work as it should. | "What happened, and how can it be repeated?" |
| **Idea** | Something you'd like PoE2 Oracle to do. | "What would you like the app to do?" |
| **Item** | Shown when the window was opened from an item. | "What's wrong with this item's price or reading?" |
| **Crash** | Shown only after a crash. | "What were you doing when it closed? (optional)" |

Write in the box, in English or Russian; the counter under it counts up to 8000 characters.
**Send** stays unavailable while the box is empty, except for a crash, where writing is optional,
and while the text is over 8000 characters or the contact over 200.

**Contact (optional)**: "Telegram, Discord or email — if you'd like an answer". Up to 200
characters. Without it the report still arrives, but the developer has no way to answer you.

## What's attached

- **Attach diagnostics** adds the [diagnostics report](troubleshooting.md#collecting-a-diagnostics-report):
  "Logs, settings, unread item texts and a system summary. Your Windows name and folders are
  hidden." It is on for **Problem**, **Item** and **Crash** and off for **Idea**; picking another
  kind sets it that way again until you switch it yourself. **What's inside** saves the same zip
  to your desktop and shows it in File Explorer, so you can read it before you send it.
- With **Item** picked, the item goes along: the line "Item: *name* — its text is attached" names
  it. With **Crash** picked, what PoE2 Oracle reported when it closed goes along (see
  [After a crash](#after-a-crash)). Pick another kind and neither is sent.

The window also says what leaves your computer: "Sent to oracle.pushka.biz: your text,
the contact if given, the app's version and languages, and what's attached. No account needed."
With the version and the languages go your Windows version, the league and the interface scale.
Your pathofexile.com session never goes along. See [Privacy](privacy.md#reports) for what the
service does with a report.

## Sending

Click **Send**; while the report goes, the button says **Sending…**. Then the window says one of:

- "Sent — thank you! Report #*number*": the developer has it. Mention the number if you write about
  the same thing again. Sometimes the window says only "Sent — thank you!": the report arrived all
  the same.
- "Couldn't send: *reason*". The reason is one of these:

  | Reason | What to do |
  |---|---|
  | "no connection to oracle.pushka.biz" | Check your internet connection, and whether a firewall, VPN or proxy blocks PoE2 Oracle. |
  | "the service is unavailable, try later" | The service couldn't pass the report on right now. Try again later. |
  | "too many reports from your address, try again in *N* min" | Reports from one address are limited. Wait that long. |
  | "the attachment is too large" | What's attached is more than the service takes. Switch **Attach diagnostics** off and send again. |
  | "the service refused the report" | The service and this version of the app disagree on what a report may hold. Update PoE2 Oracle and try again. |

  **Try again** sends the same report again. **Save to desktop** writes all of it into one zip on
  your desktop — your text and contact with the app's details, the item's or the crash's text, and
  the diagnostics report when attached — and shows it in File Explorer, so nothing you wrote is
  lost. **Close** closes the window.

**Cancel**, the **×** or <kbd>Esc</kbd> close the window without sending; <kbd>Esc</kbd> first
leaves the text box, a second press closes.

## After a crash

If PoE2 Oracle stops on an error of its own, it writes the error down before it closes. The next
start opens the report window by itself, with **Crash** picked and the line "PoE2 Oracle closed
unexpectedly on *date*; what it reported is attached". Attached: PoE2 Oracle's version, the time,
the error it stopped on and where in its code that happened, with your Windows user name and
folders hidden, and the diagnostics report, on by default.

Writing what you were doing helps, but is optional. Send the report or close the window: either
way it doesn't come back for that crash. If you quit PoE2 Oracle from the tray while the window is
open, it opens again at the next start. A crash more than a week old is forgotten without a word.

## Without the app

When PoE2 Oracle doesn't start at all, or you'd rather write from a browser, use the form on the
site: [oracle.pushka.biz/report.html](../../report.html). It sends a problem or an idea with your
text and, if you like, a contact. It can't attach an item or the diagnostics report, so for a
problem the app's window is better.
