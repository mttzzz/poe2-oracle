# Trade requests

![A trade request card at the top of the game window](../images/trade-request.png)

When a buyer sends you the trade site's purchase whisper, PoE2 Oracle shows a card at the top of
the game window: who wants what, for how much and where the item lies, with buttons that answer in
the game's chat. The feature is on by default; turn it off with «Запросы покупателей» (buyer
requests) in the [settings](settings.md#trading).

## Which whispers make a card

PoE2 Oracle follows the game's chat log, `Client.txt` in the `logs` folder of the running game.
A card appears for the message the trade site writes for a buyer, in English or in Russian,
whatever language your own client uses:

```text
@From Buyer: Hi, I would like to buy your … listed for 5 exalted in … (stash tab "3"; position: left 1, top 1)
@От кого Buyer: Здравствуйте, хочу купить у вас … за 5 exalted в лиге … (секция "3"; позиция: 1 столбец, 1 ряд)
```

Other whispers are ignored. Only messages that arrive while PoE2 Oracle runs make cards; old
whispers from the log never come back.

## The card

- **✉ buyer's name** and the time of their last message. «пишет 2-й раз» (writing for the 2nd
  time) and so on appears when the same buyer asks about the same item again; the card moves to
  the top instead of doubling. «пришёл» (arrived), in green with a green frame, appears when the
  buyer joins your party (on an English client, also when they enter your area) and goes away
  when they leave.
- **The item**, as the buyer's trade site named it, and **the price** with the currency's icon.
- **Where it lies**: «вкладка «3» · 1 столбец, 1 ряд» (tab "3", column 1, row 1, counted from the
  tab's top-left corner). Without a stash position the card shows the league instead: «лига …».
  If the buyer added their own words after the standard message, they follow as «· «…»» and the
  line turns orange.
- **Buttons** to answer.

## Buttons

Each click sends exactly one message to the game's chat.

| Button | Sends | The card |
|---|---|---|
| «Пригласить» (invite) | `/invite buyer` | stays |
| «Обмен» (trade) | `/tradewith buyer` | stays |
| «Минуту» (one moment) | `@buyer one moment please` | stays |
| «Продано» (sold) | `@buyer sorry, it's already sold` | closes |
| «Спасибо» (thanks) | `@buyer thanks, good luck!` | stays |
| «Выгнать» (kick) | `/kick buyer` | closes |

The three polite messages go in the language of the buyer's request: a Russian request gets
«минуту, пожалуйста», «извините, уже продано» and «спасибо, удачи!», an English one the English
texts above, whatever your client's language.

«Найти» (find) pastes the item's name into the search box of the open stash, so the game
highlights where the item lies. Open the stash first. The card stays.

**×** closes a card without sending anything.

### How the app types into the game

For a message, PoE2 Oracle presses <kbd>Enter</kbd> to open the chat, pastes the text with
<kbd>Ctrl</kbd>+<kbd>V</kbd> and presses <kbd>Enter</kbd>. For «Найти» it presses
<kbd>Ctrl</kbd>+<kbd>F</kbd>, pastes and presses <kbd>Enter</kbd>. Your clipboard is put back right
after. It types only while the game is the window in front, and a click on a card does not take
the keyboard away from the game. If the game is not in front, nothing is typed and a «Продано» or
«Выгнать» card stays open.

## Cards on the screen

- At most four cards at a time; a fifth request pushes the oldest card out.
- The cards sit at the top centre of the game window, below the boss health bar. They hide while
  the price panel is open or another program is in front, and come back afterwards.
- A card stays until you close it with **×**, answer with «Продано» or «Выгнать», or turn the
  feature off. Cards are not kept when PoE2 Oracle quits.
- With «Звук при новом запросе» (sound on a new request) on, each new card plays the Windows
  "Asterisk" sound. A repeated request does not.
- The cards follow the «Масштаб интерфейса» (interface scale) setting.
