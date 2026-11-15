//! The guard post of `examples/contagion_scene.rs`: guards join an attack
//! and a flight they see, never one they could not see. The trace is the
//! golden text the C example is diffed against.

#[allow(dead_code)]
#[path = "../examples/contagion_scene.rs"]
mod contagion_scene;

use contagion_scene::*;
use sagax::perception::{Grid, sight_line};

/// Every line `cargo run -p sagax --example contagion_scene` prints.
const GOLDEN: &str = "\
square
t= 14 guard 1 attack 0/0 share 0 nearest - since -
t= 15 guard 2 sees 1 start attack
t= 15 guard 2 join_attack 1/1 share 256 nearest 4 since 0
t= 15 guard 3 sees 1 start attack
t= 15 guard 3 join_attack 1/1 share 256 nearest 4 since 0
t= 16 guard 2 sees 3 start attack
t= 16 guard 3 sees 2 start attack
t= 30 grenade at 5,1
t= 30 guard 2 flee 0/2 share 0 nearest - since -
t= 31 guard 3 sees 2 start flee
t= 31 guard 3 join_flight 1/2 share 128 nearest 6 since 0
t= 32 guard 2 sees 3 start flee
hex
t= 14 guard 1 attack 0/0 share 0 nearest - since -
t= 15 guard 2 sees 1 start attack
t= 15 guard 2 join_attack 1/1 share 256 nearest 6 since 0
t= 15 guard 3 sees 1 start attack
t= 15 guard 3 join_attack 1/1 share 256 nearest 3 since 0
t= 16 guard 2 sees 3 start attack
t= 18 guard 3 sees 2 start attack
t= 30 grenade at 5,1
t= 30 guard 2 flee 0/2 share 0 nearest - since -
t= 31 guard 3 sees 2 start flee
t= 31 guard 3 join_flight 1/2 share 128 nearest 6 since 0
t= 32 guard 2 sees 3 start flee
";

/// The acts of `guard`, in order.
fn acts(scene: &Scene, guard: u32) -> Vec<u32> {
    scene
        .events
        .iter()
        .filter_map(|&(_, e)| match e {
            Event::Act { guard: g, act } if g == guard => Some(act),
            _ => None,
        })
        .collect()
}

/// Whether the straight line from `a` to `b` is clear on `grid`.
fn clear(grid: Grid, a: sagax::Pos, b: sagax::Pos) -> bool {
    sight_line(&world(grid), &mut Post, a, b).clear
}

#[test]
fn guards_join_what_they_see() {
    for grid in [Grid::Square, Grid::Hex] {
        let scene = run_scene(grid);
        // The story: 1 sees the intruder, 2 and 3 join; the grenade makes
        // 2 flee, 3 follows; 1 faces the doorway and keeps firing; 4 behind
        // the inner wall never moves.
        assert_eq!(acts(&scene, 1), [ATTACK], "{grid:?}");
        assert_eq!(acts(&scene, 2), [JOIN_ATTACK, FLEE], "{grid:?}");
        assert_eq!(acts(&scene, 3), [JOIN_ATTACK, JOIN_FLIGHT], "{grid:?}");
        assert!(acts(&scene, 4).is_empty(), "{grid:?}");

        // Guards 2 and 3 never had the intruder in line of sight: they
        // joined on what they saw of guard 1. Guard 4 never had anyone.
        for (t, p) in scene.points.iter().enumerate() {
            for g in [1, 2] {
                assert!(!clear(grid, p[g], p[GUARDS]), "{grid:?} t={t} guard {g}");
            }
            for o in 0..GUARDS {
                if o != 3 {
                    assert!(!clear(grid, p[3], p[o]), "{grid:?} t={t} guard 4");
                }
            }
            assert!(!clear(grid, p[3], p[GUARDS]), "{grid:?} t={t} guard 4");
        }

        // No cheating: every onset is of a deed the ally really showed, in
        // a clear line from the observer; every join follows such an onset
        // of the same deed within the window.
        for &(t, e) in &scene.events {
            match e {
                Event::Onset { guard, ally, deed } => {
                    let (g, a) = (guard as usize - 1, ally as usize - 1);
                    let bit = [ATTACKING, FLEEING][usize::from(deed)];
                    assert_ne!(scene.cues[t as usize][a] & bit, 0, "{grid:?} {e:?}");
                    let p = &scene.points[t as usize];
                    assert!(clear(grid, p[g], p[a]), "{grid:?} t={t} {e:?}");
                }
                Event::Act { guard, act } if act == JOIN_ATTACK || act == JOIN_FLIGHT => {
                    let deed = if act == JOIN_ATTACK { 0 } else { 1 };
                    assert!(
                        scene.events.iter().any(|&(s, o)| s <= t
                            && t - s <= WINDOW
                            && matches!(o, Event::Onset { guard: g, deed: d, .. }
                                if g == guard && d == deed)),
                        "{grid:?} t={t} {e:?}"
                    );
                }
                _ => {}
            }
        }
        // Guard 1 never saw anyone flee.
        assert!(!scene.events.iter().any(|(_, e)| matches!(
            e,
            Event::Onset {
                guard: 1,
                deed: 1,
                ..
            }
        )));
    }
}

#[test]
fn golden_trace() {
    let mut text = String::new();
    for grid in [Grid::Square, Grid::Hex] {
        for line in run_scene(grid).lines {
            text.push_str(&line);
            text.push('\n');
        }
    }
    assert_eq!(text, GOLDEN);
}
