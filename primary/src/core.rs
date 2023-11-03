// Copyright(C) Facebook, Inc. and its affiliates.
use crate::aggregators::{QCMaker, TCMaker, VotesAggregator};
//use crate::common::special_header;
use crate::error::{DagError, DagResult};
use crate::leader::LeaderElector;
use crate::messages::{
    Certificate, ConsensusMessage, Header, Proposal, Ticket, Timeout, Vote, QC, TC,
};
use crate::primary::{Height, PrimaryMessage, Slot, View};
use crate::synchronizer::Synchronizer;
use crate::timer::Timer;
use async_recursion::async_recursion;
use bytes::Bytes;
use config::{Committee, Stake};
use crypto::{Digest, PublicKey, SignatureService};
use crypto::{Hash as _, Signature};
use futures::stream::FuturesUnordered;
use futures::{Future, StreamExt};
use log::{debug, error, warn};
use network::{CancelHandler, ReliableSender};
//use tokio::time::error::Elapsed;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
//use std::task::Poll;
use store::Store;
use tokio::sync::mpsc::{Receiver, Sender};
//use tokio::time::{sleep, Duration, Instant};

//use crate::messages_consensus::{QC, TC};

#[cfg(test)]
#[path = "tests/core_tests.rs"]
pub mod core_tests;

pub struct Core {
    /// The public key of this primary.
    name: PublicKey,
    /// The committee information.
    committee: Committee,
    /// The persistent storage.
    store: Store,
    /// Handles synchronization with other nodes and our workers.
    // TODO: Start syncing asynchronously once you receive a header, so that it's not on the
    // critical path
    synchronizer: Synchronizer,
    /// Service to sign headers.
    signature_service: SignatureService,
    /// The current consensus round (used for cleanup).
    consensus_round: Arc<AtomicU64>,
    /// The depth of the garbage collector.
    gc_depth: Height,

    /// Receiver for dag messages (headers, votes, certificates).
    rx_primaries: Receiver<PrimaryMessage>,
    /// Receives loopback headers from the `HeaderWaiter`.
    rx_header_waiter: Receiver<Header>,
    /// Receives loopback instances from the 'HeaderWaiter'
    rx_header_waiter_instances: Receiver<(Proposal, ConsensusMessage)>,
    /// Receives loopback certificates from the `CertificateWaiter`.
    rx_certificate_waiter: Receiver<Certificate>,
    /// Receives our newly created headers from the `Proposer`.
    rx_proposer: Receiver<Header>,
    /// Output special certificates to the consensus layer.
    tx_consensus: Sender<Certificate>,
    // Output all certificates to the consensus Dag view
    tx_committer: Sender<ConsensusMessage>,

    /// Send valid a quorum of certificates' ids to the `Proposer` (along with their round).
    tx_proposer: Sender<Certificate>,
    tx_special: Sender<Header>,

    rx_pushdown_cert: Receiver<Certificate>,
    // Receive sync requests for headers required at the consensus layer
    rx_request_header_sync: Receiver<Digest>,

    /// The last garbage collected round.
    gc_round: Height,

    /// The authors of the last voted headers. (Ensures only voting for one header per round)
    last_voted: HashMap<Height, HashSet<PublicKey>>,
    // /// The set of headers we are currently processing.
    processing: HashMap<Height, HashSet<Digest>>, //NOTE: Keep processing separate from current_headers ==> to allow us to process multiple headers from same replica (e.g. in case we first got a header that isnt the one that creates a cert)
    /// The last header we proposed (for which we are waiting votes).
    current_header: Header,

    // Keeps track of current headers
    //TODO: Merge current_headers && processing.
    //current_headers: HashMap<Height, HashMap<PublicKey, Header>>, ///HashMap<Digest, Header>, //Note, re-factored this map to do GC cleaner.
    // Hashmap containing votes aggregators
    vote_aggregators: HashMap<Height, HashMap<Digest, Box<VotesAggregator>>>, //HashMap<Digest, VotesAggregator>,
    // /// Aggregates votes into a certificate.
    votes_aggregator: VotesAggregator,

    //votes_aggregators: HashMap<Round, VotesAggregator>, //TODO: To accomodate all to all, the map should be map<round, map<publickey, VotesAggreagtor>>
    /// A network sender to send the batches to the other workers.
    network: ReliableSender,
    /// Keeps the cancel handlers of the messages we sent.
    cancel_handlers: HashMap<Height, Vec<CancelHandler>>,

    tips: HashMap<PublicKey, Header>,
    current_proposals: HashMap<PublicKey, Proposal>,
    current_certs: HashMap<PublicKey, Certificate>,
    views: HashMap<Slot, View>,
    timers: HashSet<(Slot, View)>,
    last_voted_consensus: HashSet<(Slot, View)>,
    commit_messages: VecDeque<ConsensusMessage>,
    timer_futures: FuturesUnordered<Pin<Box<dyn Future<Output = (Slot, View)> + Send>>>,
    // TODO: Add garbage collection, related to how deep pipeline (parameter k)
    qcs: HashMap<Slot, ConsensusMessage>, // NOTE: Store the latest QC for each slot
    qc_makers: HashMap<Digest, QCMaker>,
    current_qcs_formed: usize,
    tc_makers: HashMap<(Slot, View), TCMaker>,
    current_consensus_instances: HashMap<Digest, ConsensusMessage>,
    tcs: HashMap<Slot, TC>,
    tickets: VecDeque<Ticket>,
    already_proposed_slots: HashSet<Slot>,
    committed: HashMap<Slot, ConsensusMessage>,
    tx_info: Sender<ConsensusMessage>,
    leader_elector: LeaderElector,
    timeout_delay: u64,
    // GC the vote aggregators and current headers
    // gc_map: HashMap<Round, Digest>,
}

impl Core {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        name: PublicKey,
        committee: Committee,
        store: Store,
        synchronizer: Synchronizer,
        signature_service: SignatureService,
        consensus_round: Arc<AtomicU64>,
        gc_depth: Height,
        rx_primaries: Receiver<PrimaryMessage>,
        rx_header_waiter: Receiver<Header>,
        rx_header_waiter_instances: Receiver<(Proposal, ConsensusMessage)>,
        rx_certificate_waiter: Receiver<Certificate>,
        rx_proposer: Receiver<Header>,
        tx_consensus: Sender<Certificate>,
        tx_committer: Sender<ConsensusMessage>,
        tx_proposer: Sender<Certificate>,
        tx_special: Sender<Header>,
        rx_pushdown_cert: Receiver<Certificate>,
        rx_request_header_sync: Receiver<Digest>,
        tx_info: Sender<ConsensusMessage>,
        leader_elector: LeaderElector,
        timeout_delay: u64,
    ) {
        tokio::spawn(async move {
            Self {
                name,
                //current_header: Header::genesis(&committee),
                committee,
                store,
                synchronizer,
                signature_service,
                consensus_round,
                gc_depth,
                rx_primaries,
                rx_header_waiter,
                rx_header_waiter_instances,
                rx_certificate_waiter,
                rx_proposer,
                tx_consensus,
                tx_committer,
                tx_proposer,
                tx_special,
                rx_pushdown_cert,
                rx_request_header_sync,
                tx_info,
                leader_elector,
                gc_round: 0,
                current_qcs_formed: 0,
                last_voted: HashMap::with_capacity(2 * gc_depth as usize),
                commit_messages: VecDeque::with_capacity(2 * gc_depth as usize),
                processing: HashMap::with_capacity(2 * gc_depth as usize),
                current_header: Header::default(),
                votes_aggregator: VotesAggregator::new(),
                vote_aggregators: HashMap::with_capacity(2 * gc_depth as usize),
                network: ReliableSender::new(),
                cancel_handlers: HashMap::with_capacity(2 * gc_depth as usize),
                already_proposed_slots: HashSet::new(),
                tips: HashMap::with_capacity(2 * gc_depth as usize),
                current_proposals: HashMap::with_capacity(2 * gc_depth as usize),
                current_certs: HashMap::with_capacity(2 * gc_depth as usize),
                views: HashMap::with_capacity(2 * gc_depth as usize),
                timers: HashSet::with_capacity(2 * gc_depth as usize),
                last_voted_consensus: HashSet::with_capacity(2 * gc_depth as usize),
                qcs: HashMap::with_capacity(2 * gc_depth as usize),
                qc_makers: HashMap::with_capacity(2 * gc_depth as usize),
                tc_makers: HashMap::with_capacity(2 * gc_depth as usize),
                tcs: HashMap::with_capacity(2 * gc_depth as usize),
                tickets: VecDeque::with_capacity(2 * gc_depth as usize),
                committed: HashMap::with_capacity(2 * gc_depth as usize),
                current_consensus_instances: HashMap::with_capacity(2 * gc_depth as usize),
                timeout_delay,
                timer_futures: FuturesUnordered::new(),
                //gc_map: HashMap::with_capacity(2 * gc_depth as usize),
            }
            .run()
            .await;
        });
    }

    async fn process_own_header(&mut self, header: Header) -> DagResult<()> {
        // Update the current header we are collecting votes for
        self.current_header = header.clone();

        // Reset the votes aggregator.
        self.votes_aggregator = VotesAggregator::new();

        // Broadcast the new header in a reliable manner.
        let addresses = self
            .committee
            .others_primaries(&self.name)
            .iter()
            .map(|(_, x)| x.primary_to_primary)
            .collect();
        let bytes = bincode::serialize(&PrimaryMessage::Header(header.clone()))
            .expect("Failed to serialize our own header");
        let handlers = self.network.broadcast(addresses, Bytes::from(bytes)).await;
        self.cancel_handlers
            .entry(header.height)
            .or_insert_with(Vec::new)
            .extend(handlers);

        // Process the header.
        self.process_header(header).await
    }

    #[async_recursion]
    async fn process_header(&mut self, header: Header) -> DagResult<()> {
        debug!("Processing {:?}", header);
        println!("Processing the header");

        // Check the parent certificate. Ensure the certificate contains a quorum of votes and is
        // at the preivous height
        let stake: Stake = header
            .parent_cert
            .votes
            .iter()
            .map(|(pk, _)| self.committee.stake(pk))
            .sum();
        ensure!(
            header.parent_cert.height() + 1 == header.height(),
            DagError::MalformedHeader(header.id.clone())
        );
        ensure!(
            stake >= self.committee.validity_threshold() || header.parent_cert.height() == 0,
            DagError::HeaderRequiresQuorum(header.id.clone())
        );

        // Ensure we have the payload. If we don't, the synchronizer will ask our workers to get it, and then
        // reschedule processing of this header once we have it.
        if self.synchronizer.missing_payload(&header).await? {
            println!("Missing payload");
            debug!("Processing of {} suspended: missing payload", header);
            return Ok(());
        }

        // By FIFO should have all ancestors, reschedule for processing if we don't
        if self
            .synchronizer
            .get_parent_header(&header)
            .await?
            .is_none()
        {
            return Ok(());
        }

        // Store the header since we have the parents (recursively).
        let bytes = bincode::serialize(&header).expect("Failed to serialize header");
        self.store.write(header.digest().to_vec(), bytes).await;

        // If the header received is at a greater height then add it to our local tips and
        // proposals
        if header.height() > self.tips.get(&header.origin()).unwrap().height() {
            self.tips.insert(header.origin(), header.clone());
            self.current_proposals.insert(
                header.origin(),
                Proposal {
                    header_digest: header.digest(),
                    height: header.height(),
                },
            );
        }

        // Process the parent certificate
        self.process_certificate(header.clone().parent_cert).await?;

        // Check if we can vote for this header.
        if self
            .last_voted
            .entry(header.height())
            .or_insert_with(HashSet::new)
            .insert(header.author)
        {
            // Process the consensus instances contained in the header (if any)
            let consensus_sigs = self
                .process_consensus_messages(&header, &header.consensus_instances)
                .await?;

            // Create a vote for the header and any valid consensus instances
            let vote = Vote::new(
                &header,
                &self.name,
                &mut self.signature_service,
                consensus_sigs,
            )
            .await;
            println!("Created vote");
            debug!("Created Vote {:?}", vote);

            if vote.origin == self.name {
                self.process_vote(vote)
                    .await
                    .expect("Failed to process our own vote");
            } else {
                let address = self
                    .committee
                    .primary(&header.author)
                    .expect("Author of valid header is not in the committee")
                    .primary_to_primary;
                let bytes = bincode::serialize(&PrimaryMessage::Vote(vote))
                    .expect("Failed to serialize our own vote");
                let handler = self.network.send(address, Bytes::from(bytes)).await;
                self.cancel_handlers
                    .entry(header.height())
                    .or_insert_with(Vec::new)
                    .push(handler);
            }
        }
        Ok(())
    }

    #[async_recursion]
    async fn process_vote(&mut self, vote: Vote) -> DagResult<()> {
        debug!("Processing Vote {:?}", vote);

        // Iterate through all votes for each consensus instance
        for (digest, sig) in vote.consensus_sigs.iter() {
            // TODO: Only process instance if we are the leader for it, sanity check with leader
            // elector
            // If not already a qc maker for this consensus instance message, create one
            match self.qc_makers.get(&digest) {
                Some(_) => {}
                None => {
                    self.qc_makers.insert(digest.clone(), QCMaker::new());
                }
            }

            // Otherwise get the qc maker for this instance
            let qc_maker = self.qc_makers.get_mut(&digest).unwrap();

            // Add vote to qc maker, if a QC forms then create a new consensus instance
            // TODO: Put fast path logic in qc maker (decide whether to wait timeout etc.), add
            // external messages
            if let Some(qc) =
                qc_maker.append(vote.origin, (digest.clone(), sig.clone()), &self.committee)?
            {
                self.current_qcs_formed += 1;

                let current_instance = self
                    .current_header
                    .consensus_instances
                    .get(&digest)
                    .unwrap();
                match current_instance {
                    ConsensusMessage::Prepare {
                        slot,
                        view,
                        ticket: _,
                        proposals,
                    } => {
                        // Create a tip proposal for the header which contains the prepare message,
                        // so that it can be committed as part of the proposals
                        let leader_tip_proposal: Proposal = Proposal {
                            header_digest: self.current_header.digest(),
                            height: self.current_header.height(),
                        };
                        // Add this cert to the proposals for this instance
                        let mut new_proposals = proposals.clone();
                        new_proposals.insert(self.name, leader_tip_proposal);

                        let new_instance = ConsensusMessage::Confirm {
                            slot: *slot,
                            view: *view,
                            qc,
                            proposals: new_proposals,
                        };

                        // Send this new instance to the proposer
                        self.tx_info
                            .send(new_instance)
                            .await
                            .expect("Failed to send info");
                    }
                    ConsensusMessage::Confirm {
                        slot,
                        view,
                        qc: _,
                        proposals,
                    } => {
                        let new_instance = ConsensusMessage::Commit {
                            slot: *slot,
                            view: *view,
                            qc,
                            proposals: proposals.clone(),
                        };

                        // Add this instance to our local view of the current consensus instances
                        self.current_consensus_instances
                            .insert(new_instance.digest(), new_instance.clone());

                        // Send this new instance to the proposer
                        self.tx_info
                            .send(new_instance)
                            .await
                            .expect("Failed to send info");
                    }
                    ConsensusMessage::Commit {
                        slot: _,
                        view: _,
                        qc: _,
                        proposals: _,
                    } => {}
                };
            }
        }

        // Add the vote to the votes aggregator for the actual header
        let dissemination_cert =
            self.votes_aggregator
                .append(vote, &self.committee, &self.current_header)?;
        // If there are no consensus instances in the header then only wait for the dissemination
        // cert (f+1) votes
        let dissemination_ready: bool =
            self.current_header.consensus_instances.is_empty() && dissemination_cert.is_some();
        // If there are some consensus instances in the header then wait for 2f+1 votes to form QCs
        let consensus_ready: bool = self.current_qcs_formed == self.current_header.consensus_instances.len();

        if dissemination_ready || consensus_ready {
            //debug!("Assembled {:?}", dissemination_cert.unwrap());
            self.process_certificate(dissemination_cert.unwrap())
                .await
                .expect("Failed to process valid certificate");
            self.current_qcs_formed = 0;
        }

        // TODO: Handle invalidated case where possibly want to send consensus message externally
        Ok(())
    }


    #[async_recursion]
    async fn process_certificate(&mut self, certificate: Certificate) -> DagResult<()> {
        debug!("Processing {:?}", certificate);

        // Store the certificate.
        let bytes = bincode::serialize(&certificate).expect("Failed to serialize certificate");
        self.store.write(certificate.digest().to_vec(), bytes).await;

        // If we receive a new certificate from ourself, then send to the proposer, so it can make
        // a new header
        if certificate.origin() == self.name {
            // Send it to the `Proposer`.
            self.tx_proposer
                .send(certificate.clone())
                .await
                .expect("Failed to send certificate");
        }

        println!("Current certs are {:?}", self.current_certs);
        println!("Certificate origin is {:?}", certificate.origin());

        // If we receive a new cert then check to see whether there is enough coverage for any of
        // the tickets we have
        if certificate.height()
            > self
                .current_certs
                .get(&certificate.origin())
                .unwrap()
                .height()
        {
            self.current_certs
                .insert(certificate.origin(), certificate.clone());

            // If we have pending tickets that don't have enough coverage check to see if we can
            // propose a new prepare instance
            self.is_ticket_ready().await;
        }

        Ok(())
    }

    async fn is_ticket_ready(&mut self) {
        if !self.tickets.is_empty() {
            let ticket = self.tickets.pop_front().unwrap();
            let new_proposals = self.current_proposals.clone();

            // If there is enough coverage and we haven't already proposed then create a new
            // prepare message
            if self.enough_coverage(&ticket, &new_proposals)
                && !self.already_proposed_slots.contains(&(ticket.slot + 1))
                && self.name == self.leader_elector.get_leader(ticket.slot + 1, 1)
            {
                let new_prepare_instance = ConsensusMessage::Prepare {
                    slot: ticket.slot + 1,
                    view: 1,
                    ticket: ticket.clone(),
                    proposals: new_proposals,
                };
                self.already_proposed_slots.insert(ticket.slot + 1);
                self.tickets.pop_front();
                self.current_consensus_instances
                    .insert(new_prepare_instance.digest(), new_prepare_instance.clone());

                self.tx_info
                    .send(new_prepare_instance)
                    .await
                    .expect("failed to send info to proposer");
            } else {
                self.tickets.push_front(ticket);
            }
        }
    }

    // TODO: Double check these are comprehensive enough
    fn is_valid(&mut self, consensus_message: &ConsensusMessage) -> bool {
        match consensus_message {
            ConsensusMessage::Prepare { slot, view, ticket, proposals: _ } => {
                !self.last_voted_consensus.contains(&(*slot, *view)) && ticket.slot + 1 == *slot && self.views.get(slot).unwrap() == view
            },
            ConsensusMessage::Confirm { slot, view, qc, proposals: _ } => {
                qc.verify(&self.committee).is_ok() && self.views.get(slot).unwrap() == view
            },
            ConsensusMessage::Commit { slot, view, qc, proposals: _ } => {
                qc.verify(&self.committee).is_ok() && self.views.get(slot).unwrap() == view
            },
        }
    }

    #[async_recursion]
    async fn process_consensus_messages(
        &mut self,
        header: &Header,
        consensus_messages: &HashMap<Digest, ConsensusMessage>,
    ) -> DagResult<Vec<(Digest, Signature)>> {
        // Map between consensus instance digest and a signature indicating a vote for that
        // instance
        let mut consensus_sigs: Vec<(Digest, Signature)> = Vec::new();

        for (digest, consensus_message) in consensus_messages {
            println!("processing instance");
            if self.is_valid(consensus_message) {
                match consensus_message {
                    ConsensusMessage::Prepare {
                        slot,
                        view,
                        ticket,
                        proposals,
                    } => {
                        for (pk, proposal) in proposals {
                            self.synchronizer.start_proposal_sync(proposal.clone(), &pk, consensus_message.clone());
                        }
                        self.process_prepare_message(consensus_message, header, consensus_sigs.as_mut());
                        self.last_voted_consensus.insert((*slot, *view));
                    }
                    ConsensusMessage::Confirm {
                        slot,
                        view,
                        qc,
                        proposals,
                    } => {
                        for (pk, proposal) in proposals {
                            self.synchronizer.start_proposal_sync(proposal.clone(), &pk, consensus_message.clone());
                        }

                        self.process_confirm_message(consensus_message, consensus_sigs.as_mut());
                    }
                    ConsensusMessage::Commit {
                        slot,
                        view,
                        qc,
                        proposals,
                    } => {
                        for (pk, proposal) in proposals {
                            self.synchronizer.start_proposal_sync(proposal.clone(), &pk, consensus_message.clone());
                        }

                        self.process_commit_message(consensus_message.clone());
                    }
                }
            }
        }

        Ok(consensus_sigs)
    }

    #[async_recursion]
    async fn process_prepare_message(
        &mut self,
        prepare_message: &ConsensusMessage,
        header: &Header,
        consensus_sigs: &mut Vec<(Digest, Signature)>,
    ) {
        match prepare_message {
            ConsensusMessage::Prepare {
                slot,
                view: _,
                ticket: _,
                proposals,
            } => {
                let has_proposed = self.already_proposed_slots.contains(&(slot + 1));
                // If we are the leader of the next slot and haven't already proposed then
                // receiving a Prepare for slot is our ticket to propose for next slot
                // TODO: Separate this to a new function for ticket related processing
                if self.name == self.leader_elector.get_leader(slot + 1, 1) && !has_proposed {
                    let ticket =
                        Ticket::new(Some(header.clone()), None, *slot, proposals.clone()).await;
                    let new_proposals = self.current_proposals.clone();
                    // If there are enough new proposals received then create a prepare message
                    // for the next slot
                    if self.enough_coverage(&ticket, &new_proposals) {
                        let new_prepare_instance = ConsensusMessage::Prepare {
                            slot: slot + 1,
                            view: 1,
                            ticket,
                            proposals: new_proposals,
                        };
                        self.already_proposed_slots.insert(slot + 1);
                        self.current_consensus_instances
                            .insert(new_prepare_instance.digest(), new_prepare_instance.clone());
                        self.tx_info
                            .send(new_prepare_instance)
                            .await
                            .expect("failed to send info to proposer");
                    } else {
                        // Otherwise add the ticket to the queue, and wait later until there
                        // are enough new certificates to propose
                        self.tickets.push_back(ticket);
                    }
                }

                // If we haven't already started the timer for the next slot, start it
                if !self.timers.contains(&(slot + 1, 1)) {
                    // TODO: also forward the ticket to the leader (to tolerate byzantine
                    // proposers)
                    let timer = Timer::new(slot + 1, 1, self.timeout_delay);
                    self.timer_futures.push(Box::pin(timer));
                    self.timers.insert((slot + 1, 1));
                }

                // Indicate that we vote for this instance's prepare message
                let sig = self
                    .signature_service
                    .request_signature(prepare_message.digest())
                    .await;
                consensus_sigs.push((prepare_message.digest(), sig));
            }
            _ => {}
        }
    }

    #[async_recursion]
    async fn process_confirm_message(
        &mut self,
        confirm_message: &ConsensusMessage,
        consensus_sigs: &mut Vec<(Digest, Signature)>,
    ) {
        match confirm_message {
            ConsensusMessage::Confirm {
                slot,
                view,
                qc,
                proposals,
            } => {
                self.qcs.insert(*slot, confirm_message.clone());

                // Indicate that we vote for this instance's confirm message
                let sig = self
                    .signature_service
                    .request_signature(confirm_message.digest())
                    .await;
                consensus_sigs.push((confirm_message.digest(), sig));
            }
            _ => {}
        }
    }

    fn enough_coverage(
        &mut self,
        ticket: &Ticket,
        current_proposals: &HashMap<PublicKey, Proposal>,
    ) -> bool {
        // Checks whether there have been n-f new certs from the proposals from the ticket
        let new_tips: HashMap<&PublicKey, &Proposal> = current_proposals
            .iter()
            .filter(|(pk, proposal)| proposal.height > ticket.proposals.get(&pk).unwrap().height)
            .collect();

        new_tips.len() as u32 >= self.committee.quorum_threshold()
    }

    async fn is_commit_ready(&mut self, commit_message: &ConsensusMessage) -> bool {
        match commit_message {
            ConsensusMessage::Commit {
                slot: _,
                view: _,
                qc: _,
                proposals,
            } => {
                let mut is_ready: bool = true;

                // Check if all proposals are in the store
                for (_, proposal) in proposals.clone() {
                    // Suspend processing if any proposal is not ready
                    is_ready = is_ready && self.synchronizer.is_proposal_ready(&proposal).await.unwrap();
                }

                is_ready
            }
            _ => false,
        }
    }

    #[async_recursion]
    async fn process_commit_message(&mut self, commit_message: ConsensusMessage) -> DagResult<()> {
        match &commit_message {
            ConsensusMessage::Commit {
                slot,
                view: _,
                qc: _,
                proposals,
            } => {
                // Only send to committer once proposals are ready
                if self.is_commit_ready(&commit_message).await {
                    // Send headers to the committer
                    self.tx_committer
                        .send(commit_message)
                        .await
                        .expect("Failed to send headers");
                } else {
                    // Otherwise add to pending commit messages
                    self.commit_messages.push_back(commit_message);
                }
            }
            _ => {}
        }

        Ok(())
    }

    #[async_recursion]
    async fn process_loopback(&mut self) -> DagResult<()> {
        if !self.commit_messages.is_empty() {
            let commit_message = self.commit_messages.pop_front().unwrap();

            if self.is_commit_ready(&commit_message).await {
                // Send commit message to the committer
                self.tx_committer
                    .send(commit_message.clone())
                    .await
                    .expect("Failed to send headers");
                // Remove the commit message from pending queue
                self.commit_messages.pop_front();
            } else {
                // Reinsert the commit message to the front
                self.commit_messages.push_front(commit_message);
            }
        }
        Ok(())
    }


    async fn local_timeout_round(&mut self, slot: Slot, view: View) -> DagResult<()> {
        warn!("Timeout reached for slot {}, view {}", slot, view);
        // TODO: If smaller view then return early

        // Make a timeout message.for the slot, view, containing the highest QC this replica has
        // seen
        let timeout = Timeout::new(
            slot,
            view,
            self.qcs.get(&slot).cloned(),
            self.name,
            self.signature_service.clone(),
        )
        .await;
        debug!("Created {:?}", timeout);

        // Broadcast the timeout message.
        debug!("Broadcasting {:?}", timeout);
        let addresses = self
            .committee
            .others_consensus(&self.name)
            .into_iter()
            .map(|(_, x)| x.consensus_to_consensus)
            .collect();
        let message = bincode::serialize(&PrimaryMessage::Timeout(timeout.clone()))
            .expect("Failed to serialize timeout message");
        self.network
            .broadcast(addresses, Bytes::from(message))
            .await;

        // Process our message.
        self.handle_timeout(&timeout).await
    }

    async fn handle_timeout(&mut self, timeout: &Timeout) -> DagResult<()> {
        debug!("Processing {:?}", timeout);
        // Don't process timeout messages for old views
        match self.views.get(&timeout.slot) {
            Some(view) => {
                if timeout.view < *view {
                    return Ok(());
                }
            }
            _ => {}
        };

        // Ensure the timeout is well formed.
        timeout.verify(&self.committee)?;

        // If we haven't seen a timeout for this slot, view, then create a new TC maker for it.
        if self.tc_makers.get(&(timeout.slot, timeout.view)).is_none() {
            self.tc_makers
                .insert((timeout.slot, timeout.view), TCMaker::new());
        }

        // Otherwise, get the TC maker for this slot, view.
        let tc_maker = self
            .tc_makers
            .get_mut(&(timeout.slot, timeout.view))
            .unwrap();

        // Add the new vote to our aggregator and see if we have a quorum.
        if let Some(tc) = tc_maker.append(timeout.clone(), &self.committee)? {
            debug!("Assembled {:?}", tc);

            // Try to advance the view
            self.views.insert(timeout.slot, timeout.view + 1);

            // Start the new view timer
            let timer = Timer::new(tc.slot, tc.view + 1, self.timeout_delay);
            self.timer_futures.push(Box::pin(timer));

            // Broadcast the TC.
            debug!("Broadcasting {:?}", tc);
            let addresses = self
                .committee
                .others_consensus(&self.name)
                .into_iter()
                .map(|(_, x)| x.consensus_to_consensus)
                .collect();
            let message = bincode::serialize(&PrimaryMessage::TC(tc.clone()))
                .expect("Failed to serialize timeout certificate");
            self.network
                .broadcast(addresses, Bytes::from(message))
                .await;

            // Make a new header if we are the next leader.
            if self.name
                == self
                    .leader_elector
                    .get_leader(timeout.slot, timeout.view + 1)
            {
                // TODO: Wrap this in a function
                // TODO: For fast path add prepare
                // TODO: Add latest commit message as well for early termination
                let mut winning_proposals = HashMap::new();
                let mut winning_view = 0;

                // Find the timeout message containing the highest QC, and use that as the winning
                // proposal for the view change
                for timeout in &tc.timeouts {
                    match &timeout.high_qc {
                        Some(qc) => {
                            match qc {
                                ConsensusMessage::Confirm {
                                    slot: _,
                                    view: other_view,
                                    qc: _,
                                    proposals,
                                } => {
                                    // Update the highest QC view if we see a higher one
                                    if other_view > &winning_view {
                                        winning_view = timeout.view;
                                        winning_proposals = proposals.clone();
                                    }
                                }
                                _ => {}
                            }
                        }
                        None => {}
                    };
                }

                // A TC is a ticket to propose in the next view
                // TODO: Make proposals optional in ticket
                let ticket: Ticket = Ticket {
                    header: None,
                    tc: Some(tc),
                    slot: timeout.slot,
                    proposals: winning_proposals.clone(),
                };

                // If there is no QC we have to propose, then use our current tips for our proposal
                if winning_proposals.is_empty() {
                    winning_proposals = self.current_proposals.clone();
                }

                // Create a prepare message for the next view, containing the ticket and proposals
                let prepare_instance: ConsensusMessage = ConsensusMessage::Prepare {
                    slot: timeout.slot,
                    view: timeout.view + 1,
                    ticket: ticket.clone(),
                    proposals: winning_proposals.clone(),
                };
                self.tx_info
                    .send(prepare_instance)
                    .await
                    .expect("Failed to send consensus instance");

                // A TC could be a ticket for the next slot
                // TODO: Comment it out
                if !self.already_proposed_slots.contains(&(timeout.slot + 1))
                    && self.enough_coverage(&ticket, &winning_proposals)
                {
                    let new_prepare_instance = ConsensusMessage::Prepare {
                        slot: timeout.slot + 1,
                        view: 1,
                        ticket,
                        proposals: winning_proposals,
                    };
                    self.already_proposed_slots.insert(timeout.slot + 1);
                    self.current_consensus_instances
                        .insert(new_prepare_instance.digest(), new_prepare_instance.clone());
                    self.tx_info
                        .send(new_prepare_instance)
                        .await
                        .expect("failed to send info to proposer");
                } else {
                    // Otherwise add the ticket to the queue, and wait later until there
                    // are enough new certificates to propose
                    self.tickets.push_back(ticket);
                }
            }
        }
        Ok(())
    }

    async fn handle_tc(&mut self, tc: &TC) -> DagResult<()> {
        debug!("Processing {:?}", tc);

        let slot = tc.slot;
        let view = tc.view;

        // Make a new header if we are the next leader.
        if self.name == self.leader_elector.get_leader(slot, view + 1) {
            // TODO: Generate ticket
            let mut winning_proposals = HashMap::new();
            let mut winning_timeout = tc.timeouts.get(0).unwrap();

            for timeout in &tc.timeouts {
                match &timeout.high_qc {
                    Some(qc) => match qc {
                        ConsensusMessage::Confirm {
                            slot: _,
                            view: other_view,
                            qc: _,
                            proposals,
                        } => {
                            if other_view > &winning_timeout.view {
                                winning_timeout = &timeout;
                                winning_proposals = proposals.clone();
                            }
                        }
                        _ => {}
                    },
                    None => {}
                };
            }

            let ticket: Ticket = Ticket {
                header: None,
                tc: Some(tc.clone()),
                slot,
                proposals: winning_proposals.clone(),
            };

            if winning_proposals.is_empty() {
                winning_proposals = self.current_proposals.clone();
            }

            let prepare_instance: ConsensusMessage = ConsensusMessage::Prepare {
                slot,
                view: view + 1,
                ticket: ticket.clone(),
                proposals: winning_proposals.clone(),
            };
            self.tx_info
                .send(prepare_instance)
                .await
                .expect("Failed to send consensus instance");

            // A TC could be a ticket for the next slot
            if !self.already_proposed_slots.contains(&(slot + 1))
                && self.enough_coverage(&ticket, &winning_proposals)
            {
                let new_prepare_instance = ConsensusMessage::Prepare {
                    slot: slot + 1,
                    view: 1,
                    ticket,
                    proposals: winning_proposals,
                };
                self.already_proposed_slots.insert(slot + 1);
                self.current_consensus_instances
                    .insert(new_prepare_instance.digest(), new_prepare_instance.clone());
                self.tx_info
                    .send(new_prepare_instance)
                    .await
                    .expect("failed to send info to proposer");
            } else {
                // Otherwise add the ticket to the queue, and wait later until there
                // are enough new certificates to propose
                self.tickets.push_back(ticket);
            }
        }

        Ok(())
    }

    fn sanitize_header(&mut self, header: &Header) -> DagResult<()> {
        ensure!(
            self.gc_round <= header.height,
            DagError::HeaderTooOld(header.id.clone(), header.height)
        );

        // Verify the header's signature.
        header.verify(&self.committee)?;

        // TODO [issue #3]: Prevent bad nodes from sending junk headers with high round numbers.

        Ok(())
    }

    fn sanitize_vote(&mut self, vote: &Vote) -> DagResult<()> {
        //println!("Received vote for origin: {}, header id {}, round {}. Vote sent by replica {}", vote.origin.clone(), vote.id.clone(), vote.round.clone(), vote.author.clone());
        /*ensure!(
            self.current_headers.get(&vote.height) != None,
            DagError::VoteTooOld(vote.digest(), vote.height)
        );*/
        // ensure!(
        //     self.current_header.round <= vote.round,
        //     DagError::VoteTooOld(vote.digest(), vote.round)
        // );

        // Ensure we receive a vote on the expected header.
        /*let current_header = self.current_headers.entry(vote.height).or_insert_with(HashMap::new).get(&vote.author);
        ensure!(
            current_header != None && current_header.unwrap().author == vote.origin,
            DagError::UnexpectedVote(vote.id.clone())
        );*/
        // ensure!(
        //     vote.id == self.current_header.id
        //         && vote.origin == self.current_header.author
        //         && vote.round == self.current_header.round,
        //     DagError::UnexpectedVote(vote.id.clone())
        // );

        //Deprecated code for Invalid vote proofs
        // if false && self.current_header.is_special && vote.special_valid == 0 {
        //     match &vote.tc {
        //         Some(tc) => { //invalidation proof = a TC that formed for the current view (or a future one). Implies one cannot vote in this view anymore.
        //             ensure!(
        //                 tc.view >= self.current_header.view,
        //                 DagError::InvalidVoteInvalidation
        //             );
        //             match tc.verify(&self.committee) {
        //                 Ok(()) => {},
        //                 _ => return Err(DagError::InvalidVoteInvalidation)
        //             }

        //          },
        //         None => {
        //             match &vote.qc {
        //                 Some(qc) => { //invalidation proof = a QC that formed for a future view (i.e. an extension of some TC in current view or future)
        //                     ensure!( //proof is actually showing a conflict.
        //                         qc.view > self.current_header.view,
        //                         DagError::InvalidVoteInvalidation
        //                     );
        //                     match qc.verify(&self.committee) {
        //                         Ok(()) => {},
        //                         _ => return Err(DagError::InvalidVoteInvalidation)
        //                     }
        //                 }
        //                 None => { return Err(DagError::InvalidVoteInvalidation)}
        //             }
        //         },
        //     }
        // }

        // Verify the vote.
        vote.verify(&self.committee).map_err(DagError::from)
    }

    fn sanitize_certificate(&mut self, certificate: &Certificate) -> DagResult<()> {
        ensure!(
            self.gc_round <= certificate.height(),
            DagError::CertificateTooOld(certificate.digest(), certificate.height())
        );

        println!("Past first ensure");

        // Verify the certificate (and the embedded header).
        certificate.verify(&self.committee).map_err(DagError::from)
    }

    // Main loop listening to incoming messages.
    pub async fn run(&mut self) {
        self.tips = Header::genesis_headers(&self.committee);
        self.current_certs = Certificate::genesis_certs(&self.committee);

        loop {
            let result = tokio::select! {
                // We receive here messages from other primaries.
                Some(message) = self.rx_primaries.recv() => {
                    match message {
                        PrimaryMessage::Header(header) => {
                            match self.sanitize_header(&header) {
                                Ok(()) => self.process_header(header).await,
                                error => error
                            }

                        },
                        PrimaryMessage::Vote(vote) => {
                            match self.sanitize_vote(&vote) {
                                Ok(()) => {
                                    self.process_vote(vote).await
                                },
                                error => {
                                    error
                                }
                            }
                        },
                        PrimaryMessage::Certificate(certificate) => {
                            match self.sanitize_certificate(&certificate) {
                                Ok(()) => self.process_certificate(certificate).await, //self.receive_certificate(certificate).await,
                                error => {
                                    error
                                }
                            }
                        },
                        PrimaryMessage::Timeout(timeout) => self.handle_timeout(&timeout).await,
                        PrimaryMessage::TC(tc) => self.handle_tc(&tc).await,
                        _ => panic!("Unexpected core message")
                    }
                },

                // We also receive here our new headers created by the `Proposer`.
                Some(header) = self.rx_proposer.recv() => self.process_own_header(header).await,

                // We receive here loopback headers from the `HeaderWaiter`. Those are headers for which we interrupted
                // execution (we were missing some of their dependencies) and we are now ready to resume processing.
                Some(header) = self.rx_header_waiter.recv() => self.process_header(header).await,

                // Loopback for committed instance that hasn't had all of it ancestors yet
                Some((_, _)) = self.rx_header_waiter_instances.recv() => self.process_loopback().await,
                //Loopback for special headers that were validated by consensus layer.
                //Some((header, consensus_sigs)) = self.rx_validation.recv() => self.create_vote(header, consensus_sigs).await,
                //i.e. core requests validation from consensus (check if ticket valid; wait to receive ticket if we don't have it yet -- should arrive: using all to all or forwarding)

                Some(header_digest) = self.rx_request_header_sync.recv() => self.synchronizer.fetch_header(header_digest).await,

                // We receive here loopback certificates from the `CertificateWaiter`. Those are certificates for which
                // we interrupted execution (we were missing some of their ancestors) and we are now ready to resume
                // processing.
                //Some(certificate) = self.rx_certificate_waiter.recv() => self.process_certificate(certificate).await,

                // We receive an event that timer expired
                Some((slot, view)) = self.timer_futures.next() => self.local_timeout_round(slot, view).await,

            };
            match result {
                Ok(()) => (),
                Err(DagError::StoreError(e)) => {
                    error!("{}", e);
                    panic!("Storage failure: killing node.");
                }
                Err(e @ DagError::HeaderTooOld(..)) => debug!("{}", e),
                Err(e @ DagError::VoteTooOld(..)) => debug!("{}", e),
                Err(e @ DagError::CertificateTooOld(..)) => debug!("{}", e),
                Err(e) => warn!("{}", e),
            }

            // Cleanup internal state.
            let round = self.consensus_round.load(Ordering::Relaxed);
            if round > self.gc_depth {
                let gc_round = round - self.gc_depth;
                self.last_voted.retain(|k, _| k >= &gc_round);
                self.processing.retain(|k, _| k >= &gc_round);

                //self.current_headers.retain(|k, _| k >= &gc_round);
                self.vote_aggregators.retain(|k, _| k >= &gc_round);

                //self.certificates_aggregators.retain(|k, _| k >= &gc_round);
                self.cancel_handlers.retain(|k, _| k >= &gc_round);
                self.gc_round = gc_round;
                debug!("GC round moved to {}", self.gc_round);
            }
        }
    }
}
