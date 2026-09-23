# Security policy

[English](#english) · [Русский](#русский)

## English

### Supported versions

Security fixes go into the [latest release](https://github.com/mttzzz/poe2-oracle/releases/latest)
only. Update from the tray menu (**Проверить обновления**, Check for updates) or install the latest
installer over your copy.

### Reporting a vulnerability

Report it privately through GitHub's private vulnerability reporting:
**<https://github.com/mttzzz/poe2-oracle/security/advisories/new>**.

Please don't open a public issue or post details anywhere else until a fixed release is out.
Include the app version, what an attacker could do, how to reproduce it (a proof of concept if you
have one) and anything that limits it. You will get a reply in the private advisory; once a fix is
released, the advisory is published, crediting you unless you ask not to be named.

### Scope

In scope:

- **The app** (`poe2-oracle.exe`): how it handles answers from the trade site, poe.ninja,
  poe2scout and GitHub, the game's log and the clipboard text it reads, and the files it writes.
- **The installer and uninstaller** (`packaging/installer.nsi`): the per-user install, its registry
  entries and what it deletes.
- **The updater** (`crates/auto-update`): the release check, the download, the SHA-256
  verification and the silent install.
- **Data handling**: what the app keeps on disk, and the diagnostics report with its masking of
  your Windows user name.

Out of scope:

- Path of Exile 2, the trade site, poe.ninja, poe2scout and GitHub themselves; please report to
  their owners.
- Attacks that need someone who already controls your Windows account.

Known limitations: the installer and the exe are not code-signed yet. An update is checked against
the `SHA256SUMS` published in the same GitHub release, which catches a corrupted or swapped
download but not a compromised release.

## Русский

### Поддерживаемые версии

Исправления уязвимостей выходят только в
[последнем выпуске](https://github.com/mttzzz/poe2-oracle/releases/latest). Обновитесь из меню в
трее («Проверить обновления») или установите последний установщик поверх своей копии.

### Как сообщить об уязвимости

Сообщите закрыто, через приватные отчёты об уязвимостях на GitHub:
**<https://github.com/mttzzz/poe2-oracle/security/advisories/new>**.

Пожалуйста, не открывайте публичную задачу и не публикуйте подробности где-либо ещё, пока не
выйдет исправленная версия. Укажите версию программы, что может сделать злоумышленник, как это
воспроизвести (с доказательством, если оно есть) и что ограничивает атаку. Ответ придёт в том же
закрытом отчёте; когда исправление выйдет, отчёт будет опубликован с благодарностью вам, если вы
не попросите не называть вас.

### Что входит

Входит:

- **Программа** (`poe2-oracle.exe`): как она обрабатывает ответы сайта торговли, poe.ninja,
  poe2scout и GitHub, журнал игры и текст из буфера обмена, и какие файлы пишет.
- **Установщик и деинсталлятор** (`packaging/installer.nsi`): установка для одного пользователя,
  записи в реестре и то, что удаляется.
- **Обновление** (`crates/auto-update`): проверка выпуска, скачивание, сверка SHA-256 и тихая
  установка.
- **Работа с данными**: что программа хранит на диске и отчёт диагностики, в котором скрыто имя
  пользователя Windows.

Не входит:

- Сама Path of Exile 2, сайт торговли, poe.ninja, poe2scout и GitHub — о них сообщайте их
  владельцам.
- Атаки, для которых злоумышленник уже должен управлять вашей учётной записью Windows.

Известные ограничения: установщик и exe пока не подписаны цифровой подписью. Обновление сверяется
с `SHA256SUMS` из того же выпуска на GitHub: это защищает от повреждённого или подменённого файла
при скачивании, но не от взломанного выпуска.
