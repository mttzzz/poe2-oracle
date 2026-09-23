# Settings

![The settings window](../images/settings.png)

Open the settings in any of three ways:

- the tray icon's menu → «Настройки» (Settings);
- the gear **⚙** on the price panel;
- start PoE2 Oracle again while it is already running: the running copy opens its settings.

Nothing takes effect until you click «Сохранить» (Save). «Отмена» (Cancel), the **×** in the
window's title bar, <kbd>Esc</kbd> or closing the window any other way throw the changes away.
Opening the settings hides the price panel. While the settings window is in front, the
price-check hotkey is off, so that you can record a new one.

A gold-framed box at the top can show:

- on the very first start, «Добро пожаловать!» (Welcome!) with a reminder of where the tray icon
  is and how to check an item;
- warnings marked **⚠** about your setup, such as exclusive fullscreen or the copy shortcut held by
  another program. See [Troubleshooting](troubleshooting.md).

The sections, top to bottom:

## League

Section «Лига» (league).

- «Авто · *league*» (auto, the default): the trade site's current league, the first one it lists.
- Or pick any league the trade site lists.

Notes under the choices: «Список лиг с сайта торговли не загрузился» means the list of leagues
could not be loaded; «Выбранной лиги больше нет на сайте торговли — поиск пойдёт в текущей» means
the league you picked has ended and searches go to the current league instead.

## Client language

Section «Язык клиента» (client language): «Авто» (auto, the default), «Русский» (Russian) or
«English». On auto, the language of each item is recognised from its copied text. The language
decides how the item text is read and which trade site is searched: ru.pathofexile.com for a
Russian client, www.pathofexile.com for an English one.

## Hotkey

Section «Горячая клавиша» (hotkey), row «Проверка цены» (price check). Click the field and press
the new combination; <kbd>Esc</kbd> keeps the old one.

A combination can be <kbd>Ctrl</kbd> or <kbd>Alt</kbd> (with or without <kbd>Shift</kbd>) plus a
letter A–Z or a digit 0–9, or one of <kbd>F1</kbd>–<kbd>F12</kbd>, alone or with modifiers. Letters
are physical keys: <kbd>Ctrl</kbd>+<kbd>E</kbd> stays the same key when the Russian keyboard layout
is on. The field refuses a combination and says why:

| Message | Reason |
|---|---|
| «Буквам и цифрам нужен Ctrl или Alt — иначе клавиша перестанет печататься» | A letter or digit needs Ctrl or Alt, otherwise the key would stop typing everywhere |
| «Ctrl+C занято: этим сочетанием программа копирует предмет» | Ctrl+C is how the item gets copied |
| «Alt+F4 закрывает окна, в том числе игру» | Alt+F4 closes windows, the game included |
| «Сочетания с клавишей Win не поддерживаются» | Combinations with the Windows key are not supported |
| «Подходят только буквы A–Z, цифры 0–9 и F1–F12» | Only letters, digits and F-keys can be used |
| «Это сочетание уже у другого быстрого действия» | A quick action already uses it |

If another program already holds the combination you saved, PoE2 Oracle keeps your previous
hotkey. The tray icon's tooltip always names the hotkey in use.

## Quick actions

Section «Быстрые действия» (quick actions): hotkeys that type chat commands or stash searches into
the game. See [Quick actions](quick-actions.md).

## Search

Section «Поиск» (search).

- «Допуск значений» (value tolerance), ±10% by default, from 0 to 50% in steps of 5: how far below
  your roll the **min** of each filter row starts, that is, how much the listings' values may
  differ from yours.
- «Продавцы по умолчанию» (default sellers): which sellers a new check searches. «выкуп и онлайн»
  (instant buyout and sellers online, the default), «только мгновенный выкуп» (instant buyout
  only), «только онлайн» (only sellers online), «все, включая офлайн» (everyone, offline too). The
  «Продавцы:» chip on the panel still switches it for one item.
- «Колонка продавца» (seller column), on by default: the seller's account name in the results
  table.

## Appearance

Section «Внешний вид» (appearance), row «Масштаб интерфейса» (interface scale): 100% by default,
from 80 to 150% in steps of 5. It sizes the text and controls of everything PoE2 Oracle draws over
the game: the price panel (its width too), the trade request cards and the XP overlay.

## XP overlay

Section «Оверлей опыта» (XP overlay). See [XP overlay](xp-overlay.md) for what it shows.

| Setting | Default | Meaning |
|---|---|---|
| «Показывать оверлей опыта» | on | Show the experience rate and the time to the next level above the experience bar |
| «Процент уровня» | off | Also show how much of the current level is done |
| «Таймер карты» | on | Also show the time in the current map, the experience it gave and the session's average map time |
| «Сглаживание скорости» | 10 мин | Rate smoothing: 5, 10, 20 or 30 minutes. Shorter shows a change of farming sooner, longer reads steadier |

## Trading

Section «Торговля» (trading). See [Trade requests](trade-requests.md).

- «Запросы покупателей» (buyer requests), on by default: cards for buyers' whispers, with buttons
  that answer them in the game.
- «Звук при новом запросе» (sound on a new request), on by default: plays the Windows "Asterisk"
  system sound when a new card appears.

## System

Section «Система» (system).

- «Запускать вместе с Windows» (start with Windows), off by default. It adds PoE2 Oracle to the
  apps Windows starts when you sign in; the installer's last page has the same option. Turning it
  on here also re-enables it if it was disabled in Task Manager's startup apps.
- «Проверять обновления» (check for updates), on by default: look for a new version 30 seconds
  after start. The tray menu's update entry works either way. See
  [Updates and uninstall](updates.md).

## Diagnostics

Section «Диагностика» (diagnostics), row «Отчёт для разработчика» (report for the developer):

- «Сообщить об ошибке» (report a bug) does what the tray menu entry of the same name does: it
  writes the diagnostics report to your desktop, shows it in Explorer and opens GitHub's bug report
  form in your browser. See [Reporting a bug](troubleshooting.md#reporting-a-bug).
- «Собрать отчёт» (collect report) only writes the report: a zip with the logs, the settings and
  the item texts the app could not read, on your desktop, shown in Explorer. Your Windows user
  name is hidden in it. See [Troubleshooting](troubleshooting.md#collecting-a-diagnostics-report).
- «Папка логов» (logs folder) opens the folder with the logs.

None of these needs saving: they change no settings.

## Where the settings are kept

`%APPDATA%\poe2-oracle\config\settings.json`, together with your waystone marks and quick actions.
If that file ever cannot be read, PoE2 Oracle renames it to `settings.json.bak` and starts from the
defaults.
