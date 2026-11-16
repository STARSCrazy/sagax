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
  debugging pattern for every module), perception (0.13.0: view cones,
  exact sight lines, attention, disguise, hearing; Rust and C), indirect
  coordination (0.14.0: `Contagion`, a character joins in on the deeds it
  saw its allies do; Rust and C), goal recognition (0.15.0: `GoalGuess`,
  where the one an observer watches is heading, from the steps it saw;
  Rust and C) and walking distance for the squad (0.16.0: `Distance`,
  `Walking`, `FlowDistance`; Rust and C).
- **Walking distance, next:** a **JavaScript binding** follows on demand.
  Channel delays stay straight (sound and radio do not walk); perception,
  mission and contagion do not walk yet.
- **Missions, next:** a *search planner* (GOAP or HTN) behind the same plan
  format: it would build the plans that the game now writes by hand or
  scores from a playbook, and a mission would run them unchanged. A
  **JavaScript binding** of the mission follows on demand. Perception
  (0.13.0) is the natural knowledge source for the characters' `Knowledge`
  (`learn_percepts`).
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
- **Perception, next:** a **JavaScript binding** of perception and of
  `Contagion` follows on demand. Indirect coordination is done (0.14.0:
  characters see allies attack or flee and join in, only on their own
  percepts); joining in on what was *heard* is left to the existing shout
  and radio channels.

- **Goal recognition, next:** a **JavaScript binding** of `GoalGuess`
  follows on demand. Not wired into the squad, crew or mission: a game
  feeds `split` or the columns into its own orders. Open ideas: a group
  that shares one guess (only what its members saw and told), a guess
  that feeds the mission planner, and learning `beta` per player.

Known limits: flow fields and D* Lite assume moves that work both ways.
One-way moves (zones with a leave cost, one-sided walls) make their costs
approximate.

Tried and dropped: a dense node index instead of the hash table. It gained
about 3 % (within noise) for an extra setting.
