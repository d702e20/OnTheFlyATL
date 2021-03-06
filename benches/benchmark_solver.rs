use atl_checker::algorithms::certain_zero::distributed_certain_zero;
use atl_checker::algorithms::certain_zero::search_strategy::bfs::BreadthFirstSearchBuilder;
use atl_checker::atl::Phi;
use atl_checker::edg::atledg::{vertex::AtlVertex, AtlDependencyGraph};
use atl_checker::game_structure::lcgs::ir::intermediate::IntermediateLcgs;
use atl_checker::game_structure::lcgs::parse::parse_lcgs;
use atl_checker::game_structure::EagerGameStructure;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::Arc;

const PRIORITISE_BACK_PROPAGATION: bool = true; // change this for benches with no-backprop

// CWD is atl-checker, use relative paths - implemented as macro, since concat! only works for tokens
// workaround src: https://github.com/rust-lang/rust/issues/31383
macro_rules! lcgs_model_path_prefix {
    () => {
        "../lcgs-examples/"
    };
}

macro_rules! json_model_path_prefix {
    () => {
        "../json-examples/"
    };
}

/// Benchmark solver given json-model and -formula
macro_rules! bench_json {
    ($name:ident, $model:expr, $formula:expr) => {
        fn $name(c: &mut Criterion) {
            // Write header for stats if enabled
            #[cfg(feature = "use-counts")]
            eprintln!(concat!("[stats] bench_run_start: ", stringify!($name)));
            c.bench_function(stringify!($name), |b| {
                b.iter(|| {
                    let game_structure: EagerGameStructure = serde_json::from_str(include_str!(
                        concat!(json_model_path_prefix!(), $model)
                    ))
                    .unwrap();
                    let graph = AtlDependencyGraph { game_structure };

                    let formula: Arc<Phi> = serde_json::from_str(include_str!(concat!(
                        json_model_path_prefix!(),
                        $formula
                    )))
                    .unwrap();

                    let v0 = AtlVertex::Full { state: 0, formula };

                    distributed_certain_zero(
                        graph,
                        v0,
                        num_cpus::get() as u64,
                        BreadthFirstSearchBuilder,
                        PRIORITISE_BACK_PROPAGATION,
                        false,
                    );
                })
            });
        }
    };
}

macro_rules! bench_lcgs {
    ($name:ident, $model:expr, $formula:expr) => {
        fn $name(c: &mut Criterion) {
            // Write header for stats if enabled
            #[cfg(feature = "use-counts")]
            eprintln!(concat!("[stats] bench_run_start: ", stringify!($name)));
            c.bench_function(stringify!($name), |b| {
                b.iter(|| {
                    let lcgs = parse_lcgs(include_str!(concat!(lcgs_model_path_prefix!(), $model)))
                        .expect(&format!("Could not read model {}", $model));
                    let game_structure =
                        IntermediateLcgs::create(lcgs).expect("Could not symbolcheck");
                    let graph = AtlDependencyGraph { game_structure };

                    let formula = serde_json::from_str(include_str!(concat!(
                        lcgs_model_path_prefix!(),
                        $formula
                    )))
                    .expect(&format!("Could not read formula {}", $formula));

                    let v0 = AtlVertex::Full {
                        state: graph.game_structure.initial_state_index(),
                        formula,
                    };

                    distributed_certain_zero(
                        graph,
                        v0,
                        num_cpus::get() as u64,
                        BreadthFirstSearchBuilder,
                        PRIORITISE_BACK_PROPAGATION,
                        false,
                    );
                });
            });
        }
    };
}

macro_rules! bench_lcgs_threads {
    ($name:ident, $model:expr, $formula:expr) => {
        fn $name(c: &mut Criterion) {
            let mut group = c.benchmark_group(stringify!($name));

            for core_count in 1..num_cpus::get() + 1 {
                let core_count = core_count as u64;

                // Write header for stats if enabled
                #[cfg(feature = "use-counts")]
                eprintln!(
                    "{}{}",
                    concat!(
                        "[stats] bench_run_start: ",
                        stringify!($name),
                        " core_count: "
                    ),
                    core_count
                );
                group.bench_with_input(
                    BenchmarkId::from_parameter(core_count),
                    &core_count,
                    |b, &core_count| {
                        b.iter(|| {
                            let lcgs = parse_lcgs(include_str!(concat!(
                                lcgs_model_path_prefix!(),
                                $model
                            )))
                            .expect(&format!("Could not read model {}", $model));
                            let game_structure =
                                IntermediateLcgs::create(lcgs).expect("Could not symbolcheck");
                            let graph = AtlDependencyGraph { game_structure };

                            let formula = serde_json::from_str(include_str!(concat!(
                                lcgs_model_path_prefix!(),
                                $formula
                            )))
                            .expect(&format!("Could not read formula {}", $formula));

                            let v0 = AtlVertex::Full {
                                state: graph.game_structure.initial_state_index(),
                                formula,
                            };

                            distributed_certain_zero(
                                graph,
                                v0,
                                core_count,
                                BreadthFirstSearchBuilder,
                                PRIORITISE_BACK_PROPAGATION,
                                false,
                            );
                        });
                    },
                );
            }
        }
    };
}

// STATIC THREAD COUNT
bench_lcgs!(
    mexican_standoff_3p_3hp_lcgs_survive,
    "mexican_standoff/mexican_standoff_3p_3hp.lcgs",
    "mexican_standoff/can_p1_guarantee_to_survive_FALSE.json"
);

bench_lcgs!(
    mexican_standoff_5p_1hp_lcgs_survive,
    "mexican_standoff/mexican_standoff_5p_1hp.lcgs",
    "mexican_standoff/can_p1_guarantee_to_survive_FALSE.json"
);

// tic tac toe
bench_lcgs!(
    ttt1,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_cross_avoid_lose_TRUE.json"
);

bench_lcgs!(
    ttt2,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_cross_guarantee_tie_TRUE.json"
);

bench_lcgs!(
    ttt3,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_cross_guarantee_win_FALSE.json"
);

bench_lcgs!(
    ttt4,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_nought_avoid_lose_TRUE.json"
);

bench_lcgs!(
    ttt5,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_nought_guarantee_win_FALSE.json"
);

// robot grid
bench_lcgs!(
    rc1,
    "robot_grid/robot_grid_N3.lcgs",
    "robot_grid/can_r1_and_r2_swap_with_help_from_r3_FALSE.json"
);

bench_lcgs!(
    rc2,
    "robot_grid/robot_grid_N3.lcgs",
    "robot_grid/everyone_starts_home_TRUE.json"
);

bench_lcgs!(
    rc3,
    "robot_grid/robot_grid_N3.lcgs",
    "robot_grid/exist_path_to_targets_with_no_crashes_TRUE.json"
);

// peterson
bench_lcgs!(
    pa1_3proc,
    "peterson/3/peterson_03.lcgs",
    "peterson/p0_eventually_get_access_if_requested_TRUE.json"
);

bench_lcgs!(
    pa2_3proc,
    "peterson/3/peterson_03.lcgs",
    "peterson/p0_eventually_reach_CS_TRUE.json"
);

bench_lcgs!(
    pa3_3proc,
    "peterson/3/peterson_03.lcgs",
    "peterson/3/ensure_mutual_exclusion_TRUE_03.json"
);

bench_lcgs!(
    pa4_3proc,
    "peterson/3/peterson_03.lcgs",
    "peterson/3/multiple_in_CS_FALSE_03.json"
);

bench_lcgs!(
    pa5_3proc,
    "peterson/3/peterson_03.lcgs",
    "peterson/3/never_reach_deadlock_TRUE_03.json"
);

bench_lcgs!(
    pa1_4proc,
    "peterson/4/peterson_04.lcgs",
    "peterson/p0_eventually_get_access_if_requested_TRUE.json"
);

bench_lcgs!(
    pa2_4proc,
    "peterson/4/peterson_04.lcgs",
    "peterson/p0_eventually_reach_CS_TRUE.json"
);

bench_lcgs!(
    pa3_4proc,
    "peterson/4/peterson_04.lcgs",
    "peterson/4/ensure_mutual_exclusion_TRUE_04.json"
);

bench_lcgs!(
    pa4_4proc,
    "peterson/4/peterson_04.lcgs",
    "peterson/4/multiple_in_CS_FALSE_04.json"
);

bench_lcgs!(
    pa5_4proc,
    "peterson/4/peterson_04.lcgs",
    "peterson/4/never_reach_deadlock_TRUE_04.json"
);

bench_lcgs!(
    //takes ~15s for one run
    pa1_9proc,
    "peterson/9/peterson_09.lcgs",
    "peterson/p0_eventually_get_access_if_requested_TRUE.json"
);

// gossiping girls
bench_lcgs!(
    gg1_circular,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/all_girls_ensure_that_all_girls_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs!(
    gg2_circular,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/all_girls_ensure_that_only_player_one_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs!(
    gg3_circular,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/all_girls_ensure_that_player_one_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs!(
    gg4_circular,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/all_omniscient_but_first_after_10_steps_TRUE.json"
);

bench_lcgs!(
    gg5_circular,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/eventually_10_steps_are_passed_TRUE.json"
);

bench_lcgs!(
    gg6_circular,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/girl_one_gurantee_to_become_omicient_before_10_steps_FALSE.json"
);

bench_lcgs!(
    gg7_circular,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/guarantee_all_girls_eventually_become_omniscient_but_not_girl_one_TRUE.json"
);

bench_lcgs!(
    gg1_total,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/all_girls_ensure_that_all_girls_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs!(
    gg2_total,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/all_girls_ensure_that_only_player_one_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs!(
    gg3_total,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/all_girls_ensure_that_player_one_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs!(
    gg4_total,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/all_omniscient_but_first_after_10_steps_TRUE.json"
);

bench_lcgs!(
    gg5_total,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/eventually_10_steps_are_passed_TRUE.json"
);

bench_lcgs!(
    gg6_total,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/girl_one_gurantee_to_become_omicient_before_10_steps_FALSE.json"
);

bench_lcgs!(
    gg7_total,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/guarantee_all_girls_eventually_become_omniscient_but_not_girl_one_TRUE.json"
);

// rock paper scissors
bench_lcgs!(
    rps1,
    "rock_paper_scissors/rock_paper_scissors.lcgs",
    "rock_paper_scissors/p1_always_wins_FALSE.json"
);

bench_lcgs!(
    rps2,
    "rock_paper_scissors/rock_paper_scissors.lcgs",
    "rock_paper_scissors/p1_can_win_eventually_FALSE.json"
);

// matching pennies
bench_lcgs!(
    mp1,
    "matching_pennies/matching_pennies_game.lcgs",
    "matching_pennies/can_odd_win_round_eventually_FALSE.json"
);

bench_lcgs!(
    mp2,
    "matching_pennies/matching_pennies_game.lcgs",
    "matching_pennies/can_they_guarantee_that_odd_always_has_larger_sum_TRUE.json"
);

bench_lcgs!(
    mp3,
    "matching_pennies/matching_pennies_game.lcgs",
    "matching_pennies/can_they_win_simultaneously_FALSE.json"
);

// RANDOM GENERATED MODELS
// rand_1p_1m_530d
bench_json!(
    rand_1p_1m_530d_term_despite_invariant_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_despite_invariant_0.json"
);

bench_json!(
    rand_1p_1m_530d_term_despite_invariant_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_despite_invariant_1.json"
);

bench_json!(
    rand_1p_1m_530d_term_despite_invariant_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_despite_invariant_2.json"
);

bench_json!(
    rand_1p_1m_530d_term_despite_next_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_despite_next_0.json"
);

bench_json!(
    rand_1p_1m_530d_term_despite_next_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_despite_next_1.json"
);

bench_json!(
    rand_1p_1m_530d_term_despite_next_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_despite_next_2.json"
);

bench_json!(
    rand_1p_1m_530d_term_enforce_invariant_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_enforce_invariant_0.json"
);

bench_json!(
    rand_1p_1m_530d_term_enforce_invariant_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_enforce_invariant_1.json"
);

bench_json!(
    rand_1p_1m_530d_term_enforce_invariant_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_enforce_invariant_2.json"
);

bench_json!(
    rand_1p_1m_530d_term_enforce_next_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_enforce_next_0.json"
);

bench_json!(
    rand_1p_1m_530d_term_enforce_next_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_enforce_next_1.json"
);

bench_json!(
    rand_1p_1m_530d_term_enforce_next_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_early_termination_enforce_next_2.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_eventually_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_eventually_0.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_eventually_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_eventually_1.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_eventually_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_eventually_2.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_invariant_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_invariant_0.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_invariant_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_invariant_1.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_invariant_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_invariant_2.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_next_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_next_0.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_next_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_next_1.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_next_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_next_2.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_until_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_until_0.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_until_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_until_1.json"
);

bench_json!(
    rand_1p_1m_530d_state_despite_until_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_despite_until_2.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_eventually_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_eventually_0.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_eventually_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_eventually_1.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_eventually_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_eventually_2.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_invariant_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_invariant_0.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_invariant_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_invariant_1.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_invariant_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_invariant_2.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_next_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_next_0.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_next_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_next_1.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_next_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_next_2.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_until_0,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_until_0.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_until_1,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_until_1.json"
);

bench_json!(
    rand_1p_1m_530d_state_enforce_until_2,
    "random_generated/rand_1p_1m_530d/cgs.json",
    "random_generated/rand_1p_1m_530d/atl/cgs_whole_statespace_enforce_until_2.json"
);

//rand_2p_1m_546d
bench_json!(
    rand_2p_1m_546d_term_despite_invariant_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_despite_invariant_0.json"
);

bench_json!(
    rand_2p_1m_546d_term_despite_invariant_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_despite_invariant_1.json"
);

bench_json!(
    rand_2p_1m_546d_term_despite_invariant_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_despite_invariant_2.json"
);

bench_json!(
    rand_2p_1m_546d_term_despite_next_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_despite_next_0.json"
);

bench_json!(
    rand_2p_1m_546d_term_despite_next_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_despite_next_1.json"
);

bench_json!(
    rand_2p_1m_546d_term_despite_next_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_despite_next_2.json"
);

bench_json!(
    rand_2p_1m_546d_term_enforce_invariant_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_enforce_invariant_0.json"
);

bench_json!(
    rand_2p_1m_546d_term_enforce_invariant_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_enforce_invariant_1.json"
);

bench_json!(
    rand_2p_1m_546d_term_enforce_invariant_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_enforce_invariant_2.json"
);

bench_json!(
    rand_2p_1m_546d_term_enforce_next_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_enforce_next_0.json"
);

bench_json!(
    rand_2p_1m_546d_term_enforce_next_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_enforce_next_1.json"
);

bench_json!(
    rand_2p_1m_546d_term_enforce_next_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_early_termination_enforce_next_2.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_eventually_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_eventually_0.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_eventually_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_eventually_1.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_eventually_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_eventually_2.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_invariant_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_invariant_0.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_invariant_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_invariant_1.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_invariant_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_invariant_2.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_next_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_next_0.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_next_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_next_1.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_next_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_next_2.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_until_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_until_0.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_until_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_until_1.json"
);

bench_json!(
    rand_2p_1m_546d_state_despite_until_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_despite_until_2.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_eventually_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_eventually_0.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_eventually_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_eventually_1.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_eventually_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_eventually_2.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_invariant_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_invariant_0.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_invariant_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_invariant_1.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_invariant_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_invariant_2.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_next_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_next_0.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_next_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_next_1.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_next_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_next_2.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_until_0,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_until_0.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_until_1,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_until_1.json"
);

bench_json!(
    rand_2p_1m_546d_state_enforce_until_2,
    "random_generated/rand_2p_1m_546d/cgs.json",
    "random_generated/rand_2p_1m_546d/atl/cgs_whole_statespace_enforce_until_2.json"
);

//rand_3p_1m_400d
bench_json!(
    rand_3p_1m_400d_term_despite_invariant_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_despite_invariant_0.json"
);

bench_json!(
    rand_3p_1m_400d_term_despite_invariant_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_despite_invariant_1.json"
);

bench_json!(
    rand_3p_1m_400d_term_despite_invariant_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_despite_invariant_2.json"
);

bench_json!(
    rand_3p_1m_400d_term_despite_next_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_despite_next_0.json"
);

bench_json!(
    rand_3p_1m_400d_term_despite_next_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_despite_next_1.json"
);

bench_json!(
    rand_3p_1m_400d_term_despite_next_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_despite_next_2.json"
);

bench_json!(
    rand_3p_1m_400d_term_enforce_invariant_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_enforce_invariant_0.json"
);

bench_json!(
    rand_3p_1m_400d_term_enforce_invariant_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_enforce_invariant_1.json"
);

bench_json!(
    rand_3p_1m_400d_term_enforce_invariant_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_enforce_invariant_2.json"
);

bench_json!(
    rand_3p_1m_400d_term_enforce_next_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_enforce_next_0.json"
);

bench_json!(
    rand_3p_1m_400d_term_enforce_next_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_enforce_next_1.json"
);

bench_json!(
    rand_3p_1m_400d_term_enforce_next_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_early_termination_enforce_next_2.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_eventually_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_eventually_0.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_eventually_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_eventually_1.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_eventually_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_eventually_2.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_invariant_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_invariant_0.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_invariant_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_invariant_1.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_invariant_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_invariant_2.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_next_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_next_0.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_next_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_next_1.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_next_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_next_2.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_until_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_until_0.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_until_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_until_1.json"
);

bench_json!(
    rand_3p_1m_400d_state_despite_until_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_despite_until_2.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_eventually_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_eventually_0.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_eventually_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_eventually_1.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_eventually_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_eventually_2.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_invariant_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_invariant_0.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_invariant_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_invariant_1.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_invariant_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_invariant_2.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_next_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_next_0.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_next_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_next_1.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_next_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_next_2.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_until_0,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_until_0.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_until_1,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_until_1.json"
);

bench_json!(
    rand_3p_1m_400d_state_enforce_until_2,
    "random_generated/rand_3p_1m_400d/cgs.json",
    "random_generated/rand_3p_1m_400d/atl/cgs_whole_statespace_enforce_until_2.json"
);

//rand_3p_3m_405d
bench_json!(
    rand_3p_3m_405d_term_despite_invariant_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_despite_invariant_0.json"
);

bench_json!(
    rand_3p_3m_405d_term_despite_invariant_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_despite_invariant_1.json"
);

bench_json!(
    rand_3p_3m_405d_term_despite_invariant_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_despite_invariant_2.json"
);

bench_json!(
    rand_3p_3m_405d_term_despite_next_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_despite_next_0.json"
);

bench_json!(
    rand_3p_3m_405d_term_despite_next_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_despite_next_1.json"
);

bench_json!(
    rand_3p_3m_405d_term_despite_next_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_despite_next_2.json"
);

bench_json!(
    rand_3p_3m_405d_term_enforce_invariant_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_enforce_invariant_0.json"
);

bench_json!(
    rand_3p_3m_405d_term_enforce_invariant_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_enforce_invariant_1.json"
);

bench_json!(
    rand_3p_3m_405d_term_enforce_invariant_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_enforce_invariant_2.json"
);

bench_json!(
    rand_3p_3m_405d_term_enforce_next_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_enforce_next_0.json"
);

bench_json!(
    rand_3p_3m_405d_term_enforce_next_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_enforce_next_1.json"
);

bench_json!(
    rand_3p_3m_405d_term_enforce_next_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_early_termination_enforce_next_2.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_eventually_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_eventually_0.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_eventually_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_eventually_1.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_eventually_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_eventually_2.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_invariant_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_invariant_0.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_invariant_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_invariant_1.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_invariant_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_invariant_2.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_next_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_next_0.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_next_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_next_1.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_next_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_next_2.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_until_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_until_0.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_until_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_until_1.json"
);

bench_json!(
    rand_3p_3m_405d_state_despite_until_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_despite_until_2.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_eventually_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_eventually_0.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_eventually_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_eventually_1.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_eventually_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_eventually_2.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_invariant_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_invariant_0.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_invariant_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_invariant_1.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_invariant_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_invariant_2.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_next_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_next_0.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_next_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_next_1.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_next_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_next_2.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_until_0,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_until_0.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_until_1,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_until_1.json"
);

bench_json!(
    rand_3p_3m_405d_state_enforce_until_2,
    "random_generated/rand_3p_3m_405d/cgs.json",
    "random_generated/rand_3p_3m_405d/atl/cgs_whole_statespace_enforce_until_2.json"
);

//rand_3p_4m_171d
bench_json!(
    rand_3p_4m_171d_term_despite_invariant_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_despite_invariant_0.json"
);

bench_json!(
    rand_3p_4m_171d_term_despite_invariant_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_despite_invariant_1.json"
);

bench_json!(
    rand_3p_4m_171d_term_despite_invariant_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_despite_invariant_2.json"
);

bench_json!(
    rand_3p_4m_171d_term_despite_next_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_despite_next_0.json"
);

bench_json!(
    rand_3p_4m_171d_term_despite_next_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_despite_next_1.json"
);

bench_json!(
    rand_3p_4m_171d_term_despite_next_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_despite_next_2.json"
);

bench_json!(
    rand_3p_4m_171d_term_enforce_invariant_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_enforce_invariant_0.json"
);

bench_json!(
    rand_3p_4m_171d_term_enforce_invariant_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_enforce_invariant_1.json"
);

bench_json!(
    rand_3p_4m_171d_term_enforce_invariant_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_enforce_invariant_2.json"
);

bench_json!(
    rand_3p_4m_171d_term_enforce_next_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_enforce_next_0.json"
);

bench_json!(
    rand_3p_4m_171d_term_enforce_next_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_enforce_next_1.json"
);

bench_json!(
    rand_3p_4m_171d_term_enforce_next_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_early_termination_enforce_next_2.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_eventually_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_eventually_0.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_eventually_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_eventually_1.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_eventually_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_eventually_2.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_invariant_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_invariant_0.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_invariant_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_invariant_1.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_invariant_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_invariant_2.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_next_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_next_0.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_next_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_next_1.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_next_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_next_2.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_until_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_until_0.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_until_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_until_1.json"
);

bench_json!(
    rand_3p_4m_171d_state_despite_until_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_despite_until_2.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_eventually_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_eventually_0.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_eventually_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_eventually_1.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_eventually_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_eventually_2.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_invariant_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_invariant_0.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_invariant_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_invariant_1.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_invariant_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_invariant_2.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_next_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_next_0.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_next_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_next_1.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_next_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_next_2.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_until_0,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_until_0.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_until_1,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_until_1.json"
);

bench_json!(
    rand_3p_4m_171d_state_enforce_until_2,
    "random_generated/rand_3p_4m_171d/cgs.json",
    "random_generated/rand_3p_4m_171d/atl/cgs_whole_statespace_enforce_until_2.json"
);

//rand_4p_4m_3000d
bench_json!(
    rand_4p_4m_3000d_term_despite_invariant_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_despite_invariant_0.json"
);

bench_json!(
    rand_4p_4m_3000d_term_despite_invariant_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_despite_invariant_1.json"
);

bench_json!(
    rand_4p_4m_3000d_term_despite_invariant_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_despite_invariant_2.json"
);

bench_json!(
    rand_4p_4m_3000d_term_despite_next_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_despite_next_0.json"
);

bench_json!(
    rand_4p_4m_3000d_term_despite_next_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_despite_next_1.json"
);

bench_json!(
    rand_4p_4m_3000d_term_despite_next_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_despite_next_2.json"
);

bench_json!(
    rand_4p_4m_3000d_term_enforce_invariant_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_enforce_invariant_0.json"
);

bench_json!(
    rand_4p_4m_3000d_term_enforce_invariant_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_enforce_invariant_1.json"
);

bench_json!(
    rand_4p_4m_3000d_term_enforce_invariant_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_enforce_invariant_2.json"
);

bench_json!(
    rand_4p_4m_3000d_term_enforce_next_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_enforce_next_0.json"
);

bench_json!(
    rand_4p_4m_3000d_term_enforce_next_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_enforce_next_1.json"
);

bench_json!(
    rand_4p_4m_3000d_term_enforce_next_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_early_termination_enforce_next_2.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_eventually_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_eventually_0.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_eventually_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_eventually_1.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_eventually_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_eventually_2.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_invariant_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_invariant_0.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_invariant_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_invariant_1.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_invariant_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_invariant_2.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_next_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_next_0.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_next_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_next_1.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_next_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_next_2.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_until_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_until_0.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_until_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_until_1.json"
);

bench_json!(
    rand_4p_4m_3000d_state_despite_until_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_despite_until_2.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_eventually_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_eventually_0.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_eventually_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_eventually_1.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_eventually_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_eventually_2.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_invariant_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_invariant_0.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_invariant_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_invariant_1.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_invariant_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_invariant_2.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_next_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_next_0.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_next_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_next_1.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_next_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_next_2.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_until_0,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_until_0.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_until_1,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_until_1.json"
);

bench_json!(
    rand_4p_4m_3000d_state_enforce_until_2,
    "random_generated/rand_4p_4m_3000d/cgs.json",
    "random_generated/rand_4p_4m_3000d/atl/cgs_whole_statespace_enforce_until_2.json"
);

//rand_5p_5m_3000d
bench_json!(
    rand_5p_5m_3000d_term_despite_invariant_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_despite_invariant_0.json"
);

bench_json!(
    rand_5p_5m_3000d_term_despite_invariant_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_despite_invariant_1.json"
);

bench_json!(
    rand_5p_5m_3000d_term_despite_invariant_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_despite_invariant_2.json"
);

bench_json!(
    rand_5p_5m_3000d_term_despite_next_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_despite_next_0.json"
);

bench_json!(
    rand_5p_5m_3000d_term_despite_next_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_despite_next_1.json"
);

bench_json!(
    rand_5p_5m_3000d_term_despite_next_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_despite_next_2.json"
);

bench_json!(
    rand_5p_5m_3000d_term_enforce_invariant_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_enforce_invariant_0.json"
);

bench_json!(
    rand_5p_5m_3000d_term_enforce_invariant_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_enforce_invariant_1.json"
);

bench_json!(
    rand_5p_5m_3000d_term_enforce_invariant_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_enforce_invariant_2.json"
);

bench_json!(
    rand_5p_5m_3000d_term_enforce_next_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_enforce_next_0.json"
);

bench_json!(
    rand_5p_5m_3000d_term_enforce_next_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_enforce_next_1.json"
);

bench_json!(
    rand_5p_5m_3000d_term_enforce_next_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_early_termination_enforce_next_2.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_eventually_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_eventually_0.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_eventually_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_eventually_1.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_eventually_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_eventually_2.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_invariant_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_invariant_0.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_invariant_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_invariant_1.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_invariant_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_invariant_2.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_next_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_next_0.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_next_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_next_1.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_next_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_next_2.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_until_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_until_0.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_until_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_until_1.json"
);

bench_json!(
    rand_5p_5m_3000d_state_despite_until_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_despite_until_2.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_eventually_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_eventually_0.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_eventually_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_eventually_1.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_eventually_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_eventually_2.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_invariant_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_invariant_0.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_invariant_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_invariant_1.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_invariant_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_invariant_2.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_next_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_next_0.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_next_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_next_1.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_next_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_next_2.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_until_0,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_until_0.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_until_1,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_until_1.json"
);

bench_json!(
    rand_5p_5m_3000d_state_enforce_until_2,
    "random_generated/rand_5p_5m_3000d/cgs.json",
    "random_generated/rand_5p_5m_3000d/atl/cgs_whole_statespace_enforce_until_2.json"
);

// MULTIPLE THREAD COUNT
// mexican
bench_lcgs_threads!(
    mexican_standoff_3p_3hp_lcgs_survive_threads,
    "mexican_standoff/mexican_standoff_3p_3hp.lcgs",
    "mexican_standoff/can_p1_guarantee_to_survive_FALSE.json"
);

bench_lcgs_threads!(
    mexican_standoff_5p_1hp_lcgs_survive_threads,
    "mexican_standoff/mexican_standoff_5p_1hp.lcgs",
    "mexican_standoff/can_p1_guarantee_to_survive_FALSE.json"
);

// tic tac toe
bench_lcgs_threads!(
    ttt1_threads,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_cross_avoid_lose_TRUE.json"
);

bench_lcgs_threads!(
    ttt2_threads,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_cross_guarantee_tie_TRUE.json"
);

bench_lcgs_threads!(
    ttt3_threads,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_cross_guarantee_win_FALSE.json"
);

bench_lcgs_threads!(
    ttt4_threads,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_nought_avoid_lose_TRUE.json"
);

bench_lcgs_threads!(
    ttt5_threads,
    "tic_tac_toe/tic_tac_toe.lcgs",
    "tic_tac_toe/can_nought_guarantee_win_FALSE.json"
);

// robot grid
bench_lcgs_threads!(
    rc1_threads,
    "robot_grid/robot_grid_N3.lcgs",
    "robot_grid/can_r1_and_r2_swap_with_help_from_r3_FALSE.json"
);

bench_lcgs_threads!(
    rc2_threads,
    "robot_grid/robot_grid_N3.lcgs",
    "robot_grid/everyone_starts_home_TRUE.json"
);

bench_lcgs_threads!(
    rc3_threads,
    "robot_grid/robot_grid_N3.lcgs",
    "robot_grid/exist_path_to_targets_with_no_crashes_TRUE.json"
);

// peterson
bench_lcgs_threads!(
    pa1_3proc_threads,
    "peterson/3/peterson_03.lcgs",
    "peterson/p0_eventually_get_access_if_requested_TRUE.json"
);

bench_lcgs_threads!(
    pa2_3proc_threads,
    "peterson/3/peterson_03.lcgs",
    "peterson/p0_eventually_reach_CS_TRUE.json"
);

bench_lcgs_threads!(
    pa3_3proc_threads,
    "peterson/3/peterson_03.lcgs",
    "peterson/3/ensure_mutual_exclusion_TRUE_03.json"
);

bench_lcgs_threads!(
    pa4_3proc_threads,
    "peterson/3/peterson_03.lcgs",
    "peterson/3/multiple_in_CS_FALSE_03.json"
);

bench_lcgs_threads!(
    pa5_3proc_threads,
    "peterson/3/peterson_03.lcgs",
    "peterson/3/never_reach_deadlock_TRUE_03.json"
);

bench_lcgs_threads!(
    pa1_4proc_threads,
    "peterson/4/peterson_04.lcgs",
    "peterson/p0_eventually_get_access_if_requested_TRUE.json"
);

bench_lcgs_threads!(
    pa2_4proc_threads,
    "peterson/4/peterson_04.lcgs",
    "peterson/p0_eventually_reach_CS_TRUE.json"
);

bench_lcgs_threads!(
    pa3_4proc_threads,
    "peterson/4/peterson_04.lcgs",
    "peterson/4/ensure_mutual_exclusion_TRUE_04.json"
);

bench_lcgs_threads!(
    pa4_4proc_threads,
    "peterson/4/peterson_04.lcgs",
    "peterson/4/multiple_in_CS_FALSE_04.json"
);

bench_lcgs_threads!(
    pa5_4proc_threads,
    "peterson/4/peterson_04.lcgs",
    "peterson/4/never_reach_deadlock_TRUE_04.json"
);

bench_lcgs_threads!(
    //takes ~15s for one run
    pa1_9proc_threads,
    "peterson/9/peterson_09.lcgs",
    "peterson/p0_eventually_get_access_if_requested_TRUE.json"
);

// gossiping girls
bench_lcgs_threads!(
    gg1_circular_threads,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/all_girls_ensure_that_all_girls_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs_threads!(
    gg2_circular_threads,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/all_girls_ensure_that_only_player_one_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs_threads!(
    gg3_circular_threads,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/all_girls_ensure_that_player_one_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs_threads!(
    gg4_circular_threads,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/all_omniscient_but_first_after_10_steps_TRUE.json"
);

bench_lcgs_threads!(
    gg5_circular_threads,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/eventually_10_steps_are_passed_TRUE.json"
);

bench_lcgs_threads!(
    gg6_circular_threads,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/girl_one_gurantee_to_become_omicient_before_10_steps_FALSE.json"
);

bench_lcgs_threads!(
    gg7_circular_threads,
    "gossipping_girls/gossipping_girls_circular.lcgs",
    "gossipping_girls/guarantee_all_girls_eventually_become_omniscient_but_not_girl_one_TRUE.json"
);

bench_lcgs_threads!(
    gg1_total_threads,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/all_girls_ensure_that_all_girls_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs_threads!(
    gg2_total_threads,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/all_girls_ensure_that_only_player_one_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs_threads!(
    gg3_total_threads,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/all_girls_ensure_that_player_one_gets_omicient_before_10_steps_TRUE.json"
);

bench_lcgs_threads!(
    gg4_total_threads,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/all_omniscient_but_first_after_10_steps_TRUE.json"
);

bench_lcgs_threads!(
    gg5_total_threads,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/eventually_10_steps_are_passed_TRUE.json"
);

bench_lcgs_threads!(
    gg6_total_threads,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/girl_one_gurantee_to_become_omicient_before_10_steps_FALSE.json"
);

bench_lcgs_threads!(
    gg7_total_threads,
    "gossipping_girls/gossipping_girls_total_network.lcgs",
    "gossipping_girls/guarantee_all_girls_eventually_become_omniscient_but_not_girl_one_TRUE.json"
);

// rock paper scissors
bench_lcgs_threads!(
    rps1_threads,
    "rock_paper_scissors/rock_paper_scissors.lcgs",
    "rock_paper_scissors/p1_always_wins_FALSE.json"
);

bench_lcgs_threads!(
    rps2_threads,
    "rock_paper_scissors/rock_paper_scissors.lcgs",
    "rock_paper_scissors/p1_can_win_eventually_FALSE.json"
);

// matching pennies
bench_lcgs_threads!(
    mp1_threads,
    "matching_pennies/matching_pennies_game.lcgs",
    "matching_pennies/can_odd_win_round_eventually_FALSE.json"
);

bench_lcgs_threads!(
    mp2_threads,
    "matching_pennies/matching_pennies_game.lcgs",
    "matching_pennies/can_they_guarantee_that_odd_always_has_larger_sum_TRUE.json"
);

bench_lcgs_threads!(
    mp3_threads,
    "matching_pennies/matching_pennies_game.lcgs",
    "matching_pennies/can_they_win_simultaneously_FALSE.json"
);

// groups take a name as first argument, all subsequent arguments are benchmarks for this group
// static_thread_case_studies naming is one-to-one with paper shorthand
criterion_group!(
    static_thread_case_studies,
    mexican_standoff_3p_3hp_lcgs_survive,
    mexican_standoff_5p_1hp_lcgs_survive,
    ttt1,
    ttt2,
    ttt3,
    ttt4,
    ttt5,
    rc1,
    rc2,
    rc3, //rc benches takes 215s total
    pa1_3proc,
    pa2_3proc,
    pa3_3proc,
    pa4_3proc,
    pa5_3proc,
    pa1_4proc,
    pa2_4proc,
    pa3_4proc,
    pa4_4proc,
    pa5_4proc,
    gg1_circular,
    gg2_circular,
    gg3_circular,
    gg4_circular,
    gg5_circular,
    gg6_circular,
    gg7_circular,
    gg1_total,
    gg2_total,
    gg3_total,
    gg4_total,
    gg5_total,
    gg6_total,
    gg7_total, // gg_total benches takes avg 69s a piece (nice)
    rps1,
    rps2,
    mp1,
    mp2,
    mp3,
);

criterion_group!(
    multi_thread_case_studies,
    mexican_standoff_3p_3hp_lcgs_survive_threads,
    mexican_standoff_5p_1hp_lcgs_survive_threads,
    ttt1_threads,
    ttt2_threads,
    ttt3_threads,
    ttt4_threads,
    ttt5_threads,
    //rc1_threads,
    //rc2_threads,
    //rc3_threads, //rc benches takes 215s total
    pa1_3proc_threads,
    pa2_3proc_threads,
    pa3_3proc_threads,
    pa4_3proc_threads,
    pa5_3proc_threads,
    pa1_4proc_threads,
    pa2_4proc_threads,
    pa3_4proc_threads,
    pa4_4proc_threads,
    pa5_4proc_threads,
    gg1_circular_threads,
    gg2_circular_threads,
    gg3_circular_threads,
    gg4_circular_threads,
    gg5_circular_threads,
    gg6_circular_threads,
    gg7_circular_threads,
    /*
    gg1_total_threads,
    gg2_total_threads,
    gg3_total_threads,
    gg4_total_threads,
    gg5_total_threads,
    gg6_total_threads,
    gg7_total_threads, // gg_total benches takes avg 69s a piece (nice)
    */
    rps1_threads,
    rps2_threads,
    mp1_threads,
    mp2_threads,
    mp3_threads,
);

criterion_group!(
    rand_1p_1m_530d,
    rand_1p_1m_530d_term_despite_invariant_0,
    rand_1p_1m_530d_term_despite_invariant_1,
    rand_1p_1m_530d_term_despite_invariant_2,
    rand_1p_1m_530d_term_despite_next_0,
    rand_1p_1m_530d_term_despite_next_1,
    rand_1p_1m_530d_term_despite_next_2,
    rand_1p_1m_530d_term_enforce_invariant_0,
    rand_1p_1m_530d_term_enforce_invariant_1,
    rand_1p_1m_530d_term_enforce_invariant_2,
    rand_1p_1m_530d_term_enforce_next_0,
    rand_1p_1m_530d_term_enforce_next_1,
    rand_1p_1m_530d_term_enforce_next_2,
    rand_1p_1m_530d_state_despite_eventually_0,
    rand_1p_1m_530d_state_despite_eventually_1,
    rand_1p_1m_530d_state_despite_eventually_2,
    rand_1p_1m_530d_state_despite_invariant_0,
    rand_1p_1m_530d_state_despite_invariant_1,
    rand_1p_1m_530d_state_despite_invariant_2,
    rand_1p_1m_530d_state_despite_next_0,
    rand_1p_1m_530d_state_despite_next_1,
    rand_1p_1m_530d_state_despite_next_2,
    rand_1p_1m_530d_state_despite_until_0,
    rand_1p_1m_530d_state_despite_until_1,
    rand_1p_1m_530d_state_despite_until_2,
    rand_1p_1m_530d_state_enforce_eventually_0,
    rand_1p_1m_530d_state_enforce_eventually_1,
    rand_1p_1m_530d_state_enforce_eventually_2,
    rand_1p_1m_530d_state_enforce_invariant_0,
    rand_1p_1m_530d_state_enforce_invariant_1,
    rand_1p_1m_530d_state_enforce_invariant_2,
    rand_1p_1m_530d_state_enforce_next_0,
    rand_1p_1m_530d_state_enforce_next_1,
    rand_1p_1m_530d_state_enforce_next_2,
    rand_1p_1m_530d_state_enforce_until_0,
    rand_1p_1m_530d_state_enforce_until_1,
    rand_1p_1m_530d_state_enforce_until_2,
);

criterion_group!(
    rand_2p_1m_546d,
    rand_2p_1m_546d_term_despite_invariant_0,
    rand_2p_1m_546d_term_despite_invariant_1,
    rand_2p_1m_546d_term_despite_invariant_2,
    rand_2p_1m_546d_term_despite_next_0,
    rand_2p_1m_546d_term_despite_next_1,
    rand_2p_1m_546d_term_despite_next_2,
    rand_2p_1m_546d_term_enforce_invariant_0,
    rand_2p_1m_546d_term_enforce_invariant_1,
    rand_2p_1m_546d_term_enforce_invariant_2,
    rand_2p_1m_546d_term_enforce_next_0,
    rand_2p_1m_546d_term_enforce_next_1,
    rand_2p_1m_546d_term_enforce_next_2,
    rand_2p_1m_546d_state_despite_eventually_0,
    rand_2p_1m_546d_state_despite_eventually_1,
    rand_2p_1m_546d_state_despite_eventually_2,
    rand_2p_1m_546d_state_despite_invariant_0,
    rand_2p_1m_546d_state_despite_invariant_1,
    rand_2p_1m_546d_state_despite_invariant_2,
    rand_2p_1m_546d_state_despite_next_0,
    rand_2p_1m_546d_state_despite_next_1,
    rand_2p_1m_546d_state_despite_next_2,
    rand_2p_1m_546d_state_despite_until_0,
    rand_2p_1m_546d_state_despite_until_1,
    rand_2p_1m_546d_state_despite_until_2,
    rand_2p_1m_546d_state_enforce_eventually_0,
    rand_2p_1m_546d_state_enforce_eventually_1,
    rand_2p_1m_546d_state_enforce_eventually_2,
    rand_2p_1m_546d_state_enforce_invariant_0,
    rand_2p_1m_546d_state_enforce_invariant_1,
    rand_2p_1m_546d_state_enforce_invariant_2,
    rand_2p_1m_546d_state_enforce_next_0,
    rand_2p_1m_546d_state_enforce_next_1,
    rand_2p_1m_546d_state_enforce_next_2,
    rand_2p_1m_546d_state_enforce_until_0,
    rand_2p_1m_546d_state_enforce_until_1,
    rand_2p_1m_546d_state_enforce_until_2,
);

criterion_group!(
    rand_3p_1m_400d,
    rand_3p_1m_400d_term_despite_invariant_0,
    rand_3p_1m_400d_term_despite_invariant_1,
    rand_3p_1m_400d_term_despite_invariant_2,
    rand_3p_1m_400d_term_despite_next_0,
    rand_3p_1m_400d_term_despite_next_1,
    rand_3p_1m_400d_term_despite_next_2,
    rand_3p_1m_400d_term_enforce_invariant_0,
    rand_3p_1m_400d_term_enforce_invariant_1,
    rand_3p_1m_400d_term_enforce_invariant_2,
    rand_3p_1m_400d_term_enforce_next_0,
    rand_3p_1m_400d_term_enforce_next_1,
    rand_3p_1m_400d_term_enforce_next_2,
    rand_3p_1m_400d_state_despite_eventually_0,
    rand_3p_1m_400d_state_despite_eventually_1,
    rand_3p_1m_400d_state_despite_eventually_2,
    rand_3p_1m_400d_state_despite_invariant_0,
    rand_3p_1m_400d_state_despite_invariant_1,
    rand_3p_1m_400d_state_despite_invariant_2,
    rand_3p_1m_400d_state_despite_next_0,
    rand_3p_1m_400d_state_despite_next_1,
    rand_3p_1m_400d_state_despite_next_2,
    rand_3p_1m_400d_state_despite_until_0,
    rand_3p_1m_400d_state_despite_until_1,
    rand_3p_1m_400d_state_despite_until_2,
    rand_3p_1m_400d_state_enforce_eventually_0,
    rand_3p_1m_400d_state_enforce_eventually_1,
    rand_3p_1m_400d_state_enforce_eventually_2,
    rand_3p_1m_400d_state_enforce_invariant_0,
    rand_3p_1m_400d_state_enforce_invariant_1,
    rand_3p_1m_400d_state_enforce_invariant_2,
    rand_3p_1m_400d_state_enforce_next_0,
    rand_3p_1m_400d_state_enforce_next_1,
    rand_3p_1m_400d_state_enforce_next_2,
    rand_3p_1m_400d_state_enforce_until_0,
    rand_3p_1m_400d_state_enforce_until_1,
    rand_3p_1m_400d_state_enforce_until_2,
);

criterion_group!(
    rand_3p_3m_405d,
    rand_3p_3m_405d_term_despite_invariant_0,
    rand_3p_3m_405d_term_despite_invariant_1,
    rand_3p_3m_405d_term_despite_invariant_2,
    rand_3p_3m_405d_term_despite_next_0,
    rand_3p_3m_405d_term_despite_next_1,
    rand_3p_3m_405d_term_despite_next_2,
    rand_3p_3m_405d_term_enforce_invariant_0,
    rand_3p_3m_405d_term_enforce_invariant_1,
    rand_3p_3m_405d_term_enforce_invariant_2,
    rand_3p_3m_405d_term_enforce_next_0,
    rand_3p_3m_405d_term_enforce_next_1,
    rand_3p_3m_405d_term_enforce_next_2,
    rand_3p_3m_405d_state_despite_eventually_0,
    rand_3p_3m_405d_state_despite_eventually_1,
    rand_3p_3m_405d_state_despite_eventually_2,
    rand_3p_3m_405d_state_despite_invariant_0,
    rand_3p_3m_405d_state_despite_invariant_1,
    rand_3p_3m_405d_state_despite_invariant_2,
    rand_3p_3m_405d_state_despite_next_0,
    rand_3p_3m_405d_state_despite_next_1,
    rand_3p_3m_405d_state_despite_next_2,
    rand_3p_3m_405d_state_despite_until_0,
    rand_3p_3m_405d_state_despite_until_1,
    rand_3p_3m_405d_state_despite_until_2,
    rand_3p_3m_405d_state_enforce_eventually_0,
    rand_3p_3m_405d_state_enforce_eventually_1,
    rand_3p_3m_405d_state_enforce_eventually_2,
    rand_3p_3m_405d_state_enforce_invariant_0,
    rand_3p_3m_405d_state_enforce_invariant_1,
    rand_3p_3m_405d_state_enforce_invariant_2,
    rand_3p_3m_405d_state_enforce_next_0,
    rand_3p_3m_405d_state_enforce_next_1,
    rand_3p_3m_405d_state_enforce_next_2,
    rand_3p_3m_405d_state_enforce_until_0,
    rand_3p_3m_405d_state_enforce_until_1,
    rand_3p_3m_405d_state_enforce_until_2,
);

criterion_group!(
    rand_3p_4m_171d,
    rand_3p_4m_171d_term_despite_invariant_0,
    rand_3p_4m_171d_term_despite_invariant_1,
    rand_3p_4m_171d_term_despite_invariant_2,
    rand_3p_4m_171d_term_despite_next_0,
    rand_3p_4m_171d_term_despite_next_1,
    rand_3p_4m_171d_term_despite_next_2,
    rand_3p_4m_171d_term_enforce_invariant_0,
    rand_3p_4m_171d_term_enforce_invariant_1,
    rand_3p_4m_171d_term_enforce_invariant_2,
    rand_3p_4m_171d_term_enforce_next_0,
    rand_3p_4m_171d_term_enforce_next_1,
    rand_3p_4m_171d_term_enforce_next_2,
    rand_3p_4m_171d_state_despite_eventually_0,
    rand_3p_4m_171d_state_despite_eventually_1,
    rand_3p_4m_171d_state_despite_eventually_2,
    rand_3p_4m_171d_state_despite_invariant_0,
    rand_3p_4m_171d_state_despite_invariant_1,
    rand_3p_4m_171d_state_despite_invariant_2,
    rand_3p_4m_171d_state_despite_next_0,
    rand_3p_4m_171d_state_despite_next_1,
    rand_3p_4m_171d_state_despite_next_2,
    rand_3p_4m_171d_state_despite_until_0,
    rand_3p_4m_171d_state_despite_until_1,
    rand_3p_4m_171d_state_despite_until_2,
    rand_3p_4m_171d_state_enforce_eventually_0,
    rand_3p_4m_171d_state_enforce_eventually_1,
    rand_3p_4m_171d_state_enforce_eventually_2,
    rand_3p_4m_171d_state_enforce_invariant_0,
    rand_3p_4m_171d_state_enforce_invariant_1,
    rand_3p_4m_171d_state_enforce_invariant_2,
    rand_3p_4m_171d_state_enforce_next_0,
    rand_3p_4m_171d_state_enforce_next_1,
    rand_3p_4m_171d_state_enforce_next_2,
    rand_3p_4m_171d_state_enforce_until_0,
    rand_3p_4m_171d_state_enforce_until_1,
    rand_3p_4m_171d_state_enforce_until_2,
);

criterion_group!(
    rand_4p_4m_3000d,
    rand_4p_4m_3000d_term_despite_invariant_0,
    rand_4p_4m_3000d_term_despite_invariant_1,
    rand_4p_4m_3000d_term_despite_invariant_2,
    rand_4p_4m_3000d_term_despite_next_0,
    rand_4p_4m_3000d_term_despite_next_1,
    rand_4p_4m_3000d_term_despite_next_2,
    rand_4p_4m_3000d_term_enforce_invariant_0,
    rand_4p_4m_3000d_term_enforce_invariant_1,
    rand_4p_4m_3000d_term_enforce_invariant_2,
    rand_4p_4m_3000d_term_enforce_next_0,
    rand_4p_4m_3000d_term_enforce_next_1,
    rand_4p_4m_3000d_term_enforce_next_2,
    rand_4p_4m_3000d_state_despite_eventually_0,
    rand_4p_4m_3000d_state_despite_eventually_1,
    rand_4p_4m_3000d_state_despite_eventually_2,
    rand_4p_4m_3000d_state_despite_invariant_0,
    rand_4p_4m_3000d_state_despite_invariant_1,
    rand_4p_4m_3000d_state_despite_invariant_2,
    rand_4p_4m_3000d_state_despite_next_0,
    rand_4p_4m_3000d_state_despite_next_1,
    rand_4p_4m_3000d_state_despite_next_2,
    rand_4p_4m_3000d_state_despite_until_0,
    rand_4p_4m_3000d_state_despite_until_1,
    rand_4p_4m_3000d_state_despite_until_2,
    rand_4p_4m_3000d_state_enforce_eventually_0,
    rand_4p_4m_3000d_state_enforce_eventually_1,
    rand_4p_4m_3000d_state_enforce_eventually_2,
    rand_4p_4m_3000d_state_enforce_invariant_0,
    rand_4p_4m_3000d_state_enforce_invariant_1,
    rand_4p_4m_3000d_state_enforce_invariant_2,
    rand_4p_4m_3000d_state_enforce_next_0,
    rand_4p_4m_3000d_state_enforce_next_1,
    rand_4p_4m_3000d_state_enforce_next_2,
    rand_4p_4m_3000d_state_enforce_until_0,
    rand_4p_4m_3000d_state_enforce_until_1,
    rand_4p_4m_3000d_state_enforce_until_2,
);

// expensive - first bench takes 6h
criterion_group!(
    rand_5p_5m_3000d,
    rand_5p_5m_3000d_term_despite_invariant_0,
    rand_5p_5m_3000d_term_despite_invariant_1,
    rand_5p_5m_3000d_term_despite_invariant_2,
    rand_5p_5m_3000d_term_despite_next_0,
    rand_5p_5m_3000d_term_despite_next_1,
    rand_5p_5m_3000d_term_despite_next_2,
    rand_5p_5m_3000d_term_enforce_invariant_0,
    rand_5p_5m_3000d_term_enforce_invariant_1,
    rand_5p_5m_3000d_term_enforce_invariant_2,
    rand_5p_5m_3000d_term_enforce_next_0,
    rand_5p_5m_3000d_term_enforce_next_1,
    rand_5p_5m_3000d_term_enforce_next_2,
    rand_5p_5m_3000d_state_despite_eventually_0,
    rand_5p_5m_3000d_state_despite_eventually_1,
    rand_5p_5m_3000d_state_despite_eventually_2,
    rand_5p_5m_3000d_state_despite_invariant_0,
    rand_5p_5m_3000d_state_despite_invariant_1,
    rand_5p_5m_3000d_state_despite_invariant_2,
    rand_5p_5m_3000d_state_despite_next_0,
    rand_5p_5m_3000d_state_despite_next_1,
    rand_5p_5m_3000d_state_despite_next_2,
    rand_5p_5m_3000d_state_despite_until_0,
    rand_5p_5m_3000d_state_despite_until_1,
    rand_5p_5m_3000d_state_despite_until_2,
    rand_5p_5m_3000d_state_enforce_eventually_0,
    rand_5p_5m_3000d_state_enforce_eventually_1,
    rand_5p_5m_3000d_state_enforce_eventually_2,
    rand_5p_5m_3000d_state_enforce_invariant_0,
    rand_5p_5m_3000d_state_enforce_invariant_1,
    rand_5p_5m_3000d_state_enforce_invariant_2,
    rand_5p_5m_3000d_state_enforce_next_0,
    rand_5p_5m_3000d_state_enforce_next_1,
    rand_5p_5m_3000d_state_enforce_next_2,
    rand_5p_5m_3000d_state_enforce_until_0,
    rand_5p_5m_3000d_state_enforce_until_1,
    rand_5p_5m_3000d_state_enforce_until_2,
);
// tiny suite for shorter github CI turnaround, check still fails if any path in any declared bench is wrong
criterion_group!(github_action_suite, mexican_standoff_3p_3hp_lcgs_survive);

criterion_main!(
    github_action_suite, // remember to disable when benchmarking
                         //static_thread_case_studies,
                         //rand_1p_1m_530d,
                         //rand_2p_1m_546d,
                         //rand_3p_1m_400d,
                         //rand_3p_3m_405d,
                         //rand_3p_4m_171d,
                         //rand_4p_4m_3000d //disable large test which results in no-space error on MCC
); // choose which group(s) to bench
