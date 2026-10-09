# Vendored animation engine (bloub)

This directory is the framework-free bot engine from
[bloub](https://github.com/jeremy-prt/bloub) (MIT, Copyright 2026 Jérémy Perret),
internalized for Flowy's desktop companion.

Keep it clock-free and framework-free: `engine.sample(t)` is a pure function of
time. Numeric constants are measurements from the reference animation — do not
round or "simplify" them.

The React wrapper and white-pet presentation live outside this folder
(`characters/Puff.tsx`). Do not import Vue, `Date.now()`, or Flowy UI code here.
