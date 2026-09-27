# Security policy

[English](#english) · [Русский](#русский)

## English

### Supported versions

Security fixes go into the [latest release](https://oracle.pushka.biz/) only. With **Update
automatically** on in the settings (the default), the app installs it by itself within minutes of
its release; otherwise install the latest installer over your copy.

### Reporting a vulnerability

Please report it privately, not in a public issue:

- **From the app:** **Write to the developer** in the **Help** section of the settings.
- **Without the app:** the form at <https://oracle.pushka.biz/report.html>.
- **On GitHub:** the repository's **Security** tab → **Report a vulnerability**, which only the
  maintainer sees.

A report from the app or the form reaches the developer alone, as an issue in a private GitHub
repository and a Telegram message; nothing of it is published. Say that it is a security problem,
and leave a contact (Telegram, Discord or email) so that you can get a reply. Describe what an
attacker could do, how to reproduce it (a proof of concept if you have one) and anything that
limits it; a report from the app carries the app version by itself.

What happens next: the developer answers on the contact you left, confirms the problem and fixes it
in a new release, which installed copies with automatic updates pick up by themselves. Please don't
post details anywhere public until that release is out. The changelog then credits you, unless you
ask not to be named.

### Scope

In scope:

- **The app** (`poe2-oracle.exe`): how it handles answers from the trade site, GGG's CDN,
  poe2scout and oracle.pushka.biz, the game's log and the clipboard text it reads, the files it
  writes, and what it sends in a report.
- **Sign-in** (`crates/poe2-oracle/src/login.rs`, `platform/login_window.rs`, `session.rs`,
  `platform/credentials.rs`; `crates/trade-client/src/account.rs` and `private_leagues.rs`, which
  read the account and private league pages with it): the pathofexile.com sign-in window and your
  session cookie (`POESESSID`), which the app keeps in the Windows Credential Manager and sends only
  to www.pathofexile.com and ru.pathofexile.com.
- **The installer and uninstaller** (`packaging/installer.nsi`): the per-user install, its registry
  entries and what it deletes.
- **The updater** (`crates/auto-update`, `crates/poe2-oracle/src/updates.rs`): the event stream it
  follows, the release and game data pack checks, the downloads, the check of the Ed25519
  signature over `SHA256SUMS` and of each file's SHA-256, and the start of the silent install.
- **Game data packs** (`crates/oracle-data`, `crates/poe2-oracle/src/data_pack.rs`): reading a
  signed pack's zip, the check of each table against its SHA-256 before it is parsed, and the
  folder the app unpacks packs into.
- **The release pipeline** (`.github/workflows/release.yml`, `data-release.yml`,
  `crates/release-sign`): what can reach the signing key and what gets signed.
- **The web service** at oracle.pushka.biz (`crates/oracle-web`): the report form and API, the
  update proxy and its event stream, and what the service keeps (counts per day with no ids, and
  report rate-limit counts per address until their time window ends).
- **Data handling**: what the app keeps on disk, and the diagnostics report: what it holds, its
  masking of your Windows user name and folders, and that it leaves your computer only in a report
  you send.

Out of scope:

- Path of Exile 2, the trade site, GGG's CDN, poe2scout, GitHub, Telegram and the service's hosting
  themselves; please report to their owners.
- Attacks that need someone who already controls your Windows account.

### How releases are protected

Every release, of the app or of a game data pack, carries a `SHA256SUMS` file with the SHA-256 of
each of its files, and `SHA256SUMS.sig`, an Ed25519 signature of it. The signing key never leaves
the release pipeline: it is a GitHub Actions secret, which only the signing job of
`.github/workflows/release.yml` and `data-release.yml` can read. Its public half,
[`crates/auto-update/release-signing-key.pub`](crates/auto-update/release-signing-key.pub), is built
into the app. An update is installed only if the signature checks out against that key and the
downloaded file matches its SHA-256 in `SHA256SUMS`, so neither oracle.pushka.biz nor anyone on the
way can hand out an installer or a data pack the release pipeline didn't sign.

To check a download yourself, get the release's `SHA256SUMS` and `SHA256SUMS.sig` from
`https://oracle.pushka.biz/download/v<version>/` (the latest release) or from its GitHub release,
then:

```sh
cargo run -p release-sign -- verify SHA256SUMS SHA256SUMS.sig "$(cat crates/auto-update/release-signing-key.pub)"
```

and compare the installer's SHA-256 (`Get-FileHash .\PoE2-Oracle-Setup-<version>.exe` in
PowerShell) with its line in `SHA256SUMS`.

Known limitations:

- The installer and the exe are not code-signed (Authenticode) yet, so Windows SmartScreen warns
  about them. The Ed25519 signature protects updates, not the first download: check that one as
  shown above.
- None of this protects against a compromised release pipeline or signing key.

## Русский

### Поддерживаемые версии

Исправления уязвимостей выходят только в [последнем выпуске](https://oracle.pushka.biz/ru/). Если в
настройках включено «Обновлять автоматически» (по умолчанию включено), программа сама ставит его в
течение нескольких минут после выхода; иначе установите последний установщик поверх своей копии.

### Как сообщить об уязвимости

Пожалуйста, сообщайте закрыто, а не в публичной задаче:

- **Из программы:** **«Написать разработчику»** в разделе «Помощь» настроек.
- **Без программы:** форма на <https://oracle.pushka.biz/ru/report.html>.
- **На GitHub:** вкладка **Security** репозитория → **Report a vulnerability** — это видит только
  сопровождающий.

Сообщение из программы или с формы получает только разработчик — задачей в закрытом репозитории на
GitHub и сообщением в Telegram; публично оно не появляется. Напишите, что это уязвимость, и
оставьте контакт (Telegram, Discord или почту), чтобы получить ответ. Опишите, что может сделать
злоумышленник, как это воспроизвести (с доказательством, если оно есть) и что ограничивает атаку;
сообщение из программы само несёт её версию.

Что будет дальше: разработчик ответит по оставленному контакту, подтвердит проблему и исправит её в
новом выпуске, который установленные копии с автоматическими обновлениями поставят сами.
Пожалуйста, не публикуйте подробности, пока этот выпуск не вышел. В журнале изменений вас
поблагодарят, если вы не попросите не называть вас.

### Что входит

Входит:

- **Программа** (`poe2-oracle.exe`): как она обрабатывает ответы сайта торговли, CDN GGG,
  poe2scout и oracle.pushka.biz, журнал игры и текст из буфера обмена, какие файлы пишет и что
  отправляет в сообщении разработчику.
- **Вход** (`crates/poe2-oracle/src/login.rs`, `platform/login_window.rs`, `session.rs`,
  `platform/credentials.rs`; `crates/trade-client/src/account.rs` и `private_leagues.rs`, которые
  читают с ней страницу аккаунта и приватных лиг): окно входа на pathofexile.com и cookie вашей
  сессии (`POESESSID`), которую программа хранит в диспетчере учётных данных Windows и отправляет
  только на www.pathofexile.com и ru.pathofexile.com.
- **Установщик и деинсталлятор** (`packaging/installer.nsi`): установка для одного пользователя,
  записи в реестре и то, что удаляется.
- **Обновление** (`crates/auto-update`, `crates/poe2-oracle/src/updates.rs`): поток событий, за
  которым оно следит, проверка выпусков и пакетов данных игры, скачивание, проверка подписи Ed25519
  под `SHA256SUMS` и SHA-256 каждого файла и запуск тихой установки.
- **Пакеты данных игры** (`crates/oracle-data`, `crates/poe2-oracle/src/data_pack.rs`): чтение zip
  подписанного пакета, проверка SHA-256 каждой таблицы перед разбором и папка, куда программа
  распаковывает пакеты.
- **Процесс выпуска** (`.github/workflows/release.yml`, `data-release.yml`, `crates/release-sign`):
  что может добраться до ключа подписи и что им подписывается.
- **Веб-сервис** oracle.pushka.biz (`crates/oracle-web`): форма и API сообщений, прокси обновлений
  и его поток событий и то, что сервис хранит (счётчики по дням, без идентификаторов, и счётчики
  ограничения сообщений по адресам — до конца их окна времени).
- **Работа с данными**: что программа хранит на диске, и отчёт диагностики — что в нём, как в нём
  скрыты имя пользователя Windows и пути к папкам и то, что он покидает компьютер только в
  отправленном вами сообщении.

Не входит:

- Сама Path of Exile 2, сайт торговли, CDN GGG, poe2scout, GitHub, Telegram и хостинг сервиса — о
  них сообщайте их владельцам.
- Атаки, для которых злоумышленник уже должен управлять вашей учётной записью Windows.

### Как защищены выпуски

Каждый выпуск — программы или пакета данных игры — несёт файл `SHA256SUMS` с суммами SHA-256 всех
своих файлов и `SHA256SUMS.sig` — подпись Ed25519 под ним. Ключ подписи не покидает процесс выпуска:
это секрет GitHub Actions, который читает только задание подписи в `.github/workflows/release.yml`
и `data-release.yml`. Открытая половина ключа,
[`crates/auto-update/release-signing-key.pub`](crates/auto-update/release-signing-key.pub), вшита в
программу. Обновление ставится, только если подпись сходится с этим ключом, а скачанный файл — с
его суммой SHA-256 в `SHA256SUMS`, — так ни oracle.pushka.biz, ни кто-то по дороге не может
подсунуть установщик или пакет данных, который не подписал процесс выпуска.

Чтобы проверить скачанное самим, возьмите `SHA256SUMS` и `SHA256SUMS.sig` выпуска с
`https://oracle.pushka.biz/download/v<версия>/` (последний выпуск) или из его выпуска на GitHub,
затем:

```sh
cargo run -p release-sign -- verify SHA256SUMS SHA256SUMS.sig "$(cat crates/auto-update/release-signing-key.pub)"
```

и сравните SHA-256 установщика (`Get-FileHash .\PoE2-Oracle-Setup-<версия>.exe` в PowerShell) с его
строкой в `SHA256SUMS`.

Известные ограничения:

- Установщик и exe пока не подписаны цифровой подписью (Authenticode), поэтому SmartScreen Windows
  о них предупреждает. Подпись Ed25519 защищает обновления, а не первое скачивание: его проверьте,
  как показано выше.
- От взломанного процесса выпуска или утёкшего ключа подписи всё это не защищает.
