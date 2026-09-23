# XP overlay

![The XP overlay above the experience bar](../images/xp-overlay.png)

A single line just above the game's experience bar tells how fast you gain experience and when the
next level comes. It is on by default; the [settings](settings.md#xp-overlay) section «Оверлей
опыта» (XP overlay) turns it and its parts on and off.

A full line looks like this:

```text
64,8 % · +12,4 %/ч · до 75 ур. 1 ч 32 мин игры · карта 4:07 +1,2 % · ср. 6:30
```

| Part | Meaning |
|---|---|
| `64,8 %` | How much of the current level is done. Shown with «Процент уровня» (level percent), off by default. |
| `+12,4 %/ч` | Experience per hour («ч») of play, in percent of the current level. |
| `до 75 ур. 1 ч 32 мин игры` | Playing time («игры», of play) to level 75 at this rate: 1 hour 32 minutes. «до ур.» (to the level) when your level is not known yet; «—» when there is no estimate. |
| `карта 4:07 +1,2 %` | Time in the current map and the experience it gave. Shown with «Таймер карты» (map timer), on by default. Dimmed once you leave the map; five minutes later it reads «последняя карта» (last map). |
| `ср. 6:30` | Average time of the maps finished this session. |

For the first couple of minutes the line reads «замер скорости…» (measuring the rate). Times use
«мин» for minutes, «ч» for hours and «д» for days.

In a town or hideout, and after five minutes of play without experience, the line dims and shows
a pause instead of the rate and the time to level, which would still be those of the play before:

```text
64,8 % · пауза · 12 мин · последняя карта 9:00 +3,66 %
```

`пауза · 12 мин` is how long the pause has lasted («пауза», pause); with the map timer, the map you
left follows, without the average. The pause does not change the rate: it is back as it was when
you play again.

## How it works

- Every two seconds PoE2 Oracle reads how full the experience bar is, straight from the screen, and
  reads the game's log (`Client.txt`) for level-ups, area changes and returns to character
  selection. Started in the middle of a session, it finds your character's level in the log.
- The rate weighs recent play more. With «Сглаживание скорости» (rate smoothing) at 10 minutes, the
  default, play from 10 minutes ago counts half as much as play now. Choose 5 minutes to see a
  change of farming sooner, or 20 or 30 for a steadier number.
- Time in towns and hideouts does not count towards the rate. A level-up does not reset it, and
  neither does a death: the death costs experience, the rate stays.
- The map timer pauses, dimmed, when you leave the map, and continues when you go back into the
  same map through its portal, even once it reads «последняя карта». A map counts as finished when
  you enter a different map, whether or not you completed it. Returning to character selection
  starts the map statistics over.
- Ascendancy trials (the Trial of the Sekhemas and the Trial of Chaos) are never part of a map, but
  count as play for the rate.
- When something covers the bar (a game panel, a loading screen, the price panel), that time does
  not count; after five seconds the line hides until the bar is visible again.

## Requirements

- The game window must be at least 720 pixels tall and not minimised.
- The game must run in Windowed or Windowed Fullscreen mode, like everything PoE2 Oracle draws over
  it.
- The line hides while the price panel is open, and follows the «Масштаб интерфейса» (interface
  scale) setting.
