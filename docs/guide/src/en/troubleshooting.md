# Troubleshooting and FAQ

## The hotkey does nothing

Go through these in order.

1. **Is PoE2 Oracle running?** Its icon must be in the notification area by the clock (maybe under
   the **^** arrow). If not, start it from the Start menu.
2. **Is the game the window in front?** The hotkey works only while the game or the price panel is
   active. Click into the game once and try again. If you changed the hotkey, hover the tray icon
   to see the current one.
3. **Does another program hold <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd>?** PoE2 Oracle checks
   prices by pressing the game's own copy shortcut. If a graphics card overlay, a screen recorder
   or Discord has taken that combination, the game never receives it. The settings window then
   shows at the top: «Сочетание Ctrl+Alt+C, которым игра копирует предмет, занято другой
   программой…», and after a failed check the panel says «Игра не копирует предмет: сочетание
   Ctrl+Alt+C перехватывает другая программа…». Open that program's hotkey settings and change or
   switch off <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd>.
4. **Does another program hold your hotkey?** If some program already uses
   <kbd>Ctrl</kbd>+<kbd>E</kbd>, PoE2 Oracle cannot take it. Pick another hotkey in the
   [settings](settings.md#hotkey).
5. **Does the game run as administrator?** Windows does not let a program without administrator
   rights send keys to one that runs as administrator, and PoE2 Oracle cannot detect this. Start
   the game normally, without "Run as administrator", or run PoE2 Oracle as administrator too.
6. **Is the game in exclusive Fullscreen?** Then the check may run, but Windows shows nothing over
   the game. Set the game's display mode to Windowed Fullscreen. The settings window warns about
   this: «Игра в режиме «Полноэкранный»: поверх него панель не видна…».

With no item under the cursor, pressing the hotkey does nothing at all; that is expected.

## The panel opens but shows no prices

| The panel says | What to do |
|---|---|
| «Загрузка каталога…» | The first start downloads the trade site's data. Wait a few seconds. |
| «Нет данных сайта торговли — нет интернета или сайт недоступен. Повторяю попытку сам.» | Check that pathofexile.com opens in your browser and that your firewall or antivirus lets PoE2 Oracle connect. The app tries again by itself, and again every time you press the hotkey. |
| «Ничего не найдено» | No listing matches. The buttons under the message search more broadly, one trade search each: «Широкий −10 %» (broad −10%) searches the same ticked rows with each **min** 10% below your roll, «Совпадение N из M» (N of M match) finds listings with all the ticked rows but one. You can also untick some filter rows, lower some **min** values, switch «База» to «Класс» or let other sellers in with the «Продавцы:» chip. See [When nothing matches exactly](price-check.md#when-nothing-matches-exactly). |

## "No connection" message

«Нет связи с сайтом торговли — проверьте интернет и повторите.» means the trade site could not be
reached at all. Check your internet connection, whether the Path of Exile website is up (it may be
down for maintenance) and whether a firewall, VPN or proxy blocks PoE2 Oracle. Then search again:
click «Поиск» or press <kbd>Enter</kbd> in a filter box.

## Request limit messages

- «Лимит запросов trade API — ждём N с…» is normal: the trade site's limit is close, and the app
  waits a few seconds before searching.
- «Сайт торговли временно ограничил поиск — повторите через N мин.» means the trade site has
  locked searches from your IP address for a while, often for minutes. PoE2 Oracle sends no trade
  request until then. Wait that long, then search again; Currency Exchange prices keep working.

The trade site counts every request from your IP address: the site in your browser and other trade
tools use the same limits. Many checks in a row, or several tools at once, run into them sooner.
See [Request limits](price-check.md#request-limits).

## An item is not recognised

When the item text cannot be read, the panel says «Не удалось разобрать предмет» (could not read
the item) with the reason, «Текст предмета сохранён: …» (item text saved) with the file's path, and
a button «Сообщить разработчику» (tell the developer).

- «…поддерживаются только русский и английский клиенты» — only the Russian and English clients
  are supported.
- «…неизвестный класс «…»» — the app does not know this item class yet.

Texts are saved to `%LOCALAPPDATA%\poe2-oracle\data\unparsed`; the last 100 are kept. The app also
saves there, without a message, items it read only partly: a modifier line it could not match
simply has no filter row.

Please report such items: click «Сообщить разработчику», or the link «сообщить об ошибке ↗»
under the item's name when an item was read but got wrong filters or a price that looks wrong.
Either opens the [item problem form](https://github.com/mttzzz/poe2-oracle/issues/new?template=item_problem.yml)
in your browser with the item's text, the app's version and your client's language filled in.
Describe what went wrong and submit it; you need a GitHub account. You can also fill the form by
hand: copy the item in the game with <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd> and paste it.

## Collecting a diagnostics report

1. Open the [settings](settings.md#help), section «Помощь» (help).
2. In the row «Сообщить об ошибке» (report a bug), click «Собрать отчёт» (collect report).
3. A file `PoE2-Oracle-report-<date>_<time>.zip` appears on your desktop and Explorer opens with it
   selected. Under the row, the settings window says «Сохранён: … — приложите его к сообщению об
   ошибке» (saved, attach it to your bug report).

The zip holds plain text files:

- `summary.txt`: the app's version, league and state (catalogs, prices, hotkey, the last item
  checked), your Windows version, monitors, the game window's size, and the display mode, language
  and copy key from the game's settings file, plus the names and sizes of the cached files;
- `logs/`: the log of this run and of the one before;
- `settings.json`: your settings;
- `unparsed/`: the saved item texts.

Everywhere in it, the path of your Windows user folder is replaced with `%USERPROFILE%`, which
hides your Windows user name. Nothing is sent anywhere: you attach the file yourself, and you can
open it and read it first.

«Папка логов» (logs folder) → «Открыть» (open) in the same section opens
`%LOCALAPPDATA%\poe2-oracle\data\logs`, with `poe2-oracle.log` for the current run and
`poe2-oracle.previous.log` for the one before.

## Reporting a bug

Click «Сообщить об ошибке» (report a bug) in the tray icon's menu, or «Сообщить ↗» (report) in the
settings, section «Помощь» (help). PoE2 Oracle then:

1. writes the diagnostics report to your desktop and shows it in Explorer;
2. opens GitHub's [bug report form](https://github.com/mttzzz/poe2-oracle/issues/new?template=bug_report.yml)
   in your browser, with the app's version, your client's language (when known) and a note naming
   the report already filled in.

Describe what happened and how to repeat it, drag the zip from Explorer into the form and submit
it from your GitHub account. PoE2 Oracle itself sends nothing: you see the whole form before it
goes anywhere.

Windows **Settings → Apps → Installed apps** shows the installed version under PoE2 Oracle; the
first line of `summary.txt` in the report has it too.

## Questions

**Does it work in exclusive Fullscreen?**
No. Windows does not show other programs over exclusive fullscreen. Use Windowed Fullscreen.

**Which game languages are supported?**
The English and the Russian client. Other client languages are not read.

**Why is the interface in Russian?**
PoE2 Oracle's interface is Russian only for now. This guide explains every label; items from an
English client are searched on the English trade site all the same.

**Does it need my account, password or session cookie?**
No. PoE2 Oracle searches the trade site like a visitor who is not signed in. See
[Privacy](privacy.md).

**Does it play for me?**
No. Each hotkey press does one thing: it copies one item, sends one chat message or pastes one
search. Whispers to sellers are only copied; you send them yourself.

**Where are my settings? How do I start over?**
In `%APPDATA%\poe2-oracle\config\settings.json`. Quit PoE2 Oracle from the tray, delete the file,
and the next start begins with the defaults and the welcome.
