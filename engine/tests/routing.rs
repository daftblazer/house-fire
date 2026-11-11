//! The routing pieces against figures read from the home log (`docs/simulation.md`, "Routing").
//! None needs the disc.

use housefire::library::Slot;
use housefire::routing::{
    routing_return_tick, run_on_point, slot_goal_score, slot_pick, turn_frames, walk_frames, walk_rates, walk_step, walk_top, FrameClock, WALK_TOP_ADULT,
    WALK_TOP_CHILD,
};

#[test]
fn run_on_points_of_the_home_log() {
    // tick 1645: from the walk's end (376, 336) towards the plan's last (360, 328), 3 a side
    assert_eq!(run_on_point((376.0, 336.0), (360.0, 328.0), false), (373.0, 333.0));
    // tick 496: straight on, 3 along
    assert_eq!(run_on_point((296.0, 232.0), (296.0, 248.0), false), (296.0, 235.0));
    // with the follower's +132 at 1 the reach is 9
    assert_eq!(run_on_point((296.0, 232.0), (296.0, 248.0), true), (296.0, 241.0));
}

#[test]
fn the_brake_is_the_root_times_1_001() {
    // tick 1646: carried at the adult's top, 3.588 from its point: the logged step 0.5574
    let (top, growth) = walk_rates(WALK_TOP_ADULT);
    let (v, step) = walk_step(top, top, growth, 1.0, 3.588);
    assert!((v - 0.5574).abs() < 0.0005, "{v}");
    assert_eq!(v, step);
    // from a stop, far from the end: one frame's growth
    let (v, _) = walk_step(0.0, top, growth, 1.0, 100.0);
    assert!((v - growth).abs() < 1e-6);
    // the fast mode: the top times the factor, from the first frame
    let (v, step) = walk_step(0.0, top, growth, 5.0, 100.0);
    assert_eq!(v, top);
    assert!((step - 5.0 * top).abs() < 1e-5);
}

#[test]
fn a_walk_that_runs_on_does_not_brake() {
    let (top, growth) = walk_rates(WALK_TOP_ADULT);
    let (_, v_braking) = walk_frames(30.0, top, top, growth, false, &[]);
    let (_, v_running) = walk_frames(30.0, top, top, growth, true, &[]);
    assert!(v_braking < top);
    assert_eq!(v_running, top);
    assert_eq!(walk_frames(0.0, 0.0, top, growth, false, &[]).0, 0);
}

#[test]
fn turns_and_tops() {
    // 3 degrees a frame and one: 90 degrees in 31 frames, 180 in 61 (the home log's)
    assert_eq!(turn_frames(std::f32::consts::FRAC_PI_2), 31);
    assert_eq!(turn_frames(std::f32::consts::PI), 61);
    assert_eq!(turn_frames(0.0), 1);
    assert_eq!(walk_top(Some("child")), WALK_TOP_CHILD);
    assert_eq!(walk_top(Some("adult")), WALK_TOP_ADULT);
    assert_eq!(walk_top(None), WALK_TOP_ADULT);
}

#[test]
fn the_frame_clock_holds_14_frames_in_11_ticks() {
    let mut clock = FrameClock { time: 0.0 };
    let frames: u32 = (0..1100).map(|_| clock.frames_to_tick()).sum();
    assert!((1399..=1401).contains(&frames), "{frames}");
    // the fast mode (5): a frame every 3 or 4 ticks
    let mut clock = FrameClock { time: 0.0 };
    let per: Vec<u32> = (0..55).map(|_| clock.frames_to_tick_at(5.0)).collect();
    assert!(per.iter().all(|&n| n <= 1));
    let total: u32 = per.iter().sum();
    assert!((13..=15).contains(&total), "{total}");
}

#[test]
fn the_pick_probes_on_without_wrapping() {
    // 7 % 5 = 2; 7 again finds 2 taken and takes 3; 12 % 5 = 2, then 3, then 4
    assert_eq!(slot_pick(5, &[7, 7, 12], 3), vec![2, 3, 4]);
    // past the last place a draw takes nothing
    assert_eq!(slot_pick(3, &[2, 2], 2), vec![2]);
    // the cap
    assert_eq!(slot_pick(5, &[0, 1, 2, 3], 2), vec![0, 1]);
    assert!(slot_pick(0, &[1], 1).is_empty());
}

#[test]
fn goal_scores() {
    let plain = Slot::default();
    let flagged = Slot { flags: 0x2000, ..Slot::default() };
    assert_eq!(slot_goal_score(&plain, false), 135..=135);
    assert_eq!(slot_goal_score(&flagged, false), 125..=144);
    assert_eq!(slot_goal_score(&plain, true), 225..=225);
}

#[test]
fn a_routing_primitive_returns_the_tick_after_its_walkers_frame_after_done() {
    // frames' ticks from the walk's start; 3 walking frames and 1 turning: the frame after is
    // the fifth (index 4), at tick 12, so the return at 13
    let ticks = [10, 10, 11, 12, 12, 13];
    assert_eq!(routing_return_tick(&ticks, 3, 1), Some(13));
    assert_eq!(routing_return_tick(&ticks, 9, 1), None);
}

#[test]
fn the_planners_steps_and_a_way_in_one_cell() {
    use housefire::planner::{coarse_way, step_weight, Growth, MapRect, BLOCKED};
    // the dump's weights from a cost-0 cell: 1, 2 and 6 into costs 0, 3 and 10
    assert_eq!([step_weight(0, 0), step_weight(0, 3), step_weight(0, 10)], [1.0, 2.0, 6.0]);
    assert_eq!(step_weight(3, 3), 4.0);
    // a lot walled round: start and goal share one cell, so the way is straight
    let border = [
        MapRect { y0: -7, x0: -7, y1: 23, x1: 631, cost: BLOCKED },
        MapRect { y0: 601, x0: -7, y1: 631, x1: 631, cost: BLOCKED },
        MapRect { y0: -7, x0: -7, y1: 631, x1: 23, cost: BLOCKED },
        MapRect { y0: -7, x0: 601, y1: 631, x1: 631, cost: BLOCKED },
    ];
    let growth = Growth { cap: None, open_cost: 3 };
    assert_eq!(coarse_way(&border, growth, (100, 100), (300, 120)).map(|w| w.points), Some(vec![(100, 100), (300, 120)]));
    // a start inside a blocked rect has no cell, so no way
    assert!(coarse_way(&border, growth, (0, 0), (300, 120)).is_none());
}
