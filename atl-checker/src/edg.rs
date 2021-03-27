use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Debug;
use std::fmt::Display;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, TryRecvError};

use crate::com::{Broker, ChannelBroker};
use crate::common::{
    Edges, HyperEdge, Message, MsgToken, NegationEdge, Token, VertexAssignment, WorkerId,
};
use crate::hasher::EdgHasher;
use std::cmp::max;
use std::thread::sleep;
use std::time::Duration;
use tracing::{span, trace, Level};

// Based on the algorithm described in "Extended Dependency Graphs and Efficient Distributed Fixed-Point Computation" by A.E. Dalsgaard et al., 2017

pub trait Vertex: Hash + Eq + PartialEq + Clone + Display + Debug {}

pub trait ExtendedDependencyGraph<V: Vertex, H: BuildHasher + Clone + Default = EdgHasher> {
    /// Return out going edges from `vertex`.
    /// This will be cached on each worker.
    fn succ(&self, vertex: &V) -> HashSet<Edges<V>, H>;
}

#[instrument]
pub fn distributed_certain_zero<
    G: ExtendedDependencyGraph<V, EdgHasher> + Send + Sync + Clone + Debug + 'static,
    V: Vertex + Send + Sync + 'static,
>(
    edg: G,
    v0: V,
    worker_count: u64,
) -> VertexAssignment {
    distributed_certain_zero_with_hasher::<_, _, EdgHasher>(edg, v0, worker_count)
}

pub fn distributed_certain_zero_with_hasher<
    G: ExtendedDependencyGraph<V, H> + Send + Sync + Clone + Debug + 'static,
    V: Vertex + Send + Sync + 'static,
    H: BuildHasher + Clone + Default,
>(
    edg: G,
    v0: V,
    worker_count: u64,
) -> VertexAssignment {
    trace!(?v0, worker_count, "starting distributed_certain_zero");

    let (broker, mut msg_rxs, result_rx) = ChannelBroker::new(worker_count);
    // TODO make `Broker` responsible for concurrency, and remove the `Arc` wrapper
    let broker = Arc::new(broker);

    msg_rxs.drain(..).enumerate().for_each(|(i, msg_rx)| {
        let edg = edg.clone();
        let broker = broker.clone();
        let v0 = v0.clone();
        thread::spawn(move || {
            let mut worker = Worker::new(i as u64, worker_count, v0, msg_rx, broker, edg);
            trace!("worker thread start");
            worker.run();
        });
    });

    let assignment = result_rx
        .recv()
        .expect("Error receiving final assigment on termination");
    broker.terminate();
    trace!(v0_assignment = ?assignment, "Found assignment of v0");
    assignment
}

#[derive(Debug)]
struct Worker<
    B: Broker<V> + Debug,
    G: ExtendedDependencyGraph<V, H>,
    V: Vertex,
    H: BuildHasher + Clone + Default = EdgHasher,
> {
    id: WorkerId,
    /// Number of workers working on solving the query. This is used as part of the static allocation scheme, see `crate::Worker::vertex_owner`.
    worker_count: u64,
    /// The vertex that the worker is attempting to find the assignment of.
    v0: V,
    /// When this flags becomes false, the worker will end its loop and terminate
    running: bool,
    /// Greatest known assignment for vertices.
    /// Order is as follow UNEXPLORED/None < UNDECIDED < {TRUE, FALSE}. Once a vertex has been assigned TRUE or FALSE it will never change assignment.
    assignment: HashMap<V, VertexAssignment, H>,
    depends: HashMap<V, HashSet<Edges<V>, H>, H>,
    /// Map of workers that need to be sent a message once the final assignment of a vertex is known.
    interests: HashMap<V, HashSet<WorkerId, H>, H>,
    /// Greatest number of negation edges in any path from v0 to the given vertex.
    /// Example: If there exists a path from v0 to a vertex, which contains two negation edges, then depth will be two (or more if there is a path with more negation edges).
    depth: HashMap<V, u32, H>,
    msg_rx: Receiver<Message<V>>,
    msg_queue: VecDeque<Message<V>>,
    hyper_queue: VecDeque<HyperEdge<V>>,
    negation_queue: VecDeque<NegationEdge<V>>,
    unsafe_neg_edges: Vec<Vec<NegationEdge<V>>>,
    /// Used to communicate with other workers
    broker: Arc<B>,
    /// The logic of handling which edges have been deleted from a vertex is delegated to Worker instead of having to be duplicated in every implementation of ExtendedDependencyGraph.
    /// The first time succ is called on a vertex the call goes to the ExtendedDependencyGraph implementation, and the result is saved in successors.
    /// In all subsequent calls the vertex edges will be taken from the HashMap. This allows for modification of the output of the succ function.
    successors: HashMap<V, HashSet<Edges<V>, H>, H>,
    edg: G,
    /// Used on the leader as a gate to avoid starting a round of the token ring if one is already in progress.
    token_in_circulation: bool,
    /// A flag to keep track of whether or not this worker has seen safe work since last time it
    /// saw the termination token
    dirty: bool,
}

impl<
        B: Broker<V> + Debug,
        G: ExtendedDependencyGraph<V, H> + Send + Sync + Debug,
        V: Vertex,
        H: BuildHasher + Clone + Default,
    > Worker<B, G, V, H>
{
    /// Determines which worker instance is responsible for computing the value of the vertex.
    /// Vertices are allocated to workers using a static allocation scheme. Dynamic addition and removal of workers isn't supported with this method.
    fn vertex_owner(&self, vertex: &V) -> WorkerId {
        // Static allocation of vertices to workers

        // BUG: H does not hash to the same value across all workers
        //let mut hasher = H::default().build_hasher();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        vertex.hash(&mut hasher);
        let hash = hasher.finish();
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
            running: true,
            assignment: HashMap::with_hasher(H::default()),
            depends: HashMap::<V, HashSet<Edges<V>, H>, H>::with_hasher(H::default()),
            interests: HashMap::<V, HashSet<WorkerId, H>, H>::with_hasher(H::default()),
            msg_rx,
            msg_queue: VecDeque::new(),
            hyper_queue: VecDeque::new(),
            negation_queue: VecDeque::new(),
            unsafe_neg_edges: Vec::<Vec<NegationEdge<V>>>::new(),
            depth: HashMap::<V, u32, H>::with_hasher(H::default()),
            broker,
            successors: HashMap::<V, HashSet<Edges<V>, H>, H>::with_hasher(H::default()),
            edg,
            token_in_circulation: false,
            dirty: false,
        }
    }

    fn has_seen_dirty_work(&self) -> bool {
        self.dirty || !self.msg_queue.is_empty()
    }

    fn has_unsafe_negation_edges(&self) -> bool {
        !self.unsafe_neg_edges.is_empty()
    }

    /// Is this worker the leader of the token ring
    fn is_leader(&self) -> bool {
        self.id == 0
    }

    fn get_depth_of_deepest_component(&self) -> usize {
        self.unsafe_neg_edges.len()
    }

    /// Determine the token value of this worker, and forward either the local token value or
    /// the received token depending on which is greater. This function should never be called
    /// by the leader.
    fn update_and_forward_token(&self, msg: MsgToken) {
        let local_token_value = if self.has_seen_dirty_work() {
            Token::Dirty
        } else if self.has_unsafe_negation_edges() {
            Token::HaveNegations
        } else {
            Token::Clean
        };

        let successor = (self.id + 1) % self.worker_count;
        let token = MsgToken {
            token: max(msg.token, local_token_value),
            deepest_component: max(msg.deepest_component, self.get_depth_of_deepest_component()),
        };

        self.broker.send(successor, Message::TOKEN(token))
    }

    pub fn run(&mut self) {
        let span = span!(Level::DEBUG, "worker run", worker_id = self.id);
        let _enter = span.enter();
        trace!("worker start");
        #[cfg(feature = "use-counts")]
        eprintln!("worker start");

        // Alg 1, Line 2
        // The owner of v0 starts by exploring it
        if self.is_owner(&self.v0.clone()) {
            trace!(worker_id = self.id, "exploring v0");
            self.explore(&self.v0.clone());
            self.dirty = true;
        }

        while self.running {
            // Receive incoming tasks and terminate if requested
            self.recv_all_and_fill_queues();

            if self.process_task() {
                continue; // We did a task
            }

            if self.is_leader() && !self.token_in_circulation {
                // We are out of safe tasks so consider termination
                self.initiate_potential_termination();
            } else {
                // Idle
                sleep(Duration::from_millis(1))
            }
        }
    }

    /// Receive all messages from the broker and put them into the right queues. This function
    /// will return a VertexAssignment, if the worker has been signaled to terminate. The
    /// assignment is then the assignment of the root.
    fn recv_all_and_fill_queues(&mut self) {
        // Pump all messages from broker to msg queue. Terminate messages are handled immediately.
        loop {
            match self.msg_rx.try_recv() {
                Ok(msg) => match msg {
                    Message::TERMINATE => {
                        #[cfg(feature = "use-counts")]
                        eprintln!("worker received_termination");
                        self.running = false;
                    }
                    _ => {
                        #[cfg(feature = "use-counts")]
                        eprintln!("worker receive_message");
                        self.msg_queue.push_back(msg)
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
    }

    /// Handle an incoming token.
    ///
    /// If this worker is the leader, it will decide the state of
    /// the algorithm. Termination begins if no worker has seen safe work since last
    /// synchronization. If some workers have unsafe negation edges, the negation edges of the
    /// deepest component will be released instead.
    ///
    /// If this worker is not the leader, it will upgrade the token, if needed, and forward it.
    fn handle_incoming_token(&mut self, token: MsgToken) {
        #[cfg(feature = "use-counts")]
        eprintln!("worker handle_token");
        if self.is_leader() {
            // The token has returned to the leader
            self.token_in_circulation = false;
            match token {
                // The token made it all the way without getting dirty
                // That means there are no more tasks and we can terminate
                MsgToken {
                    token: Token::Clean,
                    deepest_component: _,
                } => {
                    trace!("Late termination");
                    self.broker.return_result(VertexAssignment::FALSE)
                }
                // No one has seen safe tasks, but some workers have unsafe negation edges.
                MsgToken {
                    token: Token::HaveNegations,
                    deepest_component,
                } => {
                    // Tell everyone to release negation edges of deepest component
                    trace!(
                        depth = deepest_component,
                        "sending release component message"
                    );
                    #[cfg(feature = "use-counts")]
                    eprintln!("worker send_release_token");
                    self.broker.release(deepest_component);
                }
                // Some workers still have safe tasks, so we can't terminate yet
                _ => {
                    // no-op, other workers are busy
                    trace!("leader received Token::Dirty")
                }
            }
        } else {
            self.update_and_forward_token(token);
            self.dirty = false;
        }
    }

    /// Initiate a synchronization with the other workers to determine if it is time to
    /// terminate. Only the leader can initiate termination and the token can't be in
    /// circulation already.
    fn initiate_potential_termination(&mut self) {
        debug_assert!(self.is_leader(), "Only the leader can initiate termination");
        debug_assert!(
            !self.token_in_circulation,
            "Token is already in circulation"
        );

        // What token to send
        let token = if self.has_unsafe_negation_edges() {
            Token::HaveNegations
        } else {
            Token::Clean
        };

        debug!(?token, "starting token ring round");
        #[cfg(feature = "use-counts")]
        eprintln!("worker initiate_token_circulation");
        self.broker.send(
            (self.id + 1) % self.worker_count,
            Message::TOKEN(MsgToken {
                token,
                deepest_component: self.get_depth_of_deepest_component(),
            }),
        );

        self.token_in_circulation = true;
        self.dirty = false;
    }

    /// Process the next task. This returns true if any task was processed.
    fn process_task(&mut self) -> bool {
        if let Some(msg) = self.msg_queue.pop_front() {
            let _guard = span!(Level::TRACE, "worker receive message", worker_id = self.id);
            match msg {
                // Alg 1, Line 8
                Message::REQUEST {
                    vertex,
                    depth,
                    worker_id,
                } => self.process_request(&vertex, worker_id, depth),
                // Alg 1, Line 9
                Message::ANSWER { vertex, assignment } => self.process_answer(&vertex, assignment),
                Message::RELEASE(depth) => {
                    self.release_negations(depth);
                }
                Message::TOKEN(msg_token) => {
                    self.handle_incoming_token(msg_token);
                }
                _ => unreachable!(),
            }
            return true;
        } else if let Some(edge) = self.hyper_queue.pop_front() {
            let _guard = span!(
                Level::TRACE,
                "worker receive hyper-edge",
                worker_id = self.id,
                ?edge,
            );
            self.process_hyper_edge(edge);
            return true;
        } else if let Some(edge) = self.negation_queue.pop_front() {
            let _guard = span!(
                Level::TRACE,
                "worker receive negation-edge",
                worker_id = self.id,
                ?edge,
            );
            self.process_negation_edge(edge);
            return true;
        }
        // No tasks
        false
    }

    /// Releasing the edges from the unsafe queue to the safe negation
    fn release_negations(&mut self, depth: usize) {
        self.dirty = true;
        trace!(
            depth = self.unsafe_neg_edges.len(),
            "releasing previously unsafe negation edges"
        );
        #[cfg(feature = "use-counts")]
        eprintln!("worker release_negation_edges");

        assert!(
            self.unsafe_neg_edges.len() <= depth,
            "Attempted to release more than one component"
        );

        if self.unsafe_neg_edges.len() < depth {
            // If the worker does not have any negation edges with the given depth, then do nothing
            return;
        }

        if let Some(edges) = self.unsafe_neg_edges.pop() {
            // Queue all edges in the negation channel that have the given depth
            for edge in edges {
                trace!(?edge, "releasing negation edge");
                self.negation_queue.push_back(edge);
            }
        }
    }

    /// Mark the given worker as being interested in the assignment of the given vertex.
    /// When the certain assignment of the vertex is found, the worker will be notified.
    fn mark_interest(&mut self, vertex: &V, worker: WorkerId) {
        #[cfg(feature = "use-counts")]
        eprintln!("worker mark_interest");
        if let Some(set) = self.interests.get_mut(vertex) {
            trace!(is_initialized = true, ?vertex, "mark vertex interest");
            set.insert(worker);
        } else {
            trace!(is_initialized = false, ?vertex, "mark vertex interest");
            let mut set = HashSet::with_hasher(H::default());
            set.insert(worker);
            self.interests.insert(vertex.clone(), set);
        }
    }

    /// Explore a vertex by finding the outgoing edges of the vertex. In the process, the vertex
    /// will be assigned UNDECIDED. If the vertex has no edges, the vertex is immediately
    /// assigned false. If the vertex is owned by another worker, we send a request
    /// to that worker instead of evaluating the edges ourself.
    fn explore(&mut self, vertex: &V) {
        trace!(?vertex, "exploring vertex");
        #[cfg(feature = "use-counts")]
        eprintln!("worker explore");
        // Line 2
        self.assignment
            .insert(vertex.clone(), VertexAssignment::UNDECIDED);

        // Line 3
        if self.is_owner(vertex) {
            let successors = self.succ(vertex); // Line 4
            if successors.is_empty() {
                // Line 4
                // The vertex has no outgoing edges, so we assign it false
                self.final_assign(vertex, VertexAssignment::FALSE);
            } else {
                // Line 5
                // Queue the new edges
                for edge in &successors {
                    match edge {
                        Edges::HYPER(edge) => self.hyper_queue.push_back(edge.clone()),
                        Edges::NEGATION(edge) => self.negation_queue.push_back(edge.clone()),
                    }
                }
            }
        } else {
            // Line 7
            // The vertex is owned by another worker, so we send a request
            self.broker.send(
                self.vertex_owner(vertex),
                Message::REQUEST {
                    vertex: vertex.clone(),
                    depth: *self.depth.get(vertex).unwrap_or(&0),
                    worker_id: self.id,
                },
            );
        }
    }

    fn process_hyper_edge(&mut self, edge: HyperEdge<V>) {
        trace!(?edge, "processing hyper-edge");
        #[cfg(feature = "use-counts")]
        eprintln!("worker processing_hyper-edge");

        self.dirty = true;

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

    /// Mark `dependency` as a prerequisite for finding the final assignment of `vertex`
    fn add_depend(&mut self, vertex: &V, dependency: Edges<V>) {
        // Update the depth
        let old_vertex_depth = *self.depth.get(vertex).unwrap_or(&0);
        let source_depth = *self.depth.get(dependency.source()).unwrap_or(&0);
        let new_vertex_depth = if dependency.is_negation() {
            max(old_vertex_depth, source_depth + 1)
        } else {
            max(old_vertex_depth, source_depth)
        };
        self.depth.insert(vertex.clone(), new_vertex_depth);

        // Mark `dependency` as a prerequisite for finding the final assignment of `vertex`
        self.depends
            .entry(vertex.clone())
            .or_default()
            .insert(dependency);
    }

    /// Remove `dependency` as a prerequisite for finding the final assignment of `vertex`
    fn remove_depend(&mut self, vertex: &V, dependency: Edges<V>) {
        if let Some(dependencies) = self.depends.get_mut(vertex) {
            dependencies.remove(&dependency);
        }
    }

    fn process_negation_edge(&mut self, edge: NegationEdge<V>) {
        #[cfg(feature = "use-counts")]
        eprintln!("worker processing negation edge");
        self.dirty = true;
        match self.assignment.get(&edge.target) {
            // UNEXPLORED
            None => {
                // UNEXPLORED
                // Line 6
                trace!(?edge, assignment = "UNEXPLORED", "processing negation edge");
                self.add_depend(&edge.target, Edges::NEGATION(edge.clone()));
                self.queue_unsafe_negation(edge.clone());
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

    /// Queue unsafe negation, which will be queued to negation channel whenever
    /// release negation is called. If the negation edges later becomes safe, it does
    /// not have to be removed from the negation queue.
    fn queue_unsafe_negation(&mut self, edge: NegationEdge<V>) {
        let len = self.unsafe_neg_edges.len();
        let mut depth: usize = 0;
        if let Some(n) = self.depth.get(&edge.source) {
            depth = *n as usize;
        }

        if len <= depth {
            for _ in len..(depth + 1) {
                self.unsafe_neg_edges.push(Vec::new());
            }
        }

        self.unsafe_neg_edges
            .get_mut(depth as usize)
            .unwrap()
            .push(edge);
    }

    /// Process a request from another worker. We either answer immediately if we know the
    /// assignment of the requested vertex, or we explore it and mark the other as being
    /// interested in the assignment of the vertex.
    fn process_request(&mut self, vertex: &V, requester: WorkerId, depth: u32) {
        self.dirty = true;
        trace!(
            ?vertex,
            ?requester,
            depth,
            "got request for vertex assignment"
        );
        #[cfg(feature = "use-counts")]
        eprintln!("worker process_request");
        if let Some(assignment) = self.assignment.get(&vertex) {
            // Final assignment of `vertex` is already known, reply immediately
            if assignment.is_certain() {
                self.broker.send(
                    requester,
                    Message::ANSWER {
                        vertex: vertex.clone(),
                        assignment: *assignment,
                    },
                );
            } else {
                // update depth
                let local_depth = *self.depth.get(vertex).unwrap_or(&0);
                self.depth.insert(vertex.clone(), max(local_depth, depth));
                self.mark_interest(vertex, requester);
            }
        } else {
            // Final assignment of `vertex` is not yet known

            // update depth
            let local_depth = *self.depth.get(vertex).unwrap_or(&0);
            self.depth.insert(vertex.clone(), max(local_depth, depth));
            self.mark_interest(vertex, requester);

            if self.assignment.get(&vertex).is_none() {
                // UNEXPLORED
                self.explore(&vertex);
            }
        }
    }

    /// Process an answer by applying the given assignment
    fn process_answer(&mut self, vertex: &V, assigned: VertexAssignment) {
        self.dirty = true;
        trace!(?vertex, ?assigned, "received final assignment");
        self.final_assign(vertex, assigned);
    }

    /// Set the assignment of the given vertex. Dependent edges are requeued and interested
    /// workers are notified of the assignment.
    fn final_assign(&mut self, vertex: &V, assignment: VertexAssignment) {
        debug!(?assignment, ?vertex, "final assigned");
        #[cfg(feature = "use-counts")]
        eprintln!("worker final_assign");

        // Line 2
        if *vertex == self.v0 {
            // We found the result of the query
            self.broker.return_result(assignment);
            return;
        }

        // Line 3
        let prev_assignment = self.assignment.insert(vertex.clone(), assignment);
        let changed_assignment = prev_assignment != Some(assignment);

        // There are a few cases, where we redundantly assign a vertex to what it already is:
        // - An unsafe negation edge, turned out to be safe, and now it has been release
        // - We request the assignment of a vertex multiple times and we get two answers due to
        //   race conditions
        if changed_assignment {
            // Line 4 - Notify other workers interested in this assignment
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

            // Line 5 - Requeue edges that depend on this assignment
            if let Some(depends) = self.depends.get(&vertex) {
                for edge in depends.clone() {
                    trace!(?edge, "requeueing edge because target got assignment");
                    match edge {
                        Edges::HYPER(edge) => self.hyper_queue.push_back(edge.clone()),
                        Edges::NEGATION(edge) => self.negation_queue.push_back(edge.clone()),
                    }
                }
            }
        }
    }

    /// Helper function for deleting edges from a vertex.
    fn delete_edge(&mut self, edge: Edges<V>) {
        let source = edge.source();

        // Remove edge from source
        self.successors.get_mut(source).unwrap().remove(&edge);

        match self.successors.get(source) {
            None => panic!("successors should have been filled, or at least have a empty vector"),
            Some(successors) => {
                // Line 3
                if successors.is_empty() {
                    trace!(
                        ?source,
                        assignment = ?VertexAssignment::FALSE,
                        "no more successors, final assignment is FALSE"
                    );
                    self.final_assign(source, VertexAssignment::FALSE);
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
    fn succ(&mut self, vertex: &V) -> HashSet<Edges<V>, H> {
        if let Some(successors) = self.successors.get(vertex) {
            debug!(?vertex, ?successors, known_vertex = true, "edg::succ");
            #[cfg(feature = "use-counts")]
            eprintln!("worker succ cached");
            // List of successors is already allocated for the vertex
            successors.to_owned()
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
            #[cfg(feature = "use-counts")]
            eprintln!("worker succ generated");
            successors
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::fmt::Display;
    use test_env_log::test;

    use core::fmt::Formatter;

    use crate::common::{Edges, HyperEdge, NegationEdge};
    use crate::edg::{distributed_certain_zero, ExtendedDependencyGraph, Vertex};

    #[test]
    fn test_dcz_empty_hyper_edge() {
        simple_edg![
            A => -> {};
        ];
        edg_assert!(A, TRUE);
    }

    #[test]
    fn test_dcz_no_successors() {
        simple_edg![
            A => ;
        ];
        edg_assert!(A, FALSE);
    }

    #[test]
    fn test_dcz_general_01() {
        simple_edg![
            A => -> {B, C} -> {D};
            B => ;
            C => .> D;
            D => -> {};
        ];
        edg_assert!(A, TRUE);
        edg_assert!(B, FALSE);
        edg_assert!(C, FALSE);
        edg_assert!(D, TRUE);
    }

    #[test]
    fn test_dcz_general_02() {
        simple_edg![
            A => -> {B, C};
            B => .> E;
            C => -> {};
            D => -> {} -> {C};
            E => .> D;
        ];
        edg_assert!(A, TRUE);
        edg_assert!(B, TRUE);
        edg_assert!(C, TRUE);
        edg_assert!(D, TRUE);
        edg_assert!(E, FALSE);
    }

    #[test]
    fn test_dcz_general_03() {
        simple_edg![
            A => -> {B} -> {E};
            B => -> {C};
            C => -> {F} -> {H};
            D => -> {E} -> {C};
            E => -> {D, F};
            F => -> {};
            G => .> A;
            H => -> {I};
            I => ;
        ];
        edg_assert!(A, TRUE);
        edg_assert!(B, TRUE);
        edg_assert!(C, TRUE);
        edg_assert!(D, TRUE);
        edg_assert!(E, TRUE);
        edg_assert!(F, TRUE);
        edg_assert!(G, FALSE);
        edg_assert!(H, FALSE);
        edg_assert!(I, FALSE);
    }

    #[test]
    fn test_dcz_general_04() {
        simple_edg![
            A => -> {B} -> {C};
            B => -> {D};
            C => ;
            D => -> {};
        ];
        edg_assert!(A, TRUE);
        edg_assert!(B, TRUE);
        edg_assert!(C, FALSE);
        edg_assert!(D, TRUE);
    }

    #[test]
    fn test_dcz_general_05() {
        simple_edg![
            A => -> {B};
            B => -> {C};
            C => -> {B};
        ];
        edg_assert!(A, FALSE);
        edg_assert!(B, FALSE);
        edg_assert!(C, FALSE);
    }

    #[test]
    fn test_dcz_general_06() {
        simple_edg![
            A => -> {B} -> {C};
            B => ;
            C => ;
        ];
        edg_assert!(A, FALSE);
        edg_assert!(B, FALSE);
        edg_assert!(C, FALSE);
    }

    #[test]
    fn test_dcz_general_07() {
        simple_edg![
            A => -> {B};
            B => -> {A, C};
            C => -> {D};
            D => -> {};
        ];
        edg_assert!(A, FALSE);
        edg_assert!(B, FALSE);
        edg_assert!(C, TRUE);
        edg_assert!(D, TRUE);
    }

    #[test]
    fn test_dcz_general_08() {
        simple_edg![
            A => -> {B, C};
            B => -> {C} -> {D};
            C => -> {B};
            D => -> {C} -> {};
        ];
        edg_assert!(A, TRUE);
        edg_assert!(B, TRUE);
        edg_assert!(C, TRUE);
        edg_assert!(D, TRUE);
    }

    #[test]
    fn test_dcz_negation_01() {
        simple_edg![
            A => .> B;
            B => -> {};
        ];
        edg_assert!(A, FALSE);
        edg_assert!(B, TRUE);
    }

    #[test]
    fn test_dcz_negation_02() {
        simple_edg![
            A => .> B;
            B => -> {C};
            C => -> {B} .> D;
            D => -> {E};
            E => -> {D};
        ];
        edg_assert!(A, FALSE);
        edg_assert!(B, TRUE);
        edg_assert!(C, TRUE);
        edg_assert!(D, FALSE);
        edg_assert!(E, FALSE);
    }

    #[test]
    fn test_dcz_negation_03() {
        simple_edg![
            A => .> B .> C;
            B => .> D;
            C => -> {D};
            D => ;
        ];
        edg_assert!(A, TRUE);
        edg_assert!(B, TRUE);
        edg_assert!(C, FALSE);
        edg_assert!(D, FALSE);
    }

    #[test]
    fn test_dcz_negation_04() {
        simple_edg![
            A => .> B;
            B => -> {B};
        ];
        edg_assert!(A, TRUE);
        edg_assert!(B, FALSE);
    }

    #[test]
    fn test_dcz_negation_05() {
        simple_edg![
            A => .> B;
            B => .> C;
            C => .> D;
            D => .> E;
            E => .> F;
            F => -> {F};
        ];
        edg_assert!(A, TRUE);
        edg_assert!(B, FALSE);
        edg_assert!(C, TRUE);
        edg_assert!(D, FALSE);
        edg_assert!(E, TRUE);
        edg_assert!(F, FALSE);
    }
}
