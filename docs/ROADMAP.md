# Roadmap

- **HPA\*** (hierarchical search): cut the map into blocks, search coarsely
  over blocks, then finely. Pays off only for very large maps or very many
  searches per frame.
- **Theta\***: smooths during the search and finds slightly shorter
  any-angle ways than post-smoothing. Worth it only if post-smoothing shows
  visibly poor ways.
- **Faster D\* Lite**: after a change some neighbours are asked twice. A
  per-edge cost query would avoid that.
- **Decisions, next steps:** memory of the world is done (0.7.0, generic
  blocks for turn-based, real-time, stealth and action games). Next: a group
  block (rally point, calls to the nearest, shared orders), then a mission /
  intention planner.
- **Perception** (sight lines, cover, flanks), on the same `sagax-core`.

Known limits: flow fields and D* Lite assume moves that work both ways.
One-way moves (zones with a leave cost, one-sided walls) make their costs
approximate.

Tried and dropped: a dense node index instead of the hash table. It gained
about 3 % (within noise) for an extra setting.
