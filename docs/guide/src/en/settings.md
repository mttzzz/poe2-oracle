# Settings

![The settings window](../images/settings.png)

Open the settings in any of three ways:

- the tray icon's menu → «Настройки» (Settings);
- the gear **⚙** on the price panel;
- start PoE2 Oracle again while it is already running: the running copy opens its settings.

If the settings window is already open, any of these brings it to the front.

The window opens in the middle of the monitor the game is on and stays on top of the game. Drag it
by its title bar; resize it by its edges. The sections are listed on the left, the chosen section's
settings on the right.

Every change applies and is saved at once: there is no Save or Cancel button. Text in a box applies
when you press <kbd>Enter</kbd> or <kbd>Tab</kbd>, click elsewhere or close the window;
<kbd>Esc</kbd> in a box puts the old text back. The **×** in the title bar or <kbd>Esc</kbd> closes
the window; with the league list or a dialog open, <kbd>Esc</kbd> closes that first.

While the settings window is open, the price panel is hidden and the price-check hotkey is off, so
that you can record a new one.

A gold-framed box at the top of «Общие» (General) can show:

- on the very first start, «Добро пожаловать!» (Welcome!) with a reminder of where the tray icon
  is and how to check an item;
- warnings marked **⚠** about your setup, such as exclusive fullscreen or the copy shortcut held by
  another program. See [Troubleshooting](troubleshooting.md).

## General

Section «Общие» (general).

### League

Row «Лига» (league): a list of where prices are searched.

- «Авто · *league*» (auto, the default): the trade site's current league, the first one it lists.
- Any league the trade site lists, named the way the site names it in your client's language, as
  on the price panel's league chip.
- «Своя лига · *name*» (own league): a private league, once its name is set in
  [Account](#account).

Notes under the row's name: «Список лиг с сайта торговли загружается» / «…не загрузился» means the
list of leagues is loading or could not be loaded; «Этой лиги больше нет на сайте торговли — поиск
идёт в текущей» means the league you picked has ended and searches go to the current league instead.

### Client language

Row «Язык клиента игры» (game client language): «Авто» (auto, the default), «Русский» (Russian) or
«English». On auto, the language of each item is recognised from its copied text. The language
decides how the item text is read and which trade site is searched: ru.pathofexile.com for a
Russian client, www.pathofexile.com for an English one.

### Interface scale

Row «Масштаб интерфейса» (interface scale): 100 % by default, from 80 to 150 % in steps of 5, with
the − and + buttons. It sizes the text and controls of everything PoE2 Oracle draws over the game:
the price panel (its width too) and the overlays. The settings window itself does not scale.

### System

Group «Система» (system).

- «Запускать вместе с Windows» (start with Windows), off by default. It adds PoE2 Oracle to the
  apps Windows starts when you sign in; the installer's last page has the same option. Turning it
  on here also re-enables it if it was disabled in Task Manager's startup apps.
- «Проверять обновления» (check for updates), on by default: look for a new version 30 seconds
  after start. The tray menu's update entry works either way. See
  [Updates and uninstall](updates.md).

## Price check

Section «Проверка цены» (price check).

### Hotkey

Row «Проверка цены» (price check). Click the key box and press the new combination: it works at
once. <kbd>Esc</kbd> or a second click on the box keeps the old one.

A combination can be <kbd>Ctrl</kbd> or <kbd>Alt</kbd> (with or without <kbd>Shift</kbd>) plus a
letter A–Z or a digit 0–9, or one of <kbd>F1</kbd>–<kbd>F12</kbd>, alone or with modifiers. Letters
are physical keys: <kbd>Ctrl</kbd>+<kbd>E</kbd> stays the same key when the Russian keyboard layout
is on. The box refuses a combination and says why:

| Message | Reason |
|---|---|
| «Буквам и цифрам нужен Ctrl или Alt — иначе клавиша перестанет печататься» | A letter or digit needs Ctrl or Alt, otherwise the key would stop typing everywhere |
| «Ctrl+C занято: этим сочетанием программа копирует предмет» | Ctrl+C is how the item gets copied |
| «Alt+F4 закрывает окна, в том числе игру» | Alt+F4 closes windows, the game included |
| «Сочетания с клавишей Win не поддерживаются» | Combinations with the Windows key are not supported |
| «Подходят только буквы A–Z, цифры 0–9 и F1–F12» | Only letters, digits and F-keys can be used |
| «Это сочетание уже у другого быстрого действия» | A quick action already uses it |

If another program already holds the combination, the box says so, for example «F7 занято другой
программой — осталось Ctrl+E» (F7 is taken by another program; Ctrl+E stays), and your previous
hotkey keeps working. The tray icon's tooltip always names the hotkey in use.

### Search

Group «Поиск» (search).

- «Продавцы по умолчанию» (default sellers): which sellers every new check searches. «Мгновенный
  выкуп» (instant buyout, the default: you buy in the game and the seller need not be online),
  «Выкуп и онлайн» (instant buyout and sellers online), «Онлайн» (sellers online) or «Все»
  (everyone, offline too). The «Продавцы:» chip on the panel still switches it for one item.
  Settings saved by an older version with its default, «выкуп и онлайн», move to instant buyout
  once; after that, your choice stays.
- «Колонка продавца» (seller column), on by default: the seller's account name in the results
  table.

### Waystones

Group «Путевые камни» (waystones), row «Пометки модификаторов» (modifier marks): how many waystone
modifiers you have marked on the price panel (see [Waystones](price-check.md#waystones)). The
«Сбросить…» (reset) button deletes every mark, but asks first: «Сбросить пометки?» (reset the
marks?). «Сбросить» deletes them; «Отмена» (cancel), <kbd>Esc</kbd> or a click outside the dialog
keeps them.

## Quick actions

Section «Быстрые действия» (quick actions): hotkeys that type chat commands or stash searches into
the game. Each action has its kind («Чат», chat, or «Тайник», stash), its text, its key and a
**×**; «+ Добавить действие» (add action) is below them. See [Quick actions](quick-actions.md).

## XP overlay

Section «Оверлей опыта» (XP overlay). See [XP overlay](xp-overlay.md) for what it shows.

| Setting | Default | Meaning |
|---|---|---|
| «Показывать оверлей опыта» | on | Show the experience rate and the time to the next level above the experience bar |
| «Процент уровня» | off | Also show how much of the current level is done |
| «Таймер карты» | on | Also show the time in the current map, the experience it gave and the session's average map time |
| «Сглаживание скорости» | 10 мин | Rate smoothing: 5, 10, 20 or 30 minutes. Shorter shows a change of farming sooner, longer reads steadier |

## Account

Section «Аккаунт» (account). Signing in to pathofexile.com is needed to search private leagues and
to watch searches.

- **pathofexile.com.** The row tells what is known about your sign-in: «Вход не выполнен» (not
  signed in), «Проверяю вход…» (checking), «Вы вошли как *account*» (signed in as), «Сессия
  истекла» (the session expired: the site no longer accepts it) or «Не удалось проверить вход»
  (the site did not answer; the session is used as it is). «Войти» (sign in) opens
  pathofexile.com's sign-in page in the app's own window: sign in as usual, through Steam too, and
  the window closes by itself. The session is kept in Windows' Credential Manager and sent to
  pathofexile.com only. «Выйти» (sign out) makes PoE2 Oracle forget the session; you stay signed in
  on the site. The sign-in window needs the Microsoft Edge WebView2 Runtime; without it, the row
  shows a link «Скачать с сайта Microsoft» (download from Microsoft).
- **Watching searches.** Row «Следить» (watch): how many searches the price panel's «Следить»
  button is watching, for example «2 из 20» (2 of 20). New listings for them arrive as cards over
  the game. The site allows 20 at most. Signed out, the row reads «нужен вход» (sign-in needed).
- **Private league.** Row «Название лиги» (league name): the name as the trade site writes it,
  brackets included: `My League (PL12345)`. When you press <kbd>Enter</kbd> or leave the box,
  searches move to that league, and «Общие» lists it as «Своя лига · *name*». An empty box brings
  back «Авто». The site does not answer searches in a private league without a sign-in, and the row
  warns about it.

## Help

Section «Помощь» (help).

- «Сообщить об ошибке» (report a bug):
  - «Сообщить ↗» (report) does what the tray menu entry «Сообщить об ошибке» does: it writes the
    diagnostics report to your desktop, shows it in Explorer and opens GitHub's bug report form in
    your browser. See [Reporting a bug](troubleshooting.md#reporting-a-bug).
  - «Собрать отчёт» (collect report) only writes the report: a zip with the logs, the settings and
    the item texts the app could not read, on your desktop, shown in Explorer. Your Windows user
    name is hidden in it. See [Troubleshooting](troubleshooting.md#collecting-a-diagnostics-report).
- «Папка логов» (logs folder) → «Открыть» (open) opens the folder with the logs of this run and the
  one before.
- «О программе» (about): PoE2 Oracle's version and license. «Лицензии» (licenses) opens
  `THIRD-PARTY-NOTICES.html`, which the installer puts next to the app: the licenses of everything
  it includes. «GitHub ↗» opens the project's page.

## Where the settings are kept

`%APPDATA%\poe2-oracle\config\settings.json`, together with your waystone marks and quick actions.
If that file ever cannot be read, PoE2 Oracle renames it to `settings.json.bak` and starts from the
defaults.
