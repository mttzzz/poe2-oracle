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

The panel's top bar shows the league being searched, «PoE2 Oracle · *league*», and, once poe.ninja
prices have loaded, how many Exalted Orbs a Divine Orb is worth, drawn with the two currency icons.
The gear **⚙** opens the [settings](settings.md); **×** closes the panel.

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
| «Суммарные (псевдо)» (totals, pseudo) | Sums across modifiers: total life, total elemental resistance, all attributes and so on |

Each row has a checkbox (whether the row takes part in the search), the modifier text with your
roll highlighted in blue, a tier badge and **min**/**max** boxes. The tier badge is gold-filled for
T1, gold-outlined for T2 and grey for lower tiers. Small labels under a row say where a modifier
comes from: «усилитель» (from a rune or other augment), «мастер» (crafted), «расколотый»
(fractured), «зачарование» (enchantment), «очернённый» (desecrated). A line the trade site cannot
search says «не участвует в поиске» (not part of the search).

On a magic or rare item that can still take modifiers, rows «Свободный префикс» / «Свободных
префиксов: N» and «Свободный суффикс» / «Свободных суффиксов: N» (open prefix/suffix slots) can be
searched too.

Some rows are folded away: unselected totals, and minor properties such as a weapon's physical or
elemental DPS when it is only a small part of the total. «▾ Суммарные и второстепенные: ещё N»
(totals and minor rows: N more) unfolds them; «▴ Свернуть …» folds them again.

### What is selected at first

- Prefixes and suffixes of tier T1 or T2 are selected; lower tiers are not. On a magic item every
  modifier is selected whatever its tier: its prefix and suffix are the whole item.
- If no modifier qualifies, the totals are selected instead, so the search is never empty.
- Defences and total or elemental DPS are selected; attack speed, critical hit chance and reload
  time are not.
- On a waystone, the modifiers start unselected: its tier and properties set the price, its
  modifiers only make the map harder. Select a modifier yourself to search for it.

### Min and max

- Only **min** is filled in at first: your roll minus the value tolerance, 10% by default (setting
  «Допуск значений», see [Settings](settings.md#search)). **max** stays empty, since a
  higher roll is never a reason to leave a listing out.
- For the few modifiers where a lower number is better, **max** is filled in instead.
- Type digits, a point or a comma (both give a decimal point) and a minus sign. The first key after
  you click into a box replaces its value.
- <kbd>Enter</kbd> in a box runs the search again with the new bounds.

Ticking or unticking rows and switching the class, rarity or corrupted chips take effect with the
next search: press <kbd>Enter</kbd> in a box or click «Поиск» (Search).

## Search options

Above the results there are two more choices. Each click moves to the next value and searches
again at once.

- «Продавцы:» (sellers): «выкуп и онлайн» (instant buyout and sellers online, the default) →
  «только мгновенный выкуп» (instant buyout only) → «только онлайн» (only sellers online, to trade
  in person) → «все, включая офлайн» (everyone, offline sellers too). The starting value comes from
  the [settings](settings.md#search).
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

### Estimate

The «Оценочная стоимость» (estimated value) card shows **≈ price** with the currency's icon (a
Divine Orb or an Exalted Orb, whichever reads better), «Диапазон:» (range) and «Надёжность:»
(confidence): «высокая» (high), «средняя» (medium) or «низкая» (low). The estimate needs
poe.ninja's exchange rates and appears once they have loaded.

When most listings are priced in unusual currencies, the card says «Цены в основном в редкой
валюте: учтены только хаос, возвышения и божественные» (prices are mostly in rare currency; only
chaos, exalted and divine orbs are counted) and offers «Искать только цены в … или …» (search only
prices in exalted or divine orbs, shown by their icons).

### When nothing matches exactly

If no listing has every selected property, PoE2 Oracle searches again for listings that have most
of them and says «Точных совпадений нет — показаны предметы хотя бы с N из M выбранных свойств»
(no exact matches, showing items with at least N of M selected properties). Each row then shows
how many of the selected properties it has, for example `3/4`, and the estimate's confidence is
low. This happens only with at least three selected modifiers; selected item properties such as
defences always stay in the search.

If even that finds nothing, the panel says «Ничего не найдено» (nothing found). Untick some rows,
lower some **min** values, search by class instead of base, or let offline sellers in.

## Currency and exchange items

![The market card for a currency item](../images/market.png)

Items traded on the in-game Currency Exchange (currency, omens, runes, essences, catalysts, soul
cores, fragments, uncut and lineage gems, plain waystones and the like) are priced from
poe.ninja's market data instead of trade listings. The market card shows:

- **≈ price** with the icon of a Divine Orb or an Exalted Orb, and under it the same value in the
  other core currencies, each with its icon;
- for cheap items, how many of them one Divine Orb buys;
- «За 7 дней» (last 7 days): the price change in percent, with a chart of daily prices; days
  without trades are gaps. «мало данных» means not enough data;
- «Оборот в час» (volume per hour), in Divine Orbs;
- «Чаще всего меняют» (most traded pair): the rate of the pair this item is traded in most;
- «Ваша стопка» (your stack): «N шт. ≈ …», what the whole stack you checked is worth;
- the category and «валютная биржа, обновление раз в час» (currency exchange, updated hourly), with
  a «poe.ninja ↗» link to the item's page.

### When poe.ninja has no price

poe.ninja does not track every exchange item in every league: in a small league such as Standard
it lists far fewer. For such an item:

- if poe2scout has a price, the panel says «poe.ninja не отслеживает этот предмет в лиге …»
  (poe.ninja does not track this item in league …), shows «Цена по poe2scout:» (poe2scout price)
  and offers the button «Лоты на площадке» (listings on the trade site). The trade site is
  searched only when you click it, since every search counts against its request limit;
- without a poe2scout price, the trade site's listings are searched right away, under the same
  note.

If poe.ninja cannot be reached at all, the panel says «poe.ninja сейчас недоступен.» (poe.ninja is
unavailable right now) and shows the trade site's listings instead. «На площадке лотов нет» means
the trade site has no listings of the item.

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
| «Загрузка цен poe.ninja…» | Loading poe.ninja prices for an exchange item. |
| «Лимит запросов trade API — ждём N с…» | The trade site's request limit is close; the app waits N seconds and then searches. |
| «Сайт торговли временно ограничил поиск — повторите через N мин.» | The trade site has locked searches from your IP address for a while. Search again after that time; poe.ninja prices keep working. See [Request limits](#request-limits). |
| «Нет связи с сайтом торговли — проверьте интернет и повторите.» | The trade site could not be reached. Check your connection and search again. |
| «Trade API отклонил запрос (HTTP …): …» | The trade site refused the search; its own message follows. |
| «Ошибка поиска: …» | Any other search error. |
| «Ничего не найдено» | No listings match. |
| «poe.ninja не отслеживает этот предмет в лиге …» | poe.ninja has no price for this exchange item in your league. See [When poe.ninja has no price](#when-poeninja-has-no-price). |
| «poe.ninja сейчас недоступен.» | poe.ninja could not be reached; the trade site's listings are shown instead. |
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
  try again: «Сайт торговли временно ограничил поиск — повторите через 9 мин 50 с.» Prices from
  poe.ninja keep working meanwhile.

Repeating an identical search within two minutes does not send a new request.
