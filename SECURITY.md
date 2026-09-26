# Security policy

[English](#english) · [Русский](#русский)

## English

### Supported versions

Security fixes go into the [latest release](https://oracle.pushka.biz/) only. Update from the tray
menu (**Check for updates**) or install the latest installer over your copy.

### Reporting a vulnerability

Report it privately from the app: **Report a problem or idea…** in the tray menu, or **Write to the
developer** in the **Help** section of the settings. Without the app, use the form at
<https://oracle.pushka.biz/report.html>. A report reaches only the developer, as an issue in the
project's private repository and a Telegram message; nothing of it is published. Say that it is a
security problem, and leave a contact (Telegram, Discord or email) so that you can get a reply.

Please don't post details anywhere public until a fixed release is out. Describe what an attacker
could do, how to reproduce it (a proof of concept if you have one) and anything that limits it; a
report from the app carries the app version by itself. Once a fix is released, the changelog
credits you unless you ask not to be named.

### Scope

In scope:

- **The app** (`poe2-oracle.exe`): how it handles answers from the trade site, GGG's CDN,
  poe2scout and oracle.pushka.biz, the game's log and the clipboard text it reads, the files it
  writes, and what it sends in a report.
- **Sign-in** (`crates/poe2-oracle/src/login.rs`, `platform/login_window.rs`, `session.rs`): the
  pathofexile.com sign-in window and your session cookie (`POESESSID`), which the app keeps in the
  Windows Credential Manager and sends only to www.pathofexile.com and ru.pathofexile.com.
- **The installer and uninstaller** (`packaging/installer.nsi`): the per-user install, its registry
  entries and what it deletes.
- **The updater** (`crates/auto-update`): the release check, the download, the check of the
  release's Ed25519 signature and the installer's SHA-256, and the silent install.
- **The web service** at oracle.pushka.biz (`crates/oracle-web`): the report form and API, the
  update proxy, and what the service keeps (counts per day, no ids).
- **Data handling**: what the app keeps on disk, and the diagnostics report: what it holds, its
  masking of your Windows user name and folders, and that it leaves your computer only in a report
  you send.

Out of scope:

- Path of Exile 2, the trade site, GGG's CDN, poe2scout, GitHub, Telegram and the service's hosting
  themselves; please report to their owners.
- Attacks that need someone who already controls your Windows account.

Known limitations: the installer and the exe are not code-signed yet, so Windows SmartScreen warns
about them. An update is installed only if the release's Ed25519 signature over `SHA256SUMS` checks
out against the public key built into the app and the installer matches its SHA-256 there, so
neither oracle.pushka.biz nor anyone on the way can hand out an installer the release pipeline
didn't sign. That doesn't protect against a compromised release pipeline or signing key.

## Русский

### Поддерживаемые версии

Исправления уязвимостей выходят только в [последнем выпуске](https://oracle.pushka.biz/ru/).
Обновитесь из меню в трее («Проверить обновления») или установите последний установщик поверх своей
копии.

### Как сообщить об уязвимости

Сообщите закрыто, из самой программы: **«Сообщить о проблеме или идее…»** в меню значка в трее или
**«Написать разработчику»** в разделе «Помощь» настроек. Без программы — через форму на
<https://oracle.pushka.biz/ru/report.html>. Сообщение получает только разработчик — задачей в
закрытом репозитории проекта и сообщением в Telegram; публично оно не появляется. Напишите, что это
уязвимость, и оставьте контакт (Telegram, Discord или почту), чтобы получить ответ.

Пожалуйста, не публикуйте подробности, пока не выйдет исправленная версия. Опишите, что может
сделать злоумышленник, как это воспроизвести (с доказательством, если оно есть) и что ограничивает
атаку; сообщение из программы само несёт её версию. Когда исправление выйдет, в журнале изменений
вас поблагодарят, если вы не попросите не называть вас.

### Что входит

Входит:

- **Программа** (`poe2-oracle.exe`): как она обрабатывает ответы сайта торговли, CDN GGG,
  poe2scout и oracle.pushka.biz, журнал игры и текст из буфера обмена, какие файлы пишет и что
  отправляет в сообщении разработчику.
- **Вход** (`crates/poe2-oracle/src/login.rs`, `platform/login_window.rs`, `session.rs`): окно входа
  на pathofexile.com и cookie вашей сессии (`POESESSID`), которую программа хранит в диспетчере
  учётных данных Windows и отправляет только на www.pathofexile.com и ru.pathofexile.com.
- **Установщик и деинсталлятор** (`packaging/installer.nsi`): установка для одного пользователя,
  записи в реестре и то, что удаляется.
- **Обновление** (`crates/auto-update`): проверка выпуска, скачивание, проверка подписи Ed25519
  выпуска и SHA-256 установщика и тихая установка.
- **Веб-сервис** oracle.pushka.biz (`crates/oracle-web`): форма и API сообщений, прокси обновлений
  и то, что сервис хранит (счётчики по дням, без идентификаторов).
- **Работа с данными**: что программа хранит на диске, и отчёт диагностики — что в нём, как в нём
  скрыты имя пользователя Windows и пути к папкам и то, что он покидает компьютер только в
  отправленном вами сообщении.

Не входит:

- Сама Path of Exile 2, сайт торговли, CDN GGG, poe2scout, GitHub, Telegram и хостинг сервиса — о
  них сообщайте их владельцам.
- Атаки, для которых злоумышленник уже должен управлять вашей учётной записью Windows.

Известные ограничения: установщик и exe пока не подписаны цифровой подписью, поэтому SmartScreen
Windows о них предупреждает. Обновление ставится, только если подпись Ed25519 выпуска под
`SHA256SUMS` сходится с открытым ключом, встроенным в программу, а установщик совпадает с суммой
SHA-256 оттуда, — так ни oracle.pushka.biz, ни кто-то по дороге не может подсунуть установщик,
который не подписал процесс выпуска. От взломанного процесса выпуска или утёкшего ключа подписи
это не защищает.
