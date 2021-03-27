use crate::common::{Edges, HyperEdge, NegationEdge};
use crate::edg::{ExtendedDependencyGraph, Vertex};
use std::collections::HashSet;
use std::sync::Arc;

use crate::atl::common::{Player, State};
use crate::atl::formula::Phi;
use crate::atl::gamestructure::GameStructure;
use std::fmt::{Display, Formatter};
use std::hash::BuildHasher;

#[derive(Clone, Debug)]
pub struct ATLDependencyGraph<G: GameStructure> {
    pub game_structure: G,
}

#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub enum ATLVertex {
    FULL {
        state: State,
        formula: Arc<Phi>,
    },
    PARTIAL {
        state: State,
        partial_move: PartialMove,
        formula: Arc<Phi>,
    },
}

impl Display for ATLVertex {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ATLVertex::FULL { state, formula } => write!(f, "state={} formula={}", state, formula),
            ATLVertex::PARTIAL {
                state,
                partial_move,
                formula,
            } => {
                write!(f, "state={} pmove=[", state)?;
                for (i, choice) in partial_move.iter().enumerate() {
                    choice.fmt(f)?;
                    if i < partial_move.len() - 1 {
                        f.write_str(", ")?;
                    }
                }
                write!(f, "] formula={}", formula)
            }
        }
    }
}

impl ATLVertex {
    fn state(&self) -> State {
        match self {
            ATLVertex::FULL { state, .. } => *state,
            ATLVertex::PARTIAL { state, .. } => *state,
        }
    }

    fn formula(&self) -> Arc<Phi> {
        match self {
            ATLVertex::FULL { formula, .. } => formula.clone(),
            ATLVertex::PARTIAL { formula, .. } => formula.clone(),
        }
    }
}

impl Vertex for ATLVertex {}

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub enum PartialMoveChoice {
    /// Range from 0 to given number
    RANGE(usize),
    /// Chosen move for player
    SPECIFIC(usize),
}

impl Display for PartialMoveChoice {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PartialMoveChoice::RANGE(max) => write!(f, "(0..{})", max - 1),
            PartialMoveChoice::SPECIFIC(choice) => write!(f, "{}", choice),
        }
    }
}

pub type PartialMove = Vec<PartialMoveChoice>;

/// An iterator that produces all move vectors in a partial move.
/// Example: The partial move {1, 2},{1},{1, 2} results in 111, 112, 211, and 212.
struct PartialMoveIterator<'a> {
    partial_move: &'a PartialMove,
    initialized: bool,
    current: Vec<usize>,
}

impl<'a> PartialMoveIterator<'a> {
    /// Create a new PartialMoveIterator
    fn new(partial_move: &'a PartialMove) -> PartialMoveIterator {
        PartialMoveIterator {
            partial_move,
            initialized: false,
            current: vec![],
        }
    }

    /// Initializes the partial move iterator. This should be called exactly once
    /// before any call to make_next. This function creates the first move vector in a partial
    /// move. All partial move always contain at least one move vector.
    fn make_first(&mut self) {
        self.initialized = true;
        // Create the first move vector from matching on the partial move
        self.current = self
            .partial_move
            .iter()
            .map(|case| match case {
                PartialMoveChoice::RANGE(_) => 0,
                PartialMoveChoice::SPECIFIC(n) => *n,
            })
            .collect();
    }

    /// Updates self.current to the next move vector. This function returns false if a new
    /// move vector could not be created (due to exceeding the ranges in the partial move).
    fn make_next(&mut self, player: Player) -> bool {
        if player >= self.partial_move.len() {
            false

        // Call this function recursively, where we check the next player
        } else if !self.make_next(player + 1) {
            // The next player's move has rolled over or doesn't exist.
            // Then it is our turn to roll -- only RANGE can roll, SPECIFIC should not change
            match self.partial_move[player] {
                PartialMoveChoice::SPECIFIC(_) => false,
                PartialMoveChoice::RANGE(n) => {
                    let current = &mut self.current;
                    // Increase move index and return true if it's valid
                    current[player] += 1;
                    if current[player] < n {
                        true
                    } else {
                        // We have rolled over (self.next[player] >= n).
                        // Reset this player's move index and return false to indicate it
                        // was not possible to create a valid next move at this depth
                        current[player] = 0;
                        false
                    }
                }
            }
        } else {
            true
        }
    }
}

/// Allows the PartialMoveIterator to be iterated over.
impl<'a> Iterator for PartialMoveIterator<'a> {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.initialized {
            self.make_first();
            Some(self.current.clone())
        } else if self.make_next(0) {
            Some(self.current.clone())
        } else {
            None
        }
    }
}

pub struct VarsIterator {
    moves: Vec<usize>,
    position: PartialMove,
    completed: bool,
}

impl VarsIterator {
    /// Iterates over all partial moves variants that results from a number of players
    /// making a combination of specific choices.
    ///
    /// # Arguments
    ///
    /// * `moves` number of moves for each player.
    /// * `players` set of players who has to make a specific move.
    ///
    pub fn new(moves: Vec<usize>, players: HashSet<Player>) -> Self {
        let mut position = Vec::with_capacity(moves.len());
        for (i, mov) in moves.iter().enumerate() {
            position.push(if players.contains(&i) {
                PartialMoveChoice::SPECIFIC(0)
            } else {
                PartialMoveChoice::RANGE(*mov)
            })
        }

        Self {
            moves,
            position,
            completed: false,
        }
    }
}

impl Iterator for VarsIterator {
    type Item = PartialMove;

    fn next(&mut self) -> Option<Self::Item> {
        if self.completed {
            return None;
        }

        let current = self.position.clone();

        let mut roll_over_pos = 0;
        loop {
            // If all digits have rolled over we reached the end
            if roll_over_pos >= self.moves.len() {
                self.completed = true;
                break;
            }

            match self.position[roll_over_pos] {
                PartialMoveChoice::RANGE(_) => {
                    roll_over_pos += 1;
                    continue;
                }
                PartialMoveChoice::SPECIFIC(value) => {
                    let new_value = value + 1;

                    if new_value >= self.moves[roll_over_pos] {
                        // Rolled over
                        self.position[roll_over_pos] = PartialMoveChoice::SPECIFIC(0);
                        roll_over_pos += 1;
                    } else {
                        self.position[roll_over_pos] = PartialMoveChoice::SPECIFIC(new_value);
                        break;
                    }
                }
            }
        }

        Some(current)
    }
}

/// An iterator that produces all resulting states from taking a partial move at a state.
/// The iterator will make sure the same state is not produced multiple times.
struct DeltaIterator<'a, G: GameStructure> {
    game_structure: &'a G,
    state: State,
    moves: PartialMoveIterator<'a>,
    /// Contains the states, that have already been produced once, so we can avoid producing
    /// them again
    known: HashSet<State>,
}

impl<'a, G: GameStructure> DeltaIterator<'a, G> {
    /// Create a new DeltaIterator
    fn new(game_structure: &'a G, state: State, moves: &'a PartialMove) -> Self {
        let known = HashSet::new();
        let moves = PartialMoveIterator::new(&moves);

        Self {
            game_structure,
            state,
            moves,
            known,
        }
    }
}

impl<'a, G: GameStructure> Iterator for DeltaIterator<'a, G> {
    type Item = State;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Get the next move vector from the partial move
            let mov = self.moves.next();
            if let Some(mov) = mov {
                let res = self.game_structure.transitions(self.state, mov);
                // Have we already produced this resulting state before?
                if self.known.contains(&res) {
                    continue;
                } else {
                    self.known.insert(res);
                    return Some(res);
                }
            } else {
                return None;
            }
        }
    }
}

impl<G: GameStructure> ATLDependencyGraph<G> {
    #[allow(dead_code)]
    fn invert_players(&self, players: &[Player]) -> HashSet<Player> {
        let max_players = self.game_structure.max_player();
        let mut inv_players =
            HashSet::with_capacity((self.game_structure.max_player()) - players.len());
        // Iterate over all players and only add the ones not in players
        for player in 0usize..max_players {
            if players.contains(&player) {
                inv_players.insert(player);
            }
        }
        inv_players
    }
}

impl<G: GameStructure, H: BuildHasher + Clone + Default> ExtendedDependencyGraph<ATLVertex, H>
    for ATLDependencyGraph<G>
{
    fn succ(&self, vert: &ATLVertex) -> HashSet<Edges<ATLVertex>, H> {
        match vert {
            ATLVertex::FULL { state, formula } => match formula.as_ref() {
                Phi::True => {
                    let mut edges = HashSet::with_hasher(H::default());

                    // Empty hyper edge
                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets: vec![],
                    }));

                    edges
                }
                Phi::False => {
                    // No edges
                    HashSet::with_hasher(H::default())
                }
                Phi::Proposition(prop) => {
                    let props = self.game_structure.labels(vert.state());
                    if props.contains(prop) {
                        let mut edges = HashSet::with_hasher(H::default());
                        edges.insert(Edges::HYPER(HyperEdge {
                            source: vert.clone(),
                            targets: vec![],
                        }));
                        edges
                    } else {
                        HashSet::with_hasher(H::default())
                    }
                }
                Phi::Not(phi) => {
                    let mut edges = HashSet::with_hasher(H::default());

                    edges.insert(Edges::NEGATION(NegationEdge {
                        source: vert.clone(),
                        target: ATLVertex::FULL {
                            state: *state,
                            formula: phi.clone(),
                        },
                    }));

                    edges
                }
                Phi::Or(left, right) => {
                    let mut edges = HashSet::with_hasher(H::default());

                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets: vec![ATLVertex::FULL {
                            state: *state,
                            formula: left.clone(),
                        }],
                    }));

                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets: vec![ATLVertex::FULL {
                            state: *state,
                            formula: right.clone(),
                        }],
                    }));

                    edges
                }
                Phi::And(left, right) => {
                    let mut edges = HashSet::with_hasher(H::default());
                    let mut targets = vec![];

                    targets.push(ATLVertex::FULL {
                        state: *state,
                        formula: left.clone(),
                    });
                    targets.push(ATLVertex::FULL {
                        state: *state,
                        formula: right.clone(),
                    });

                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets,
                    }));

                    edges
                }
                Phi::DespiteNext { players, formula } => {
                    let mut edges = HashSet::with_hasher(H::default());

                    let moves = self.game_structure.move_count(*state);
                    let targets: Vec<ATLVertex> =
                        VarsIterator::new(moves, players.iter().copied().collect())
                            .map(|pmove| ATLVertex::PARTIAL {
                                state: *state,
                                partial_move: pmove,
                                formula: formula.clone(),
                            })
                            .collect();

                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets,
                    }));
                    edges
                }
                Phi::EnforceNext { players, formula } => {
                    let moves: Vec<usize> = self.game_structure.move_count(*state);
                    VarsIterator::new(moves, players.iter().copied().collect())
                        .map(|pmove| {
                            let targets: Vec<ATLVertex> =
                                DeltaIterator::new(&self.game_structure, *state, &pmove)
                                    .map(|state| ATLVertex::FULL {
                                        state,
                                        formula: formula.clone(),
                                    })
                                    .collect();
                            Edges::HYPER(HyperEdge {
                                source: vert.clone(),
                                targets,
                            })
                        })
                        .collect::<HashSet<Edges<ATLVertex>, H>>()
                }
                Phi::DespiteUntil {
                    players,
                    pre,
                    until,
                } => {
                    let mut edges = HashSet::with_hasher(H::default());

                    // `pre`-target
                    // "Is `pre` formula satisfied now?"
                    let pre = ATLVertex::FULL {
                        state: *state,
                        formula: pre.clone(),
                    };

                    let moves = self.game_structure.move_count(*state);
                    let mut targets: Vec<ATLVertex> =
                        VarsIterator::new(moves, players.iter().cloned().collect())
                            .map(|pmove| ATLVertex::PARTIAL {
                                state: *state,
                                partial_move: pmove,
                                formula: vert.formula(),
                            })
                            .collect();
                    targets.push(pre);

                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets,
                    }));

                    // `until`-formula branch
                    // "Is the `until` formula satisfied now?"
                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets: vec![ATLVertex::FULL {
                            state: *state,
                            formula: until.clone(),
                        }],
                    }));

                    edges
                }
                Phi::EnforceUntil {
                    players,
                    pre,
                    until,
                } => {
                    // `pre`-target
                    // "Is `pre` formula satisfied now?"
                    let pre = ATLVertex::FULL {
                        state: *state,
                        formula: pre.clone(),
                    };

                    let moves = self.game_structure.move_count(*state);
                    let mut edges: HashSet<Edges<ATLVertex>, H> =
                        VarsIterator::new(moves, players.iter().copied().collect())
                            .map(|pmove| {
                                let mut targets: Vec<ATLVertex> =
                                    DeltaIterator::new(&self.game_structure, *state, &pmove)
                                        .map(|state| ATLVertex::FULL {
                                            state,
                                            formula: formula.clone(),
                                        })
                                        .collect();
                                targets.push(pre.clone());
                                Edges::HYPER(HyperEdge {
                                    source: vert.clone(),
                                    targets,
                                })
                            })
                            .collect();

                    // `until`-formula branch
                    // "Is the `until` formula satisfied now?"
                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets: vec![ATLVertex::FULL {
                            state: *state,
                            formula: until.clone(),
                        }],
                    }));

                    edges
                }
                Phi::DespiteEventually {
                    players,
                    formula: subformula,
                } => {
                    let mut edges = HashSet::with_hasher(H::default());

                    // Partial targets with same formula
                    // "Is the formula also satisfied in the next state?"
                    let moves = self.game_structure.move_count(*state);
                    let targets: Vec<ATLVertex> =
                        VarsIterator::new(moves, players.iter().cloned().collect())
                            .map(|pmove| ATLVertex::PARTIAL {
                                state: *state,
                                partial_move: pmove,
                                formula: formula.clone(),
                            })
                            .collect();
                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets,
                    }));

                    // sub-formula target
                    // "Is the sub formula satisfied in current state?"
                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets: vec![ATLVertex::FULL {
                            state: *state,
                            formula: subformula.clone(),
                        }],
                    }));

                    edges
                }
                Phi::EnforceEventually {
                    players,
                    formula: subformula,
                } => {
                    // Successor states with same formula
                    // "Is the formula also satisfied in the next state?"
                    let moves = self.game_structure.move_count(*state);
                    let mut edges: HashSet<Edges<ATLVertex>, H> =
                        VarsIterator::new(moves, players.iter().copied().collect())
                            .map(|pmove| {
                                let targets: Vec<ATLVertex> =
                                    DeltaIterator::new(&self.game_structure, *state, &pmove)
                                        .map(|state| ATLVertex::FULL {
                                            state,
                                            formula: formula.clone(),
                                        })
                                        .collect();
                                Edges::HYPER(HyperEdge {
                                    source: vert.clone(),
                                    targets,
                                })
                            })
                            .collect();

                    // sub-formula target
                    // "Is the sub formula satisfied in current state?"
                    edges.insert(Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets: vec![ATLVertex::FULL {
                            state: *state,
                            formula: subformula.clone(),
                        }],
                    }));

                    edges
                }
                Phi::DespiteInvariant {
                    players,
                    formula: subformula,
                } => {
                    let mut edges = HashSet::with_hasher(H::default());
                    edges.insert(Edges::NEGATION(NegationEdge {
                        source: vert.clone(),
                        target: ATLVertex::FULL {
                            state: *state,
                            formula: Arc::new(Phi::EnforceUntil {
                                players: players.clone(),
                                pre: Arc::new(Phi::True),
                                until: Arc::new(Phi::Not(subformula.clone())),
                            }),
                        },
                    }));
                    edges
                }
                Phi::EnforceInvariant {
                    players,
                    formula: subformula,
                } => {
                    let mut edges = HashSet::with_hasher(H::default());
                    edges.insert(Edges::NEGATION(NegationEdge {
                        source: vert.clone(),
                        target: ATLVertex::FULL {
                            state: *state,
                            formula: Arc::new(Phi::DespiteUntil {
                                players: players.clone(),
                                pre: Arc::new(Phi::True),
                                until: Arc::new(Phi::Not(subformula.clone())),
                            }),
                        },
                    }));
                    edges
                }
            },
            ATLVertex::PARTIAL {
                state,
                partial_move,
                formula,
            } => DeltaIterator::new(&self.game_structure, *state, partial_move)
                .map(|state| {
                    let targets = vec![ATLVertex::FULL {
                        state,
                        formula: formula.clone(),
                    }];
                    Edges::HYPER(HyperEdge {
                        source: vert.clone(),
                        targets,
                    })
                })
                .collect::<HashSet<Edges<ATLVertex>, H>>(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::atl::common::DynVec;
    use crate::atl::dependencygraph::{
        DeltaIterator, PartialMoveChoice, PartialMoveIterator, VarsIterator,
    };
    use crate::atl::gamestructure::EagerGameStructure;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn partial_move_iterator_01() {
        let partial_move = vec![
            PartialMoveChoice::RANGE(2),
            PartialMoveChoice::SPECIFIC(1),
            PartialMoveChoice::RANGE(2),
        ];

        let mut iter = PartialMoveIterator::new(&partial_move);
        assert_eq!(iter.next(), Some(vec![0, 1, 0]));
        assert_eq!(iter.next(), Some(vec![0, 1, 1]));
        assert_eq!(iter.next(), Some(vec![1, 1, 0]));
        assert_eq!(iter.next(), Some(vec![1, 1, 1]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn vars_iterator_01() {
        let moves = vec![2, 3, 2];
        let mut players = HashSet::new();
        players.insert(0);
        players.insert(2);
        let mut iter = VarsIterator::new(moves, players);

        assert_eq!(
            &iter.next(),
            &Some(vec![
                PartialMoveChoice::SPECIFIC(0),
                PartialMoveChoice::RANGE(3),
                PartialMoveChoice::SPECIFIC(0)
            ])
        );
        assert_eq!(
            &iter.next(),
            &Some(vec![
                PartialMoveChoice::SPECIFIC(1),
                PartialMoveChoice::RANGE(3),
                PartialMoveChoice::SPECIFIC(0)
            ])
        );
        assert_eq!(
            &iter.next(),
            &Some(vec![
                PartialMoveChoice::SPECIFIC(0),
                PartialMoveChoice::RANGE(3),
                PartialMoveChoice::SPECIFIC(1)
            ])
        );
        assert_eq!(
            &iter.next(),
            &Some(vec![
                PartialMoveChoice::SPECIFIC(1),
                PartialMoveChoice::RANGE(3),
                PartialMoveChoice::SPECIFIC(1)
            ])
        );
    }

    #[test]
    fn vars_iterator_02() {
        let mut players = HashSet::new();
        players.insert(2);
        let mut iter = VarsIterator::new(vec![2, 3, 3], players);

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::RANGE(2));
        assert_eq!(value[1], PartialMoveChoice::RANGE(3));
        assert_eq!(value[2], PartialMoveChoice::SPECIFIC(0));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::RANGE(2));
        assert_eq!(value[1], PartialMoveChoice::RANGE(3));
        assert_eq!(value[2], PartialMoveChoice::SPECIFIC(1));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::RANGE(2));
        assert_eq!(value[1], PartialMoveChoice::RANGE(3));
        assert_eq!(value[2], PartialMoveChoice::SPECIFIC(2));

        let value = iter.next();
        assert_eq!(value, None);
    }

    #[test]
    fn vars_iterator_03() {
        // Both players choose. So we should end up with every move vector
        let mut players = HashSet::new();
        players.insert(0);
        players.insert(1);
        let mut iter = VarsIterator::new(vec![3, 3], players);

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(0));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(0));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(1));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(0));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(2));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(0));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(0));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(1));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(1));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(1));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(2));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(1));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(0));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(2));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(1));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(2));

        let value = iter.next().unwrap();
        assert_eq!(value[0], PartialMoveChoice::SPECIFIC(2));
        assert_eq!(value[1], PartialMoveChoice::SPECIFIC(2));

        assert_eq!(iter.next(), None);
    }

    #[test]
    fn delta_iterator_01() {
        // player 0
        let transitions = DynVec::NEST(vec![
            // player 1
            Arc::new(DynVec::NEST(vec![
                // Player 2
                Arc::new(DynVec::NEST(vec![
                    // player 3
                    Arc::new(DynVec::NEST(vec![
                        // Player 4
                        Arc::new(DynVec::NEST(vec![Arc::new(DynVec::BASE(1))])),
                        // Player 4
                        Arc::new(DynVec::NEST(vec![Arc::new(DynVec::BASE(2))])),
                        // Player 4
                        Arc::new(DynVec::NEST(vec![Arc::new(DynVec::BASE(3))])),
                    ])),
                ])),
                // Player 2
                Arc::new(DynVec::NEST(vec![
                    // player 3
                    Arc::new(DynVec::NEST(vec![
                        // Player 4
                        Arc::new(DynVec::NEST(vec![Arc::new(DynVec::BASE(4))])),
                        // Player 4
                        Arc::new(DynVec::NEST(vec![Arc::new(DynVec::BASE(5))])),
                        // Player 4
                        Arc::new(DynVec::NEST(vec![Arc::new(DynVec::BASE(1))])),
                    ])),
                ])),
            ])),
        ]);
        let game_structure = EagerGameStructure {
            player_count: 5,
            labeling: vec![],
            transitions: vec![transitions],
            moves: vec![],
        };
        let state = 0;
        let partial_move = vec![
            PartialMoveChoice::SPECIFIC(0), // player 0
            PartialMoveChoice::RANGE(2),    // player 1
            PartialMoveChoice::SPECIFIC(0), // player 2
            PartialMoveChoice::RANGE(3),    // player 3
            PartialMoveChoice::SPECIFIC(0), // player 4
        ];
        let mut iter = DeltaIterator::new(&game_structure, state, &partial_move);

        let value = iter.next().unwrap();
        assert_eq!(value, 1);

        let value = iter.next().unwrap();
        assert_eq!(value, 2);

        let value = iter.next().unwrap();
        assert_eq!(value, 3);

        let value = iter.next().unwrap();
        assert_eq!(value, 4);

        let value = iter.next().unwrap();
        assert_eq!(value, 5);

        // repeats state 1 again, but that is suppressed due to deduplication of emitted states

        let value = iter.next();
        assert_eq!(value, None);
    }
}
