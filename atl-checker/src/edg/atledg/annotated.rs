use crate::atl::Phi;
use crate::edg::annotated_edg::{
    AnnotatedEdge, AnnotatedExtendedDependencyGraph, AnnotatedHyperEdge, AnnotatedNegationEdge,
    Annotation,
};
use crate::edg::atledg::pmoves::{DeltaIterator, PartialMove, PmovesIterator};
use crate::edg::atledg::vertex::ATLVertex;
use crate::edg::atledg::ATLDependencyGraph;
use crate::game_structure::GameStructure;
use std::sync::Arc;

impl Annotation for Option<PartialMove> {}

impl<G: GameStructure> AnnotatedExtendedDependencyGraph<ATLVertex, Option<PartialMove>>
    for ATLDependencyGraph<G>
{
    /// Produce the edges of the given vertex annotated with the (partial move) that produces it.
    /// Where possible, the smallest edge will be the first in the produced vector,
    /// and similarly, the smallest target will be the first in the edges' vector of targets.
    /// This is mostly relevant for the Until formulae.
    fn annotated_succ(
        &self,
        vert: &ATLVertex,
    ) -> Vec<AnnotatedEdge<ATLVertex, Option<PartialMove>>> {
        match vert {
            ATLVertex::FULL { state, formula } => match formula.as_ref() {
                Phi::True => {
                    // Hyper edge with no targets
                    vec![AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                        source: vert.clone(),
                        annotation: None,
                        targets: vec![],
                    })]
                }
                Phi::False => {
                    // No edges
                    vec![]
                }
                Phi::Proposition(prop) => {
                    let props = self.game_structure.labels(vert.state());
                    if props.contains(prop) {
                        vec![AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets: vec![],
                        })]
                    } else {
                        vec![]
                    }
                }
                Phi::Not(phi) => {
                    vec![AnnotatedEdge::Negation(AnnotatedNegationEdge {
                        source: vert.clone(),
                        annotation: None,
                        target: ATLVertex::FULL {
                            state: *state,
                            formula: phi.clone(),
                        },
                    })]
                }
                Phi::Or(left, right) => {
                    vec![
                        AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets: vec![(
                                ATLVertex::FULL {
                                    state: *state,
                                    formula: left.clone(),
                                },
                                None,
                            )],
                        }),
                        AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets: vec![(
                                ATLVertex::FULL {
                                    state: *state,
                                    formula: right.clone(),
                                },
                                None,
                            )],
                        }),
                    ]
                }
                Phi::And(left, right) => {
                    vec![AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                        source: vert.clone(),
                        annotation: None,
                        targets: vec![
                            (
                                ATLVertex::FULL {
                                    state: *state,
                                    formula: left.clone(),
                                },
                                None,
                            ),
                            (
                                ATLVertex::FULL {
                                    state: *state,
                                    formula: right.clone(),
                                },
                                None,
                            ),
                        ],
                    })]
                }
                Phi::DespiteNext { players, formula } => {
                    let moves = self.game_structure.move_count(*state);
                    let targets: Vec<(ATLVertex, Option<PartialMove>)> =
                        PmovesIterator::new(moves, players.iter().copied().collect())
                            .map(|pmove| {
                                (
                                    ATLVertex::PARTIAL {
                                        state: *state,
                                        partial_move: pmove,
                                        formula: formula.clone(),
                                    },
                                    None, // The partial move is part of the partial configuration instead
                                )
                            })
                            .collect();

                    vec![AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                        source: vert.clone(),
                        annotation: None,
                        targets,
                    })]
                }
                Phi::EnforceNext { players, formula } => {
                    let moves = self.game_structure.move_count(*state);
                    PmovesIterator::new(moves, players.iter().copied().collect())
                        .map(|pmove| {
                            let targets: Vec<(ATLVertex, Option<PartialMove>)> =
                                DeltaIterator::new(&self.game_structure, *state, &pmove)
                                    .map(|(state, mov)| {
                                        (
                                            ATLVertex::FULL {
                                                state,
                                                formula: formula.clone(),
                                            },
                                            Some(mov),
                                        )
                                    })
                                    .collect();
                            AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                                source: vert.clone(),
                                annotation: Some(pmove),
                                targets,
                            })
                        })
                        .collect::<Vec<AnnotatedEdge<ATLVertex, Option<PartialMove>>>>()
                }
                Phi::DespiteUntil {
                    players,
                    pre,
                    until,
                } => {
                    // `pre`-target
                    // "Is `pre` formula satisfied now?"
                    let pre = (
                        ATLVertex::FULL {
                            state: *state,
                            formula: pre.clone(),
                        },
                        None,
                    );

                    // Together with the `pre` target is all the possible moves by other players,
                    // but it is important that `pre` is the first target
                    let moves = self.game_structure.move_count(*state);
                    let targets: Vec<(ATLVertex, Option<PartialMove>)> = std::iter::once(pre)
                        .chain(
                            PmovesIterator::new(moves, players.iter().cloned().collect()).map(
                                |pmove| {
                                    (
                                        ATLVertex::PARTIAL {
                                            state: *state,
                                            partial_move: pmove,
                                            formula: vert.formula(),
                                        },
                                        None, // The partial move is part of the partial configuration
                                    )
                                },
                            ),
                        )
                        .collect();

                    vec![
                        // `until`-formula branch
                        // "Is the `until` formula satisfied now?"
                        // This must be the first edge
                        AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets: vec![(
                                ATLVertex::FULL {
                                    state: *state,
                                    formula: until.clone(),
                                },
                                None,
                            )],
                        }),
                        // Other branches where pre is satisfied
                        AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets,
                        }),
                    ]
                }
                Phi::EnforceUntil {
                    players,
                    pre,
                    until,
                } => {
                    let mut edges = vec![
                        // `until`-formula branch
                        // "Is the `until` formula satisfied now?"
                        // This must be the first edge
                        AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets: vec![(
                                ATLVertex::FULL {
                                    state: *state,
                                    formula: until.clone(),
                                },
                                None,
                            )],
                        }),
                    ];

                    // `pre`-target
                    // "Is `pre` formula satisfied now?"
                    let pre = (
                        ATLVertex::FULL {
                            state: *state,
                            formula: pre.clone(),
                        },
                        None,
                    );

                    let moves = self.game_structure.move_count(*state);
                    edges.extend(
                        PmovesIterator::new(moves, players.iter().copied().collect()).map(
                            |pmove| {
                                // Together with the `pre` target is all the possible moves by other players,
                                // but it is important that `pre` is the first target
                                let delta =
                                    DeltaIterator::new(&self.game_structure, *state, &pmove).map(
                                        |(state, mov)| {
                                            (
                                                ATLVertex::FULL {
                                                    state,
                                                    formula: formula.clone(),
                                                },
                                                Some(mov),
                                            )
                                        },
                                    );
                                let targets: Vec<(ATLVertex, Option<PartialMove>)> =
                                    std::iter::once(pre.clone()).chain(delta).collect();
                                AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                                    source: vert.clone(),
                                    annotation: Some(pmove),
                                    targets,
                                })
                            },
                        ),
                    );

                    edges
                }
                Phi::DespiteEventually {
                    players,
                    formula: subformula,
                } => {
                    // Partial targets with same formula
                    // "Is the formula satisfied in the next state instead?"
                    let moves = self.game_structure.move_count(*state);
                    let targets: Vec<(ATLVertex, Option<PartialMove>)> =
                        PmovesIterator::new(moves, players.iter().cloned().collect())
                            .map(|pmove| {
                                (
                                    ATLVertex::PARTIAL {
                                        state: *state,
                                        partial_move: pmove,
                                        formula: formula.clone(),
                                    },
                                    None, // Partial move is part of partial configuration
                                )
                            })
                            .collect();

                    vec![
                        // sub-formula target
                        // "Is the sub formula satisfied in current state?"
                        // This must be the first edge
                        AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets: vec![(
                                ATLVertex::FULL {
                                    state: *state,
                                    formula: subformula.clone(),
                                },
                                None,
                            )],
                        }),
                        AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets,
                        }),
                    ]
                }
                Phi::EnforceEventually {
                    players,
                    formula: subformula,
                } => {
                    let mut edges = vec![
                        // sub-formula target
                        // "Is the sub formula satisfied in current state?"
                        // This must be the first edge
                        AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                            source: vert.clone(),
                            annotation: None,
                            targets: vec![(
                                ATLVertex::FULL {
                                    state: *state,
                                    formula: subformula.clone(),
                                },
                                None,
                            )],
                        }),
                    ];

                    // Successor states with same formula
                    // "Is the formula satisfied in the next state instead?"
                    let moves = self.game_structure.move_count(*state);
                    edges.extend(
                        PmovesIterator::new(moves, players.iter().copied().collect()).map(
                            |pmove| {
                                let targets: Vec<(ATLVertex, Option<PartialMove>)> =
                                    DeltaIterator::new(&self.game_structure, *state, &pmove)
                                        .map(|(state, mov)| {
                                            (
                                                ATLVertex::FULL {
                                                    state,
                                                    formula: formula.clone(),
                                                },
                                                Some(mov),
                                            )
                                        })
                                        .collect();
                                AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                                    source: vert.clone(),
                                    annotation: Some(pmove),
                                    targets,
                                })
                            },
                        ),
                    );

                    edges
                }
                Phi::DespiteInvariant {
                    players,
                    formula: subformula,
                } => {
                    vec![AnnotatedEdge::Negation(AnnotatedNegationEdge {
                        source: vert.clone(),
                        annotation: None,
                        target: ATLVertex::FULL {
                            state: *state,
                            // Modified formula, switching to minimum-fixed point domain
                            formula: Arc::new(Phi::EnforceUntil {
                                players: players.clone(),
                                pre: Arc::new(Phi::True),
                                until: Arc::new(Phi::Not(subformula.clone())),
                            }),
                        },
                    })]
                }
                Phi::EnforceInvariant {
                    players,
                    formula: subformula,
                } => {
                    vec![AnnotatedEdge::Negation(AnnotatedNegationEdge {
                        source: vert.clone(),
                        annotation: None,
                        target: ATLVertex::FULL {
                            state: *state,
                            // Modified formula, switching to minimum-fixed point
                            formula: Arc::new(Phi::DespiteUntil {
                                players: players.clone(),
                                pre: Arc::new(Phi::True),
                                until: Arc::new(Phi::Not(subformula.clone())),
                            }),
                        },
                    })]
                }
            },
            ATLVertex::PARTIAL {
                state,
                partial_move,
                formula,
            } => DeltaIterator::new(&self.game_structure, *state, partial_move)
                .map(|(state, mov)| {
                    let targets = vec![(
                        ATLVertex::FULL {
                            state,
                            formula: formula.clone(),
                        },
                        None,
                    )];
                    AnnotatedEdge::Hyper(AnnotatedHyperEdge {
                        source: vert.clone(),
                        annotation: Some(mov),
                        targets,
                    })
                })
                .collect::<Vec<AnnotatedEdge<ATLVertex, Option<PartialMove>>>>(),
        }
    }
}
