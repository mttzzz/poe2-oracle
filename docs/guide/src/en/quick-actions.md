# Quick actions

Quick actions are hotkeys that type into the game for you: a chat command or message, or a search
string for the stash or a vendor's window. Set them up in the [settings](settings.md), section
«Быстрые действия» (quick actions). You can have up to 12.

Out of the box there is one action, `/hideout`, without a hotkey: record a key for it to use it.

## Setting up an action

1. Click «+ Добавить действие» (add action). A new row appears with the cursor in its text box.
2. Pick its kind: «Чат» (chat) or «Тайник» (stash).
3. Type the text: a command or message for chat (the box suggests `/hideout, /exit, @last
   спасибо…`), or a search string for the stash (the box suggests «Строка поиска, например с
   poe2.re»: a search string, for example from poe2.re). The action is saved as soon as you press
   <kbd>Enter</kbd> or leave the box.
4. Click the key field, which reads «без клавиши» (no key), and press a combination: one of
   <kbd>F1</kbd>–<kbd>F12</kbd>, or <kbd>Ctrl</kbd> or <kbd>Alt</kbd> with a letter or a digit.
   The key works at once. <kbd>Backspace</kbd> leaves the action without a key; <kbd>Esc</kbd>
   keeps the old one.

Like everything in the settings, changes apply at once: there is no Save button. The **×** at the
right of a row removes the action. An action without text is not saved. A key cannot be the
price-check hotkey («Это сочетание уже у проверки цены») or another action's («Это сочетание уже у
другого быстрого действия»); otherwise the same rules apply as for the
[price-check hotkey](settings.md#hotkey).

## Chat actions

PoE2 Oracle presses <kbd>Enter</kbd> to open the chat, pastes the text and presses <kbd>Enter</kbd>
to send it. If the text does not start with a chat channel sign (`#`, `%`, `@`, `$`, `&`) or a
command's `/`, whatever the chat box already held is selected first, so the text replaces it.

`@last` stands for the last player who whispered you:

| Text | What happens |
|---|---|
| `@last thanks` | Answers that player with "thanks" |
| `/invite @last` | Sends the command with that player's name in place of `@last` |
| `@last` | Opens a whisper to that player and waits for you to type |

Only a separate `@last` counts: the whole text, at the start followed by a space, or at the end
after a space. `@lastochka hi` is sent as it is.

## Stash search actions

Open the stash or a vendor's window first. PoE2 Oracle presses <kbd>Ctrl</kbd>+<kbd>F</kbd>, which
puts the cursor in the window's search box, pastes the text and presses <kbd>Enter</kbd>; the game
highlights the matching items. The search string is used exactly as you typed it. Tools such as
poe2.re build such strings for you.

## Commands that are never sent

Some chat commands destroy something or change it for good. A mis-pressed key must not do that,
so quick actions never send these, in any letter case and also after a chat channel sign or
`@last` (`%/destroy`, `@last /destroy`):

| Command | What it does in the game |
|---|---|
| `/destroy` | Destroys the item on the cursor |
| `/clear_ignore_list` | Empties your ignore list |
| `/convertracereward` | Destroys the race reward unique on the cursor, turning it into an account-bound skin |
| `/ResetAtlas` | Resets your Atlas (the game allows it only when no map is left to run) |

If an action's text is one of them, a warning appears under it, for example «Команда /destroy
уничтожает предмет — быстрые действия её не отправляют» (/destroy destroys an item; quick actions
don't send it), and that text is not saved: the action keeps its last allowed text (a new action,
none) until you write another. Stash search actions are checked the same way. Any other text, such
as `@last thanks`, is sent as you wrote it.

## Good to know

- Quick action hotkeys work only while the game is the window in front. In every other program
  those keys keep their usual meaning.
- The text arrives through the clipboard, so text in any language arrives intact whatever your
  keyboard layout. Your clipboard is put back right after.
- Each press sends one message: holding the key down sends it once, and a press while an action
  is still typing is ignored.
- If another program already holds the key you press, the row says so, for example «F7 занято
  другой программой — осталось F5» (F7 is taken by another program; F5 stays), and the action
  keeps its previous key (a new one, none).
