# Roadmap

- **HPA\*** (hierarchical search): cut the map into blocks, search coarsely
  over blocks, then finely. Pays off only for very large maps or very many
  searches per frame.
- **Theta\***: smooths during the search and finds slightly shorter
  any-angle ways than post-smoothing. Worth it only if post-smoothing shows
  visibly poor ways.
- **Faster D\* Lite**: after a change some neighbours are asked twice. A
  per-edge cost query would avoid that.
- **Decisions, next steps:** memory of the world (0.7.0), group tactics
  (0.8.0: squad leader, orders, signal, skill), the 0.9.0 API round (one
  set of conventions, the `Crew`, the `sagax` crate with its prelude) and the
  0.10.0 slim round (a core with a kit, the rules object for a game's own
  pieces) are done, and so are the mission planner (0.11.0: motive, plan,
  leeway, detours, replanning; Rust and C), the trace round (0.12.0: one
  debugging pattern for every module) and perception (0.13.0: view cones,
  exact sight lines, attention, disguise, hearing; Rust and C).
- **Missions, next:** a *search planner* (GOAP or HTN) behind the same plan
  format: it would build the plans that the game now writes by hand or
  scores from a playbook, and a mission would run them unchanged. A
  **JavaScript binding** of the mission follows on demand. Perception
  (0.13.0) is the natural knowledge source for the characters' `Knowledge`
  (`learn_percepts`).
- **Walking distance for the squad:** the squad measures distances in
  a straight-line metric (Euclid, Manhattan, Chebyshev or hex). Using the
  real walking distance instead (for `IN_TARGET_DIST`, the keeper's
  distance to its goal and the channel delays) was designed in the 0.9.0
  round and put off, because four questions need an answer first: the
  *shape* (a callback `Squad::set_distance(fn)`, C function pointer, JS
  function, or a table the game fills), the *units* (path cost or cells),
  what to do when the target is *unreachable* (infinite, or the partial
  way's end), and the *cost* (a search per member per tick is too much; who
  caches). Until then the game can feed its own distances through the
  `extra` columns of the scoring row.
- **Hex distance in offset coordinates:** a `Metric::HexOffset(layout)` that
  measures directly in the map's offset columns and rows was dropped. Offset
  coordinates cannot be averaged or turned, and a distance from deltas needs
  the row's parity, so every block that does arithmetic on positions would
  have to know the layout. The AI blocks stay axial; the `Crew` converts
  (`CrewConfig::hex_layout`) and everything else converts at the edge with
  `to_axial` / `from_axial` (see "Hex" in the guide).
- **Group tactics, deferred from the 0.8.0 design:** a skill-driven chance
  that a member ignores its slot. Not built yet. (The trace sink of the
  same design came in 0.12.0: `SquadTrace`, `CrewTrace`.)
- **Perception, next: indirect coordination**, where characters see allies
  attack or flee and join in, only when they actually saw it. Perception
  itself is done (0.13.0, `sagax-perception` on `sagax-core`; the squad
  stays blind and its slots ask `sight_line` / `in_sight`). An ally's
  `Cues { new }` percept with an *attacking* bit is a percept like any
  other, so a rule "join an ally I saw attack" reads only percepts and is
  honest by construction. A **JavaScript binding** of perception follows
  on demand.

Known limits: flow fields and D* Lite assume moves that work both ways.
One-way moves (zones with a leave cost, one-sided walls) make their costs
approximate.

Tried and dropped: a dense node index instead of the hash table. It gained
about 3 % (within noise) for an extra setting.
