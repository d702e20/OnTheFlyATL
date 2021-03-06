use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Debug;
use std::fmt::Display;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, TryRecvError, TrySendError};

use crate::com::{Broker, ChannelBroker};
use crate::common::{Edges, HyperEdge, Message, NegationEdge, Token, VertexAssignment, WorkerId};
use std::cmp::max;
use tracing::{span, trace, Level};

// Based on the algorithm described in "Extended Dependency Graphs and Efficient Distributed Fixed-Point Computation" by A.E. Dalsgaard et al., 2017

pub trait Vertex: Hash + Eq + PartialEq + Clone + Display + Debug {}

pub trait ExtendedDependencyGraph<V: Vertex> {
    /// Return out going edges from `vertex`.
    /// This will be cached on each worker.
    fn succ(&self, vertex: &V) -> HashSet<Edges<V>>;
}

#[instrument]
pub fn distributed_certain_zero<
    G: ExtendedDependencyGraph<V> + Send + Sync + Clone + Debug + 'static,
    V: Vertex + Send + Sync + 'static,
>(
    edg: G,
    v0: V,
    worker_count: u64,
) -> VertexAssignment {
    trace!(?v0, worker_count, "starting distributed_certain_zero");

    // NOTE: 'static lifetime doesn't mean the full duration of the program execution
    let (broker, mut msg_rxs) = ChannelBroker::new(worker_count);
    // TODO make `Broker` responsible for concurrency, and remove the `Arc` wrapper
    let broker = Arc::new(broker);
    // Channel used for returning the final assigment of `v0` to the calling thread
    let (early_tx, early_rx) = crossbeam_channel::bounded(worker_count as usize);

    for i in (0..worker_count).rev() {
        let msg_rx = msg_rxs.pop().unwrap();
        let mut worker = Worker::new(
            i,
            worker_count,
            v0.clone(),
            msg_rx,
            broker.clone(),
            edg.clone(),
        );
        let tx = early_tx.clone();
        thread::spawn(move || {
            trace!("worker thread start");
            let result = worker.run();
            match tx.try_send(result) {
                Ok(_) => {}
                Err(err) => match err {
                    TrySendError::Full(_) => panic!(
                        "Failed to submit final assignment of v0 because the channel is full: {}",
                        err
                    ),
                    TrySendError::Disconnected(_) => {}
                },
            }
        });
    }

    let assignment = early_rx
        .recv()
        .expect("Error receiving final assigment from early termination");
    trace!(v0_assignment = ?assignment, "early termination");
    assignment
}

#[derive(Debug)]
struct Worker<B: Broker<V> + Debug, G: ExtendedDependencyGraph<V>, V: Vertex> {
    id: WorkerId,
    /// Number of workers working on solving the query. This is used as part of the static allocation scheme, see `crate::Worker::vertex_owner`.
    worker_count: u64,
    v0: V,
    assignment: HashMap<V, VertexAssignment>,
    depends: HashMap<V, HashSet<Edges<V>>>,
    /// Map of workers that need to be sent a message once the final assignment of a vertex is known.
    interests: HashMap<V, HashSet<WorkerId>>,
    /// Latest path of negation edges starting from v0 leading to the vertex.
    /// Example: If a path starting from v0, and that contains two negation edges, exists to the vertex the depth will be two.
    depth: HashMap<V, u32>,
    msg_rx: Receiver<Message<V>>,
    msg_queue: VecDeque<Message<V>>,
    hyper_queue: VecDeque<HyperEdge<V>>,
    negation_queue: VecDeque<NegationEdge<V>>,
    unsafe_edges: Vec<Vec<NegationEdge<V>>>,
    broker: Arc<B>,
    /// The logic of handling which edges have been deleted from a vertex is delegated to Worker instead of having to be duplicated in every implementation of ExtendedDependencyGraph.
    /// The first time succ is called on a vertex the call goes to the ExtendedDependencyGraph implementation, and the result is saved in successors.
    /// In all subsequent calls the vertex edges will be taken from the HashMap. This allows for modification of the output of the succ function.
    successors: HashMap<V, HashSet<Edges<V>>>,
    edg: G,
    counter: u32,
}

impl<B: Broker<V> + Debug, G: ExtendedDependencyGraph<V> + Send + Sync + Debug, V: Vertex>
    Worker<B, G, V>
{
    /// Determines which worker instance is responsible for computing the value of the vertex.
    /// Vertices are allocated to workers using a static allocation scheme. Dynamic addition and removal of workers isn't supported with this method.
    fn vertex_owner(&self, vertex: &V) -> WorkerId {
        // Static allocation of vertices to workers
        let mut hasher = DefaultHasher::new();
        vertex.hash::<DefaultHasher>(&mut hasher);
        let hash = hasher.finish();
        trace!(hash);
        hash % self.worker_count
    }

    /// Determines if `self` is responsible for computing the value of `vertex`
    #[inline]
    fn is_owner(&self, vertex: &V) -> bool {
        self.vertex_owner(vertex) == self.id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: WorkerId,
        worker_count: u64,
        v0: V,
        msg_rx: Receiver<Message<V>>,
        broker: Arc<B>,
        edg: G,
    ) -> Self {
        trace!(
            worker_id = id,
            worker_count,
            ?v0,
            "new distributed_certain_zero worker"
        );

        Self {
            id,
            worker_count,
            v0,
            assignment: HashMap::new(),
            depends: HashMap::<V, HashSet<Edges<V>>>::new(),
            interests: HashMap::<V, HashSet<WorkerId>>::new(),
            msg_rx,
            msg_queue: VecDeque::new(),
            hyper_queue: VecDeque::new(),
            negation_queue: VecDeque::new(),
            unsafe_edges: Vec::<Vec<NegationEdge<V>>>::new(),
            depth: HashMap::<V, u32>::new(),
            broker,
            successors: HashMap::<V, HashSet<Edges<V>>>::new(),
            edg,
            counter: 0,
        }
    }

    fn have_pending_work(&self) -> bool {
        self.msg_queue.is_empty()
    }

    fn have_pending_negation_edges(&self) -> bool {
        self.unsafe_edges.is_empty()
    }

    fn is_leader(&self) -> bool {
        self.id == 0
    }

    // TODO move msg_rx and term_rx argument from Worker::new to Worker::run
    pub fn run(&mut self) -> VertexAssignment {
        let span = span!(Level::DEBUG, "worker run", worker_id = self.id);
        let _enter = span.enter();
        trace!("worker start");

        // Alg 1, Line 2
        if self.is_owner(&self.v0.clone()) {
            trace!(worker_id = self.id, "exploring v0");
            self.explore(&self.v0.clone());
        }

        let msg_rx = self.msg_rx.clone();

        loop {
            // Pump all messages from broker to queues
            loop {
                match msg_rx.try_recv() {
                    Ok(msg) => match msg {
                        Message::REQUEST { .. } => self.msg_queue.push_back(msg),
                        Message::ANSWER { .. } => self.msg_queue.push_back(msg),
                        Message::TOKEN(_) => self.msg_queue.push_back(msg),
                        Message::RELEASE => self.msg_queue.push_back(msg),
                        Message::NEGATION(edge) => self.negation_queue.push_back(edge),
                        Message::HYPER(edge) => self.hyper_queue.push_back(edge),
                        Message::TERMINATE(assignment) => {
                            // Alg 1, Line 11-12
                            trace!(?assignment, "worker received termination");
                            return match assignment {
                                VertexAssignment::UNDECIDED => VertexAssignment::FALSE,
                                VertexAssignment::FALSE => VertexAssignment::FALSE,
                                VertexAssignment::TRUE => VertexAssignment::TRUE,
                            };
                        }
                    },
                    Err(err) => match err {
                        TryRecvError::Empty => break,
                        TryRecvError::Disconnected => {
                            panic!("worker receive channel disconnected unexpectedly: {}", err)
                        }
                    },
                }
            }

            if let Some(msg) = self.msg_queue.pop_back() {
                let _guard = span!(Level::TRACE, "worker receive message", worker_id = self.id);
                self.counter = 0;
                match msg {
                    // Alg 1, Line 8
                    Message::REQUEST {
                        vertex,
                        depth,
                        worker_id,
                    } => self.process_request(&vertex, worker_id, depth),
                    // Alg 1, Line 9
                    Message::ANSWER { vertex, assignment } => {
                        self.process_answer(&vertex, assignment)
                    }
                    Message::TOKEN(token) => {
                        // TODO check that the token ring logic works correctly with only a single worker
                        match token {
                            Token::Clean => {
                                if self.id == 0 {
                                    // Late termination
                                    self.broker.terminate(VertexAssignment::FALSE)
                                } else {
                                    if self.dirty {
                                        todo!("check if the token should be changed to Dirty or HaveNegations")
                                    } else {
                                        // Forward the Token::Clean
                                        self.broker.send(
                                            (self.id + 1) % self.worker_count,
                                            Message::TOKEN(token),
                                        )
                                    }
                                }
                            }
                            Token::HaveNegations => {
                                if self.id == 0 {
                                    for worker_id in 0..self.worker_count {
                                        self.broker.send(worker_id, Message::RELEASE)
                                    }
                                } else {
                                    self.broker.send(
                                        (self.id + 1) % self.worker_count,
                                        Message::TOKEN(token),
                                    )
                                }
                            }
                            Token::Dirty => {
                                if self.id == 0 {
                                    // no-op, other workers are busy
                                } else {
                                    todo!("check if Token::Dirty need to be upgraded to Token::HaveNegations");
                                    self.broker.send(
                                        (self.id + 1) % self.worker_count,
                                        Message::TOKEN(token),
                                    )
                                }
                            }
                        }
                    }
                    Message::RELEASE => {
                        todo!("release negation")
                    }
                    Message::NEGATION(_) => unreachable!(),
                    Message::HYPER(_) => unreachable!(),
                    Message::TERMINATE(_) => unreachable!(),
                }
            } else if let Some(edge) = self.hyper_queue.pop_front() {
                let _guard = span!(
                    Level::TRACE,
                    "worker receive hyper-edge",
                    worker_id = self.id,
                    ?edge,
                );
                self.process_hyper_edge(edge)
            } else if let Some(edge) = self.negation_queue.pop_front() {
                let _guard = span!(
                    Level::TRACE,
                    "worker receive negation-edge",
                    worker_id = self.id,
                    ?edge,
                );
                match self.assignment.get(&edge.target) {
                    None => self.process_negation_edge(edge.clone()),
                    // Alg 1, Line 7
                    Some(assignment) => match assignment {
                        VertexAssignment::FALSE => {
                            self.counter = 0;
                            self.process_negation_edge(edge.clone())
                        }
                        VertexAssignment::TRUE => {
                            self.counter = 0;
                            self.process_negation_edge(edge.clone())
                        }
                        // In case of undecided the edge is only processed after 10 iterations
                        VertexAssignment::UNDECIDED => {
                            if self.counter == 9 {
                                self.counter = 0;
                                self.process_negation_edge(edge.clone())
                            } else {
                                trace!(?edge, "queueing negation");
                                self.broker.queue_negation(self.id, edge.clone());
                                self.counter += 1;
                            }
                        }
                    },
                }
            } else {
                let _guard = span!(Level::TRACE, "worker release negation", worker_id = self.id);
                // if the negation channel is empty, unsafe negation edges are released
                self.release_negations(self.unsafe_edges.len());
            }
        }
    }

    /// Releasing the edges from the unsafe queue to the safe negation channel
    fn release_negations(&mut self) {
        trace!(depth = self.unsafe_edges.len(), "release negation");

        if let Some(edges) = self.unsafe_edges.pop() {
            // Queue all edges in the negation channel that have the given depth
            for edge in edges {
                trace!(?edge, "release negation edge");
                self.broker.queue_negation(self.id, edge);
            }
        }
    }
    /// Adds worker to C^i(vertex)
    fn mark_interest(&mut self, vertex: &V, worker: WorkerId) {
        if let Some(set) = self.interests.get_mut(vertex) {
            trace!(is_initialized = true, ?vertex, "mark vertex interest");
            set.insert(worker);
        } else {
            trace!(is_initialized = false, ?vertex, "mark vertex interest");
            let mut set = HashSet::new();
            set.insert(worker);
            self.interests.insert(vertex.clone(), set);
        }
    }

    fn explore(&mut self, vertex: &V) {
        trace!(?vertex, "exploring vertex");
        // Line 2
        self.assignment
            .insert(vertex.clone(), VertexAssignment::UNDECIDED);

        // Line 3
        if self.is_owner(vertex) {
            let successors = self.succ(vertex); // Line 4
            if successors.is_empty() {
                // Line 4
                self.final_assign(vertex, VertexAssignment::FALSE);
            } else {
                // Line 5
                for edge in successors.iter() {
                    match edge {
                        Edges::HYPER(edge) => self.broker.queue_hyper(self.id, edge.clone()),
                        Edges::NEGATION(edge) => self.queue_negation(edge.clone()),
                    }
                }
            }
        } else {
            // Line 7
            self.broker.send(
                self.vertex_owner(vertex),
                Message::REQUEST {
                    vertex: vertex.clone(),
                    depth: *self.depth.get(vertex).unwrap_or(&0),
                    worker_id: self.id,
                },
            )
        }
    }

    fn process_hyper_edge(&mut self, edge: HyperEdge<V>) {
        trace!(?edge, "processing hyper-edge");
        // Line 3, condition (in case of targets is empty, the default value is true)
        let all_final = edge.targets.iter().all(|target| {
            self.assignment
                .get(target)
                .map_or(false, |f| matches!(f, VertexAssignment::TRUE))
        });

        // Line 3
        if all_final {
            self.final_assign(&edge.source, VertexAssignment::TRUE);
            return;
        }

        // Line 4, condition
        let any_target = edge.targets.iter().any(|target| {
            self.assignment
                .get(target)
                .map_or(false, |f| matches!(f, VertexAssignment::FALSE))
        });

        // Line 4
        if any_target {
            self.delete_edge(Edges::HYPER(edge));
            return;
        }

        // Line 5-8
        for target in &edge.targets {
            // Line 5 condition
            match self.assignment.get(&target) {
                Some(VertexAssignment::UNDECIDED) => {
                    // UNDECIDED
                    // Line 7
                    self.add_depend(target, Edges::HYPER(edge.clone()));
                }
                None => {
                    // UNEXPLORED
                    // Line 7
                    self.add_depend(target, Edges::HYPER(edge.clone()));
                    // Line 8
                    self.explore(target);
                }
                _ => {}
            }
        }
    }

    // Mark `dependency` as a prerequisite for finding the final assignment of `vertex`
    fn add_depend(&mut self, vertex: &V, dependency: Edges<V>) {
        // Update the depth
        let default_depth = 0;
        let vertex_depth = *self.depth.get(vertex).unwrap_or(&default_depth);
        match dependency.clone() {
            Edges::NEGATION(edge) => {
                let source_depth = self.depth.get(&edge.source).unwrap_or_else(|| {
                    debug!(
                        ?edge,
                        worker_id = self.id,
                        "Assigned default depth to edge because source edge depth is unknown",
                    );
                    &default_depth
                }) + 1;
                self.depth
                    .insert(vertex.clone(), max(source_depth, vertex_depth));
            }
            Edges::HYPER(edge) => {
                let source_depth = *self.depth.get(&edge.source).unwrap_or_else(|| {
                    trace!(
                        ?edge,
                        worker_id = self.id,
                        "Assigned default depth to edge because source edge depth is unknown"
                    );
                    &default_depth
                });
                self.depth
                    .insert(vertex.clone(), max(source_depth, vertex_depth));
            }
        }

        // Mark `dependency` as a prerequisite for finding the final assignment of `vertex`
        if let Some(dependencies) = self.depends.get_mut(vertex) {
            dependencies.insert(dependency);
        } else {
            let mut dependencies = HashSet::new();
            dependencies.insert(dependency);
            self.depends.insert(vertex.clone(), dependencies);
        }
    }

    // Remove `dependency` as a prerequisite for finding the final assignment of `vertex`
    fn remove_depend(&mut self, vertex: &V, dependency: Edges<V>) {
        if let Some(dependencies) = self.depends.get_mut(vertex) {
            dependencies.remove(&dependency);
        }
    }

    fn process_negation_edge(&mut self, edge: NegationEdge<V>) {
        match self.assignment.get(&edge.target) {
            // UNEXPLORED
            None => {
                // UNEXPLORED
                // Line 6
                trace!(?edge, assignment = "UNEXPLORED", "processing negation edge");
                self.add_depend(&edge.target, Edges::NEGATION(edge.clone()));
                self.queue_negation(edge.clone());
                self.explore(&edge.target);
            }
            Some(assignment) => match assignment {
                VertexAssignment::UNDECIDED => {
                    trace!(?edge, assignment = ?VertexAssignment::UNDECIDED, "processing negation edge");
                    self.final_assign(&edge.source, VertexAssignment::TRUE)
                }
                VertexAssignment::FALSE => {
                    trace!(?edge, assignment = ?VertexAssignment::FALSE, "processing negation edge");
                    self.final_assign(&edge.source, VertexAssignment::TRUE)
                }
                VertexAssignment::TRUE => {
                    trace!(?edge, assignment = ?VertexAssignment::TRUE, "processing negation edge");
                    self.delete_edge(Edges::NEGATION(edge))
                }
            },
        }
    }

    /// Queueing unsafe negation, which will be queued to negation channel whenever
    /// release negation is called
    fn queue_negation(&mut self, edge: NegationEdge<V>) {
        let len = self.unsafe_edges.len();
        let mut depth: usize = 0;
        if let Some(n) = self.depth.get(&edge.source) {
            depth = *n as usize;
        }

        if len <= depth {
            for _ in len..(depth + 1) {
                self.unsafe_edges.push(Vec::new());
            }
        }

        self.unsafe_edges
            .get_mut(depth as usize)
            .unwrap()
            .push(edge);
    }

    // Another worker has requested the final assignment of a `vertex`
    fn process_request(&mut self, vertex: &V, requester: WorkerId, depth: u32) {
        trace!(
            ?vertex,
            ?requester,
            depth,
            "got request for vertex assignment"
        );
        if let Some(assigned) = self.assignment.get(&vertex) {
            // Final assignment of `vertex` is already known, reply immediately
            match assigned {
                VertexAssignment::FALSE => {
                    return self.broker.send(
                        requester,
                        Message::ANSWER {
                            vertex: vertex.clone(),
                            assignment: VertexAssignment::FALSE,
                        },
                    );
                }
                VertexAssignment::TRUE => {
                    return self.broker.send(
                        requester,
                        Message::ANSWER {
                            vertex: vertex.clone(),
                            assignment: VertexAssignment::TRUE,
                        },
                    );
                }
                _ => {
                    // update depth
                    let local_depth = *self.depth.get(vertex).unwrap_or(&0);
                    self.depth.insert(vertex.clone(), max(local_depth, depth));

                    self.mark_interest(vertex, requester);

                    if self.depends.contains_key(vertex) {
                        if let Some(assignment) = self.assignment.get(vertex) {
                            if let VertexAssignment::UNDECIDED = assignment {
                                self.explore(vertex)
                            }
                        }
                    }
                }
            }
        }
        // Final assignment of `vertex` is not yet known
        self.mark_interest(vertex, requester);
        if self.assignment.get(&vertex).is_none() {
            // UNEXPLORED
            self.explore(&vertex);
        }
    }

    fn process_answer(&mut self, vertex: &V, assigned: VertexAssignment) {
        trace!(?vertex, ?assigned, "received final assignment");
        self.final_assign(vertex, assigned);
    }

    fn final_assign(&mut self, vertex: &V, assignment: VertexAssignment) {
        // Line 2
        if *vertex == self.v0 {
            self.broker.terminate(assignment);
            // Don't bother returning the weight, this is early termination
            return;
        }
        // Line 3
        let prev_assignment = self.assignment.insert(vertex.clone(), assignment);
        let changed_assignment = prev_assignment != Some(assignment);
        debug!(
            ?prev_assignment,
            new_assignment = ?assignment,
            ?vertex,
            final_again =
                !(prev_assignment == Some(VertexAssignment::UNDECIDED) || prev_assignment == None),
            changed_assignment,
            "final assigned"
        );

        if changed_assignment {
            // Line 4
            if let Some(interested) = self.interests.get(&vertex) {
                for worker_id in interested {
                    self.broker.send(
                        *worker_id,
                        Message::ANSWER {
                            vertex: vertex.clone(),
                            assignment,
                        },
                    )
                }
            }

            // Line 5
            if let Some(depends) = self.depends.get(&vertex) {
                for edge in depends.clone() {
                    trace!(
                        ?edge,
                        "requeueing edg because edge have received final assignment"
                    );
                    match edge {
                        Edges::HYPER(edge) => self.broker.queue_hyper(self.id, edge.clone()),
                        Edges::NEGATION(edge) => self.queue_negation(edge.clone()),
                    }
                }
            }
        }
    }

    /// Helper function for deleting edges from a vertex.
    fn delete_edge(&mut self, edge: Edges<V>) {
        // Get v
        let source = match edge {
            Edges::HYPER(ref edge) => edge.source.clone(),
            Edges::NEGATION(ref edge) => edge.source.clone(),
        };

        // Initializes the successors hashmap for key source
        if let Some(successors) = self.successors.get_mut(&source) {
            successors.remove(&edge);
        } else {
            let mut successors = self.edg.succ(&source);
            trace!(?successors, remove_edge = ?edge, "initializing successors hashmap in edg::delete_edge");
            successors.remove(&edge);
            self.successors.insert(source.clone(), successors);
        }

        match self.successors.get(&source) {
            None => panic!("successors should have been filled, or at least have a empty vector"),
            Some(successors) => {
                // Line 3
                if successors.is_empty() {
                    trace!(
                        ?source,
                        assignment = ?VertexAssignment::FALSE,
                        "no more successors, final assignment is FALSE"
                    );
                    self.final_assign(&source, VertexAssignment::FALSE);
                }
            }
        }

        match edge {
            // Line 4-6
            Edges::HYPER(ref edge) => {
                debug!(source = ?edge, targets = ?edge.targets, "remove hyper-edge as dependency");
                for target in &edge.targets {
                    self.remove_depend(target, Edges::HYPER(edge.clone()))
                }
            }
            // Line 7-8
            Edges::NEGATION(ref edge) => {
                debug!(source = ?edge, target = ?edge.target, "remove negation-edge as dependency");
                self.remove_depend(&edge.target, Edges::NEGATION(edge.clone()))
            }
        }
    }

    /// Wraps the ExtendedDependencyGraph::succ(v) with caching allowing edges to be deleted.
    /// See documentation for the `successors` field.
    fn succ(&mut self, vertex: &V) -> HashSet<Edges<V>> {
        if let Some(successors) = self.successors.get(vertex) {
            debug!(?vertex, ?successors, known_vertex = true, "edg::succ");
            // List of successors is already allocated for the vertex
            successors.clone()
        } else {
            // Setup the successors list the first time it is requested
            let successors = self.edg.succ(vertex);
            self.successors.insert(vertex.clone(), successors.clone());
            debug!(
                ?vertex,
                ?successors,
                known_vertex = false,
                "loaded successors from EDG"
            );
            successors
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::hash_map::RandomState;
    use std::collections::HashSet;
    use std::fmt::Display;
    use test_env_log::test;

    use core::fmt::Formatter;

    use crate::common::{Edges, HyperEdge, NegationEdge, VertexAssignment};
    use crate::edg::{distributed_certain_zero, ExtendedDependencyGraph, Vertex};

    const WORKER_COUNT: u64 = 1;

    #[derive(Hash, Clone, Eq, PartialEq, Debug)]
    struct ExampleEDG {}

    #[test]
    fn test_empty_hyper_edge() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![],
                        }));

                        successors
                    }
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex A"
        );
    }

    #[test]
    fn test_no_successors() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => HashSet::new(),
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex A"
        );
    }

    #[test]
    fn test_1() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
            B,
            C,
            D,
            E,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![ExampleEDGVertices::B, ExampleEDGVertices::C],
                        }));
                        successors
                    }
                    ExampleEDGVertices::B => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::NEGATION(NegationEdge {
                            source: ExampleEDGVertices::B,
                            target: ExampleEDGVertices::E,
                        }));

                        successors
                    }
                    ExampleEDGVertices::C => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::C,
                            targets: vec![],
                        }));

                        successors
                    }
                    ExampleEDGVertices::D => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::D,
                            targets: vec![],
                        }));

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::D,
                            targets: vec![ExampleEDGVertices::C],
                        }));

                        successors
                    }
                    ExampleEDGVertices::E => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::NEGATION(NegationEdge {
                            source: ExampleEDGVertices::E,
                            target: ExampleEDGVertices::D,
                        }));

                        successors
                    }
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex A"
        );
    }

    #[test]
    fn test_with_edg() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
            B,
            C,
            D,
            E,
            F,
            N,
            T,
            G,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                // A -> B
                // A -> E
                // B -> C
                // C -> F
                // C -> T
                // D -> E
                // D -> C
                // E -> (D, F)
                // F -> Ø
                // N ..> A
                // T -> G
                // G

                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![ExampleEDGVertices::B],
                        }));

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![ExampleEDGVertices::E],
                        }));

                        successors
                    }
                    ExampleEDGVertices::B => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::B,
                            targets: vec![ExampleEDGVertices::C],
                        }));

                        successors
                    }
                    ExampleEDGVertices::C => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::C,
                            targets: vec![ExampleEDGVertices::F],
                        }));

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::C,
                            targets: vec![ExampleEDGVertices::T],
                        }));

                        successors
                    }
                    ExampleEDGVertices::D => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::D,
                            targets: vec![ExampleEDGVertices::E],
                        }));

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::D,
                            targets: vec![ExampleEDGVertices::C],
                        }));

                        successors
                    }
                    ExampleEDGVertices::E => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::E,
                            targets: vec![ExampleEDGVertices::D, ExampleEDGVertices::F],
                        }));

                        successors
                    }
                    ExampleEDGVertices::F => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::F,
                            targets: vec![],
                        }));

                        successors
                    }
                    ExampleEDGVertices::N => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::NEGATION(NegationEdge {
                            source: ExampleEDGVertices::N,
                            target: ExampleEDGVertices::A,
                        }));

                        successors
                    }
                    ExampleEDGVertices::T => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::T,
                            targets: vec![ExampleEDGVertices::G],
                        }));

                        successors
                    }
                    ExampleEDGVertices::G => HashSet::new(),
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex A"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::B, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex B"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::C, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex C"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::D, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex D"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::E, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex E"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::F, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex F"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::T, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex T"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::G, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex G"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::N, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex N"
        );
    }

    #[test]
    //#[ignore]
    fn test_small_dg_all_true_except_for_c() {
        warn!("foo bar");
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
            B,
            C,
            D,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                // A -> B
                // A -> C
                // B -> D
                // C
                // D -> Ø

                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![ExampleEDGVertices::B],
                        }));

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![ExampleEDGVertices::C],
                        }));

                        successors
                    }
                    ExampleEDGVertices::B => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::B,
                            targets: vec![ExampleEDGVertices::D],
                        }));

                        successors
                    }
                    ExampleEDGVertices::C => {
                        // No successors
                        HashSet::new()
                    }
                    ExampleEDGVertices::D => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::D,
                            targets: vec![],
                        }));

                        successors
                    }
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex A"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::B, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex B"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::C, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex C"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::D, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex D"
        );
    }

    #[test]
    //#[ignore]
    fn test_small_dg_all_false_except_for_d() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
            B,
            C,
            D,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                // A -> B
                // B -> C
                // C -> B
                // D -> Ø

                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![ExampleEDGVertices::B],
                        }));

                        successors
                    }
                    ExampleEDGVertices::B => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::B,
                            targets: vec![ExampleEDGVertices::C],
                        }));

                        successors
                    }
                    ExampleEDGVertices::C => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::C,
                            targets: vec![ExampleEDGVertices::B],
                        }));

                        successors
                    }
                    ExampleEDGVertices::D => {
                        let mut successors = HashSet::new();
                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::D,
                            targets: vec![],
                        }));

                        successors
                    }
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex A"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::B, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex B"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::C, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex C"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::D, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex D"
        );
    }

    #[test]
    //#[ignore]
    fn test_a_node_with_no_succsessors() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        // No successors
                        HashSet::new()
                    }
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex A"
        );
    }

    #[test]
    //#[ignore]
    fn test_termination_condtion() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
            B,
            C,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![ExampleEDGVertices::B, ExampleEDGVertices::C],
                        }));

                        successors
                    }
                    ExampleEDGVertices::B => {
                        // No successors
                        HashSet::new()
                    }
                    ExampleEDGVertices::C => {
                        // No successors
                        HashSet::new()
                    }
                }
            }
        }
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex A"
        );
        //assert_eq!(distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::B, WORKER_COUNT), VertexAssignment::FALSE, "Vertex B");
        //assert_eq!(distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::C, WORKER_COUNT), VertexAssignment::FALSE, "Vertex C");
    }

    #[test]
    //#[ignore]
    fn test_loop_di_loops() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
            B,
            C,
            D,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            // A -> B
            // B -> (A, C)
            // C -> D
            // D -> Ø

            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::A,
                            targets: vec![ExampleEDGVertices::B],
                        }));

                        successors
                    }
                    ExampleEDGVertices::B => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::B,
                            targets: vec![ExampleEDGVertices::A, ExampleEDGVertices::C],
                        }));

                        successors
                    }
                    ExampleEDGVertices::C => {
                        let mut successors = HashSet::new();
                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::C,
                            targets: vec![ExampleEDGVertices::D],
                        }));

                        successors
                    }
                    ExampleEDGVertices::D => {
                        let mut successors = HashSet::new();
                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::D,
                            targets: vec![],
                        }));

                        successors
                    }
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex A"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::B, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex B"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::C, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex C"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::D, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex D"
        );
    }

    #[test]
    #[ignore]
    fn test_negation_edges() {
        #[derive(Hash, Clone, Eq, PartialEq, Debug)]
        enum ExampleEDGVertices {
            A,
            B,
        }

        impl Display for ExampleEDGVertices {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }

        impl Vertex for ExampleEDGVertices {}

        impl ExtendedDependencyGraph<ExampleEDGVertices> for ExampleEDG {
            // A ..> B
            // B -> Ø

            fn succ(
                &self,
                vertex: &ExampleEDGVertices,
            ) -> HashSet<Edges<ExampleEDGVertices>, RandomState> {
                debug!(?vertex, "edg succ");
                match vertex {
                    ExampleEDGVertices::A => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::NEGATION(NegationEdge {
                            source: ExampleEDGVertices::A,
                            target: ExampleEDGVertices::B,
                        }));

                        successors
                    }
                    ExampleEDGVertices::B => {
                        let mut successors = HashSet::new();

                        successors.insert(Edges::HYPER(HyperEdge {
                            source: ExampleEDGVertices::B,
                            targets: vec![],
                        }));

                        successors
                    }
                }
            }
        }

        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::A, WORKER_COUNT),
            VertexAssignment::FALSE,
            "Vertex A"
        );
        assert_eq!(
            distributed_certain_zero(ExampleEDG {}, ExampleEDGVertices::B, WORKER_COUNT),
            VertexAssignment::TRUE,
            "Vertex B"
        );
    }
}
