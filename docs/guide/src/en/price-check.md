# Price check

![The price panel next to the inventory, checking a rare item](../images/price-check.png)

## Checking an item

1. In the game, point at an item: in your inventory, in the stash or in a vendor's window.
2. Press <kbd>Ctrl</kbd>+<kbd>E</kbd>. This is the default; you can pick another key in the
   [settings](settings.md#hotkey).

PoE2 Oracle presses the game's own item-copy shortcut, <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd>,
reads the item text from the clipboard and puts back whatever you had copied before. A panel then
opens over the full height of the game window: next to the inventory if the cursor was on the right
half of the game, next to the stash if it was on the left half. It never covers the inventory or
the stash.

> [!NOTE]
> <kbd>Alt</kbd> in that shortcut is the game's key for advanced item descriptions. If you bound
> that action to another modifier key in the game's options, PoE2 Oracle reads your binding from the
> game's settings file and presses <kbd>Ctrl</kbd> + that key + <kbd>C</kbd> instead.

- The hotkey works only while the game or the panel is the window in front. In every other program
  <kbd>Ctrl</kbd>+<kbd>E</kbd> keeps its usual meaning.
- Press the hotkey over another item to check that one; the panel starts over for each item.
- With no item under the cursor, nothing happens.
- Close the panel with <kbd>Esc</kbd> or the **×** in its top-right corner. It does not close when
  the mouse leaves the item, so you can move into it and work with it. While the panel is open,
  <kbd>Esc</kbd> closes the panel and does not reach the game.
- Clicks on chips, checkboxes and buttons leave the keyboard with the game. Only the min/max boxes
  take the keyboard when you click into them; when the panel closes, the game gets it back.
- While the panel is open, the mouse over it (clicks and the wheel) works on the panel, not on the
  game.

The panel's top bar shows the league being searched, «PoE2 Oracle · *league*», and, once the
exchange prices have loaded, how many Exalted Orbs a Divine Orb is worth, drawn with the two
currency icons. The gear **⚙** opens the [settings](settings.md); **×** closes the panel.

> [!TIP]
> The app's interface is in Russian and writes numbers with a decimal comma: `1,72` means 1.72.
> Large numbers are shortened with `k` and `M`: `4,1k` is 4,100. Every price is a number followed
> by the currency's icon, such as the Divine Orb's or the Exalted Orb's; a currency without an
> icon is named instead.

## The nameplate

At the top of the panel:

- the item's picture, when the item is in the app's item table;
- the name in the game's rarity colour, and the base type under it for rare and unique items;
- links «poe2db ↗» (the item's page on poe2db.tw, in Russian for items from a Russian client) and
  «вики ↗» (wiki: the English Path of Exile 2 wiki, poe2wiki.net), for items in the app's item
  table. They open in your browser;
- the link «сообщить об ошибке ↗» (report a problem), for when the item was read or priced
  wrong. It opens GitHub's item problem form in your browser with the item's text, the app's
  version and your client's language filled in; you describe what is wrong and submit it from
  your GitHub account. See [Reporting a problem](troubleshooting.md#reporting-a-bug).

## The chip row

Under the nameplate, a row of chips describes the item and how it is searched.

Chips that only show information:

| Chip | Meaning |
|---|---|
| «Класс: …» | Item class, as the game names it |
| «Ур. предмета: …» | Item level |
| «Требуется ур.: …» | Required character level |
| «Гнёзда: …» | Sockets |
| «Качество: +…%» | Quality |
| «Осквернено» | The item is corrupted |
| «В стопке: …» | Stack size |

Chips with a gold border and **↔** at the end switch something when clicked. Hover any of them for
an explanation.

| Chip | What it switches |
|---|---|
| «Класс: …» ↔ «База: …» | Search among every item of the class (a rare is compared with its whole class by default) or only among items of this base type. The base can matter on its own: a base implicit, a sought-after base. |
| «Редкость: …» (rarity) | «волшебные» (magic), «редкие» (rare) or «обычные» (normal) searches only items of the same rarity; «все, кроме уникальных» searches every rarity except unique. A magic item is compared with magic items by default, a rare with all non-unique items. |
| «Без осквернённых» ↔ «И осквернённые» | For an item that is not corrupted, corrupted listings are left out by default: they cannot be changed and may have modifiers this item lacks. Click to include them. |
| «Св-ва: N из M» (properties) | How many of the listed searchable properties are selected. Click to select all of them, or none. |

A unique item has no rarity chip: its name already pins the search down. Mirrored and sanctified
listings are left out of the search for ordinary gear unless your item is one; there is no chip
for that.

## Filters

Below the chips, the item's properties are listed in sections:

| Section | Contents |
|---|---|
| «Свойства предмета» (item properties) | Item level, sockets, quality, defences, weapon DPS, attack speed, critical hit chance, waystone tier and the like |
| «Собственные свойства» (implicit modifiers) | Implicit modifiers |
| «Префиксы · N» (prefixes) | Prefix modifiers |
| «Суффиксы · N» (suffixes) | Suffix modifiers |
| «Прочие свойства» or «Свойства» (other properties) | Enchantments, rune and crafted modifiers and other lines that are not prefixes or suffixes |
| «Суммарные (псевдо)» (totals, pseudo) | Sums across modifiers: total life, total elemental resistance, all attributes and so on; some are labelled «сумма» (sum) |

Each row has a checkbox (whether the row takes part in the search), the modifier text with your
roll highlighted in blue, a tier badge and **min**/**max** boxes; a modifier whose tier the app
knows also has a roll slider under it (see [Min and max](#min-and-max)). The tier badge is
gold-filled for T1, gold-outlined for T2 and grey for lower tiers. Small labels under a row say
where a modifier comes from: «усилитель» (from a rune or other augment), «мастер» (crafted),
«расколотый» (fractured), «зачарование» (enchantment), «очернённый» (desecrated). The label
«сумма» (sum), explained when you hover it, marks a total whose value adds up the stat from all
the item's modifiers: the trade site searches listings by the same sum. A line the trade site
cannot search says «не участвует в поиске» (not part of the search).

On a magic or rare item that can still take modifiers, rows «Свободный префикс» / «Свободных
префиксов: N» and «Свободный суффикс» / «Свободных суффиксов: N» (open prefix/suffix slots) can be
searched too.

Some rows are folded away: unselected totals, and minor properties such as a weapon's physical or
elemental DPS when it is only a small part of the total. «▾ Суммарные и второстепенные: ещё N»
(totals and minor rows: N more) unfolds them; «▴ Свернуть …» folds them again.

### Search profiles

A search profile decides which rows are selected and where their bounds start. The row of search
options under the «Поиск» (Search) button starts with «Профиль:» (profile) and a chip naming the
current profile. Click the chip for a menu of the four profiles, each with a note on what it
searches:

| Profile | Note in the menu |
|---|---|
| «Быстрая цена» (quick price) | «до 4 самых ценных свойств, от ваших значений» (up to the 4 most valuable properties, from your values) |
| «Точное совпадение» (exact match) | «все свойства, от ваших значений» (all properties, from your values) |
| «Широкий −10 %» (broad −10%) | «отмеченные свойства, минимумы на 10 % ниже» (the ticked properties, minimums 10% lower) |
| «База для крафта» (crafting base) | «собственные и расколотые свойства на этой же базе» (implicit and fractured properties on the same base) |

Picking a profile, the current one included, sets the rows up and searches once. A click outside
the menu or <kbd>Esc</kbd> closes it without searching. The chip is there only for items searched
by their filter rows, not for Currency Exchange items or items searched by their exact name.

An item opens with «Точное совпадение» if it is a unique, a relic, a flask or a charm, or if it can
no longer be modified (corrupted, mirrored, sanctified or unmodifiable) and is not a waystone.
Every other item opens with «Быстрая цена», and so does a crafting base that can still be
modified: an item of normal rarity, a fractured item, or one with quality above 20% or more rune
sockets than its base has.

What each profile selects:

- «Быстрая цена» selects rows the way PoE Overlay II does. Every modifier gets a score from its
  tier (against the best tier the item's level allows), its tags weighted by the item class, how
  high it rolled within its range, and a few fixed bonuses. Up to 4 rows scoring 3 or more are
  selected, highest scores first, and no more than 2 of them may be «сумма» (sum) rows. A local
  modifier (defences, weapon damage) counts toward the property it changes, and that property's
  row is selected instead. Magic items follow the same rule.
- «Точное совпадение» selects every row outside «Свойства предмета» (item properties) that is not
  folded away, plus the item properties «Быстрая цена» would pick.
- «Широкий −10 %» keeps the rows that are ticked now and sets each **min** 10% below the item's
  roll.
- «База для крафта» selects the implicit, fractured and granted-skill rows and item level, and
  searches among items of the same base type: the class chip switches to «База: …». The other
  profiles set that chip back to its default for the item.

In every profile a few stats are always selected: base implicits that define the base, such as
extra projectiles or bolts, chaining, piercing, maximum elemental resistances, spirit or movement
speed; unrevealed modifiers; a timeless jewel's legend; and granted skills of level 19 or higher
(of any level on an amulet).

In «Быстрая цена» and «Точное совпадение», item level, sockets, quality, gem and waystone rows keep
their usual checkbox. On a waystone, the modifiers start unselected, a desecrated one excepted:
its tier and properties set the price, its modifiers only make the map harder. Select a modifier
yourself to search for it.

### Min and max

- Only **min** is filled in: your roll in «Быстрая цена», «Точное совпадение» and «База для
  крафта», 10% below it in «Широкий −10 %». **max** stays empty, since a higher roll is never a
  reason to leave a listing out.
- For the few modifiers where a lower number is better, **max** is filled in instead.
- Type digits, a point or a comma (both give a decimal point) and a minus sign. The first key after
  you click into a box replaces its value.
- <kbd>Enter</kbd> in a box runs the search again with the new bounds.

Under a modifier row whose tier the app's tier table knows, a slider runs from the lowest to the
highest roll of that modifier across all its tiers on this kind of item, with those two numbers at
its ends. A blue tick marks your roll. A gold handle marks the search's **min** (its **max** on
rows where a lower number is better), and the part of the track the search admits is lit gold.
Click or drag on the slider to move the handle, and the **min** (or **max**) box follows; typing
in the box moves the handle. Hover the slider for a short explanation.

Hover a tier badge to see where the tier stands, for example «T3 из 9 · с 68 ур. предмета · лучший
доступный T2» (T3 of 9 · from item level 68 · best available T2): the tier among all tiers of that
modifier on this kind of item, the item level that tier needs, and the best tier the item's level
can roll.

The button «минимум тира» (tier minimum), to the right of «Поиск» (Search), shows while a ticked
modifier row has a known tier. It sets the **min** of every such ticked row to the bottom of its
current tier, so the search finds the same tier and better. Rows where a lower number is better
are left alone.

Ticking or unticking rows, moving a slider, «минимум тира» and switching the class, rarity or
corrupted chips take effect with the next search: press <kbd>Enter</kbd> in a box or click «Поиск»
(Search).

## Search options

Above the results, the row under the «Поиск» (Search) button starts with «Профиль:» (profile) and
the current profile's chip, which opens the menu described in [Search profiles](#search-profiles).
Two more choices follow. Each click on them moves to the next value and searches again at once.

- «Продавцы:» (sellers): «выкуп и онлайн» (instant buyout and sellers online) → «только
  мгновенный выкуп» (instant buyout only, the default: the game's own auction sells the item
  without the seller online) → «только онлайн» (only sellers online, to trade in person) → «все,
  включая офлайн» (everyone, offline sellers too). The starting value comes from the
  [settings](settings.md#search).
- «Цена:» (price): «любая валюта» (any currency) → the Exalted Orb or the Divine Orb («или»
  between their icons) → «только» (only) Exalted Orb → only Divine Orb → only Chaos Orb. The
  currencies are shown by their icons.

## Results

- «Найдено: N» (found) is the number of listings on the trade site. The link next to it,
  «www.pathofexile.com/trade ↗» (or «ru.pathofexile.com/trade ↗»), opens the same search in your
  browser.
- The table shows the 10 cheapest listings, cheapest first. Several listings of one seller at the
  same price share a row.
- Columns: «Цена» (price), «Ур.» (item level), «Продавец» (seller; can be hidden in the settings)
  and «Выставлен» (listed): «только что» (just now), «5 мин. назад» (5 min ago), «3 ч. назад»
  (3 h ago), «2 дн. назад» (2 days ago), «1 мес. назад» (1 month ago).
- A price is the amount and the currency's icon. Markers after it: **?** means the price comes
  from the stash tab's name, not from a note on the item, and may not be meant; **× N** means the
  seller listed N of these at that price. A price in a currency other than the Divine or the
  Exalted Orb also shows roughly what it is worth in one of those two.
- An envelope **✉** before the seller's name means the seller trades in person. A coloured dot
  shows their status: pink online, orange away, red offline. Listings without the envelope are
  instant buyout: buy those through the trade site.

### Copying the whisper

Click a row with **✉** to copy the trade site's whisper message for that listing. For a few
seconds the row reads «✓ скопировано — вставьте в чат» (copied, paste it into chat). Open the chat
in the game, paste with <kbd>Ctrl</kbd>+<kbd>V</kbd> and send it yourself. PoE2 Oracle never sends
it for you.

### Listing details

Hover a row to see the listing the way the game's own item tooltip shows it: the name in its
rarity colour, properties, requirements, sockets with their runes, item level, every modifier,
flags such as «Осквернено» (corrupted) and the seller's note. Each modifier has its tier on the
left and «ур. N» (the item level it needs) on the right. The range a value rolls in follows it in
brackets, for example `+38(36-40)%`.

The modifiers you search for are marked:

- green **✓** — the modifier meets your bounds;
- red **✗** with «(нужно …)» (needed) — it is outside them, and the bounds are shown;
- «Нет у этого предмета:» (this item lacks) at the foot of the card — selected properties the
  listing does not have at all.

### When nothing matches exactly

When a search finds nothing, the panel says «Ничего не найдено» (nothing found) and offers two
broader searches. Each costs one search on the trade site, and only when you press it:

- «Широкий −10 %» (broad −10%): the same ticked rows, each **min** 10% below the item's roll. Not
  offered when that is already the profile.
- «Совпадение N из M» (N of M match): listings that have at least N of the M ticked rows, that is,
  all of them but one. Item properties such as defences, and «сумма» rows, are not counted and
  always stay required. Offered only when at least two ticked rows count.

After «Совпадение N из M», the results say «Точных совпадений нет — показаны предметы хотя бы с N
из M выбранных свойств» (no exact matches, showing items with at least N of M selected properties),
and each row shows how many of the selected properties it has, for example `3/4`. If that search
finds nothing either, the panel says «Ничего не найдено — даже с N из M выбранных свойств»
(nothing found, even with N of M selected properties).

You can also widen the search yourself: untick some rows, lower some **min** values, search by
class instead of base, or let other sellers in with the «Продавцы:» chip.

## Currency and exchange items

![The market card for a currency item](../images/market.png)

Items traded on the in-game Currency Exchange (currency, omens, runes, essences, catalysts, soul
cores, fragments, uncut and lineage gems, plain waystones and the like) are priced from GGG's own
record of the trades made on the exchange instead of trade listings. GGG publishes it hour by
hour, a few minutes after each hour ends: the price is the average rate of the last complete hour's
trades, or of up to three hours for an item traded rarely. The hours downloaded are kept, so
without an internet connection the card shows the last ones saved. The market card shows:

- **≈ price** with the icon of a Divine Orb or an Exalted Orb, and under it the same value in the
  other core currencies, each with its icon;
- for cheap items, how many of them one Divine Orb buys;
- «За 7 дней» (last 7 days): the price change in percent over the last seven days on poe2scout,
  with a chart of daily prices; days without a price are gaps. «мало данных» means not enough data;
- «Оборот в час» (volume per hour), in Divine Orbs;
- «Чаще всего меняют» (most traded pair): the rate of the pair this item is traded in most;
- «Ваша стопка» (your stack): «N шт. ≈ …», what the whole stack you checked is worth;
- the category and «курс за 09:00–10:00» (rate for 09:00–10:00): the hours the price comes from,
  on your computer's clock, with the date when they are not today's; and a «poe2scout ↗» link to
  the item's page on poe2scout.

### When the exchange has no recent trades

Not every exchange item changes hands every hour: in a small league such as Standard many don't.
For an item without trades in the last hours:

- if poe2scout has a price, the panel says «На бирже в лиге … этот предмет за последние часы не
  меняли.» (this item has not been traded on the exchange in league … in the last hours), shows
  «Цена по poe2scout:» (poe2scout price) and offers the button «Лоты на площадке» (listings on the
  trade site). The trade site is searched only when you click it, since every search counts
  against its request limit;
- without a poe2scout price, the trade site's listings are searched right away, under the same
  note.

If GGG's exchange data cannot be reached and none is saved, the panel says «Данные биржи GGG
сейчас недоступны.» (GGG's exchange data is unavailable right now) and prices the item from
poe2scout the same way; if poe2scout cannot be reached either, it shows the trade site's listings.
«На площадке лотов нет» means the trade site has no listings of the item.

## Unique items

An identified unique is searched by its name. Above the results, «Цена по poe2scout:» (poe2scout
price) shows its price on poe2scout, with the currency's icon.

An unidentified unique is compared with unidentified uniques of the same base. It has no poe2scout
line, since its name is not known yet.

## Waystones

A waystone's modifiers start unselected: its tier and properties decide the price. You can mark
modifiers to spot them at a glance on every waystone you check:

- click the **◇** at the end of a modifier row. Each click moves the mark on: danger (red) →
  warning (orange) → wanted (green) → no mark;
- marked modifiers are listed under the filters: «Опасно:» (danger), «Осторожно:» (warning),
  «Желанно:» (wanted);
- with no marks yet, the panel reminds you: «◇ в конце свойства — пометить его опасным, спорным
  или желанным» (◇ at the end of a property marks it as dangerous, doubtful or wanted).

Marks are kept in your settings and apply to the same modifier on every waystone, in both client
languages. Marking a modifier does not add it to the search.

## Vendor gambles

A vendor's gamble offer ("Random Helmet" and the like) is not searched. The panel says «Это ставка
у торговца: какой предмет выпадет, станет известно только после покупки. На площадке такие не
продаются.» (this is a vendor gamble: which item you get is known only after buying; such items
are not sold on the trade site).

## Messages

| Message | Meaning |
|---|---|
| «Загрузка каталога…» | Loading the trade site's data. Happens on the very first start and takes a few seconds. |
| «Нет данных сайта торговли — нет интернета или сайт недоступен. Повторяю попытку сам.» | The trade site's data could not be downloaded: no internet, or the site is down. The app retries by itself. |
| «Наведите курсор на предмет и нажмите Ctrl+E» | Point at an item and press the hotkey. |
| «Поиск…» | Searching. |
| «Загрузка цен биржи…» | Loading the Currency Exchange's prices for an exchange item. |
| «Лимит запросов trade API — ждём N с…» | The trade site's request limit is close; the app waits N seconds and then searches. |
| «Сайт торговли временно ограничил поиск — повторите через N мин.» | The trade site has locked searches from your IP address for a while. Search again after that time; Currency Exchange prices keep working. See [Request limits](#request-limits). |
| «Нет связи с сайтом торговли — проверьте интернет и повторите.» | The trade site could not be reached. Check your connection and search again. |
| «Trade API отклонил запрос (HTTP …): …» | The trade site refused the search; its own message follows. |
| «Ошибка поиска: …» | Any other search error. |
| «Ничего не найдено» | No listings match. Buttons under it offer broader searches, one trade search each: «Широкий −10 %» and «Совпадение N из M». «Ничего не найдено — даже с N из M выбранных свойств» (nothing found, even with N of M selected properties) means that «Совпадение N из M» found nothing either. See [When nothing matches exactly](#when-nothing-matches-exactly). |
| «На бирже в лиге … этот предмет за последние часы не меняли.» | This exchange item has not been traded in your league in the last hours; poe2scout's price or the trade site's listings are shown instead. See [When the exchange has no recent trades](#when-the-exchange-has-no-recent-trades). |
| «Данные биржи GGG сейчас недоступны.» | GGG's exchange data could not be reached; poe2scout's price or the trade site's listings are shown instead. |
| «На площадке лотов нет» | The trade site has no listings of this exchange item. |
| «Игра не копирует предмет: сочетание Ctrl+Alt+C перехватывает другая программа…» | Another program holds the copy shortcut, so the game never copied the item. See [Troubleshooting](troubleshooting.md#the-hotkey-does-nothing). |
| «Не удалось разобрать предмет…» | The item text could not be read. The button «Сообщить разработчику» (tell the developer) under the message opens the item problem form with the text filled in. See [Troubleshooting](troubleshooting.md#an-item-is-not-recognised). |

### Request limits

The trade site limits how many requests one IP address may make in a given time, and it counts
every request from it: PoE2 Oracle's, the trade site open in your browser and any other trade tool
on the same connection. A price check costs one search and one request for the 10 cheapest
listings. PoE2 Oracle reads the limits from the site's answers:

- when the next search would break a limit and the wait is 15 seconds or less, the app waits,
  saying «Лимит запросов trade API — ждём N с…», and then searches by itself;
- when the trade site refuses a request, it locks your IP address out for a while, often for
  minutes. PoE2 Oracle then sends no trade request at all until the lockout ends and says when to
  try again: «Сайт торговли временно ограничил поиск — повторите через 9 мин 50 с.» Currency
  Exchange prices keep working meanwhile.

Repeating an identical search within two minutes does not send a new request.
