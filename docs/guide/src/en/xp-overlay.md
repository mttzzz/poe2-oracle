# XP overlay

![The XP overlay's lines above the game's flask and skill panels](../images/en/xp-overlay.webp)

PoE2 Oracle sets its experience readout on top of the game's own HUD. On the rail along the top
of the flask panel, left of the experience bar, stands a plate that tells how fast you gain
experience and when the next level comes, with a ⚙ at its end that opens the
[settings](settings.md). With the map timer on, a plate on the skill panel's rail shows the current
map. The plates stand on the rails, never over them -- the game fills the rails with its rage and
stun gauges -- and are drawn pixel by pixel in the HUD's own molding and colours. At its outer end
each plate runs on to the frame of its globe, life or mana, filling the gap there to the pixel; at
its inner end it curls down onto the tip of the game's scrollwork in one smooth curve. The overlay
is on by default; the [settings](settings.md#xp-overlay) section **XP overlay** turns it and its
parts on and off.

Above the flask panel:

```text
64.8% ◆ +12.4%/h · level 75 in 2h 50m
```

Above the skill panel:

```text
map 4:07 +1.2% · avg 6:30
```

| Part | Meaning |
|---|---|
| `64.8%` | How much of the current level is done. Shown with **Level percentage**, off by default, and always in a pause. |
| `+12.4%/h` | Experience per hour (`h`) of play, in percent of the current level. |
| `level 75 in 2h 50m` | Playing time to level 75 at this rate: 2 hours 50 minutes. `next level in` while the app can't tell which of your characters you are playing, and so your level (see [How it works](#how-it-works)); `—` when there is no estimate. |
| `map 4:07 +1.2%` | Time in the current map and the experience it gave. Shown with **Map timer**, on by default. Dimmed once you leave the map; five minutes later it reads `last map`. |
| `avg 6:30` | Average time of the maps finished this session. |

Each line says as much as its plate has room for. When the whole wording doesn't fit, the flask
panel's line drops the level (`64.8% ◆ +12.4%/h · 2h 50m`) and then the percentage
(`+12.4%/h · 2h 50m`); the map line drops the average and `last`. For the first couple of minutes
the line reads `measuring rate…`. Times use `m` for minutes, `h` for hours and `d` for days. With
the Russian [interface language](settings.md#interface-language), the lines are in Russian:
`64,8 % ◆ +12,4 %/ч · до 75 ур. 2 ч 50 мин`.

In a town or hideout, and after five minutes of play without experience or a change of area, the
line dims and says only how much of the level is done, with **Level percentage** off too: the rate
and the time to level would still be those of the play before.

```text
64.8%
```

The map line drops the average. In a town or hideout it keeps the map you left, dimmed; in an idle
pause inside a map it stays lit, and the map's time keeps running. The pause does not change the
rate: it is back as it was when you play again.

## How it works

- Every two seconds PoE2 Oracle reads how full the experience bar is, straight from the screen, and
  reads the game's log (`Client.txt`) for level-ups, area changes and returns to character
  selection. While the game is behind another window, where no experience comes in, it looks at the
  bar only every ten seconds, and two seconds after a look that finds it changed or can't read it.
  Started in the middle of a session, it reads back through the log with the time of each line:
  your character's level (if it levelled up in that stretch), the map you are in or left, with its
  time so far, and how long you have been in town or the hideout. Only a map already under way when
  the log's last stretch it reads begins is left out, since its start isn't there. After a restart
  for an update it doesn't start over: the rate, the time to the next level and the map timer go on
  as they were.
- The level comes from a level-up line in the game's log and, until the log names it, from where
  the bar stands. PoE2 Oracle keeps on your computer the level of your last 20 characters and where
  each one's bar stood when you last played it (see
  [Privacy](privacy.md#what-it-keeps-on-your-computer)); the first time, it reads the levels back
  from the end of the log. After a restart or a login it compares the first bar reading it accepts
  with those positions: if exactly one character's bar stood within 0.3% of it, that is you, and
  the plate gives the level again instead of `next level in`. The first time, before any position
  is known, it takes your most recent character; if you play another, your next level-up corrects
  that. If none fits, or more than one, the plate says `next level in` until your next level-up.
- The rate weighs recent play more. With **Rate smoothing** at 10 minutes, the default, play from
  10 minutes ago counts half as much as play now. Choose 5 minutes to see a change of farming
  sooner, or 20 or 30 for a steadier number.
- Time in towns and hideouts does not count towards the rate. A level-up does not reset it, and
  neither does a death: the death costs experience, the rate stays.
- The map timer pauses, dimmed, when you leave the map, and continues when you go back into the
  same map through its portal, even once it reads `last map`. A map counts as finished when you
  enter a different map, whether or not you completed it. Returning to character selection starts
  everything over, since you may come back as another character: the rate (`measuring rate…`
  again), the level (found again from where the bar stands, as after a restart) and the map
  statistics.
- Ascendancy trials (the Trial of the Sekhemas and the Trial of Chaos) are never part of a map, but
  count as play for the rate.
- If something covers the bar for more than a few seconds (a loading screen, the passive tree, the
  price panel), that time counts only when you earned experience behind the cover, as in a fight
  with the price panel open, and the bar was back within a minute; otherwise neither that time nor
  the experience earned in it counts. The plates themselves follow their rails, not the bar: a
  loading screen or the passive tree takes them down with the HUD, and the price panel hides only
  a plate it stands over (see [Requirements](#requirements)).

## Requirements

- The game window must be at least 720 pixels tall and not minimised.
- The game must run in Windowed or Windowed Fullscreen mode, like everything PoE2 Oracle draws over
  it.
- The plates are the size of the game's HUD at the game's resolution; the **Interface scale**
  setting does not change them. They let clicks through to the game, all but the ⚙, and the price
  panel hides one only while the panel stands over it: dragged there, or wide enough to reach it by
  itself (a 4:3 or 5:4 game window, a large **Interface scale**). While the game is in front, a
  plate steps aside the moment a tooltip, the chat or another part of the game's interface covers
  its rail, and is back the moment it goes. What the game puts over a rail while you leave the
  mouse and keyboard alone, such as a loading screen that comes a while after a click, takes a
  plate down within two seconds. Behind another window the game still shows a tooltip under the
  pointer; a plate then steps aside within four seconds, and is back within two.
