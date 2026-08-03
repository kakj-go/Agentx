use std::collections::{BTreeMap, BTreeSet, VecDeque};

use agentx_domain::{AttemptId, NodeErrorPolicy, NodeExecutionId};
use agentx_node_protocol::{Item, ItemSource, ReadinessPolicy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::CompiledWorkflow;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionStatus {
    Created,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationStatus {
    Ready,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    Running,
    Succeeded,
    Failed,
    Suspended,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartialExecutionMode {
    Whole,
    Node,
    ToNode,
    FromNode,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAttempt {
    pub id: AttemptId,
    pub attempt_number: u16,
    pub status: AttemptStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeActivation {
    pub id: NodeExecutionId,
    pub node_index: usize,
    pub generation: u32,
    pub slot: u32,
    pub run_index: u32,
    pub status: ActivationStatus,
    pub inputs: BTreeMap<String, Vec<Item>>,
    pub input_delivery_sequences: Vec<u64>,
    pub attempts: Vec<RuntimeAttempt>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "items", rename_all = "snake_case")]
pub enum DeliveryKind {
    Data(Vec<Item>),
    ClosedWithoutData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeDelivery {
    pub id: Uuid,
    pub sequence: u64,
    pub connection_index: usize,
    pub source_node_execution_id: NodeExecutionId,
    pub target_node: usize,
    pub target_generation: u32,
    pub kind: DeliveryKind,
}

#[derive(Debug, Error, PartialEq)]
pub enum MachineError {
    #[error("activation {0} was not found")]
    ActivationNotFound(NodeExecutionId),
    #[error("activation {id} cannot transition from {from:?} to {to:?}")]
    InvalidTransition {
        id: NodeExecutionId,
        from: ActivationStatus,
        to: ActivationStatus,
    },
    #[error("execution is already terminal")]
    ExecutionTerminal,
    #[error("workflow activation budget {0} was exhausted")]
    ActivationBudgetExceeded(u32),
    #[error("resume output port '{0}' is not declared")]
    InvalidResumePort(String),
    #[error("partial execution node '{0}' was not found")]
    PartialNodeNotFound(String),
    #[error("partial execution node '{0}' has no inputs at this checkpoint")]
    PartialInputUnavailable(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExecutionMachine {
    workflow: CompiledWorkflow,
    status: RuntimeExecutionStatus,
    activations: BTreeMap<NodeExecutionId, NodeActivation>,
    activation_keys: BTreeMap<String, NodeExecutionId>,
    ready: VecDeque<NodeExecutionId>,
    deliveries: Vec<EdgeDelivery>,
    next_delivery_sequence: u64,
    run_counts: Vec<u32>,
}

impl ExecutionMachine {
    pub fn new(workflow: CompiledWorkflow, input: Vec<Item>) -> Result<Self, MachineError> {
        let node_count = workflow.nodes.len();
        let mut machine = Self {
            workflow,
            status: RuntimeExecutionStatus::Created,
            activations: BTreeMap::new(),
            activation_keys: BTreeMap::new(),
            ready: VecDeque::new(),
            deliveries: Vec::new(),
            next_delivery_sequence: 1,
            run_counts: vec![0; node_count],
        };
        let starts = machine.workflow.start_nodes.clone();
        for start in starts {
            let mut inputs = BTreeMap::new();
            inputs.insert("main".into(), input.clone());
            machine.create_activation(start, 0, 0, inputs, Vec::new(), ActivationStatus::Ready)?;
        }
        machine.status = RuntimeExecutionStatus::Running;
        Ok(machine)
    }

    pub fn fork_from_checkpoint(
        &self,
        mode: PartialExecutionMode,
        node_id: Option<&str>,
        whole_input: Vec<Item>,
        input_overrides: &Value,
    ) -> Result<Self, MachineError> {
        if mode == PartialExecutionMode::Whole {
            return Self::new(self.workflow.clone(), whole_input);
        }
        let node_id = node_id.ok_or_else(|| MachineError::PartialNodeNotFound(String::new()))?;
        let selected = self
            .workflow
            .nodes
            .iter()
            .position(|node| node.id == node_id)
            .ok_or_else(|| MachineError::PartialNodeNotFound(node_id.into()))?;
        let included = match mode {
            PartialExecutionMode::Node => BTreeSet::from([selected]),
            PartialExecutionMode::ToNode => reachable_nodes(&self.workflow, selected, true),
            PartialExecutionMode::FromNode => reachable_nodes(&self.workflow, selected, false),
            PartialExecutionMode::Whole => unreachable!(),
        };
        let (workflow, indexes) = subgraph(&self.workflow, &included, mode, selected);
        if mode == PartialExecutionMode::ToNode {
            return Self::new(workflow, whole_input);
        }
        let mut inputs = self
            .activations
            .values()
            .filter(|activation| activation.node_index == selected)
            .max_by_key(|activation| activation.run_index)
            .map(|activation| activation.inputs.clone())
            .ok_or_else(|| MachineError::PartialInputUnavailable(node_id.into()))?;
        apply_input_overrides(&mut inputs, input_overrides);
        Self::new_with_starts(workflow, vec![(indexes[&selected], inputs)])
    }

    fn new_with_starts(
        workflow: CompiledWorkflow,
        starts: Vec<(usize, BTreeMap<String, Vec<Item>>)>,
    ) -> Result<Self, MachineError> {
        let node_count = workflow.nodes.len();
        let mut machine = Self {
            workflow,
            status: RuntimeExecutionStatus::Created,
            activations: BTreeMap::new(),
            activation_keys: BTreeMap::new(),
            ready: VecDeque::new(),
            deliveries: Vec::new(),
            next_delivery_sequence: 1,
            run_counts: vec![0; node_count],
        };
        for (node, inputs) in starts {
            machine.create_activation(node, 0, 0, inputs, Vec::new(), ActivationStatus::Ready)?;
        }
        machine.status = RuntimeExecutionStatus::Running;
        Ok(machine)
    }

    #[must_use]
    pub const fn status(&self) -> RuntimeExecutionStatus {
        self.status
    }

    #[must_use]
    pub fn workflow(&self) -> &CompiledWorkflow {
        &self.workflow
    }

    pub fn activations(&self) -> impl Iterator<Item = &NodeActivation> {
        self.activations.values()
    }

    #[must_use]
    pub fn activation(&self, id: NodeExecutionId) -> Option<&NodeActivation> {
        self.activations.get(&id)
    }

    #[must_use]
    pub fn deliveries(&self) -> &[EdgeDelivery] {
        &self.deliveries
    }

    pub fn next_ready(&mut self) -> Option<NodeExecutionId> {
        self.ready.pop_front()
    }

    pub fn defer_for_confirmation(&mut self, id: NodeExecutionId) -> Result<(), MachineError> {
        self.ensure_active()?;
        let activation = self
            .activations
            .get_mut(&id)
            .ok_or(MachineError::ActivationNotFound(id))?;
        if activation.status != ActivationStatus::Ready {
            return Err(MachineError::InvalidTransition {
                id,
                from: activation.status,
                to: ActivationStatus::Waiting,
            });
        }
        activation.status = ActivationStatus::Waiting;
        self.ready.push_front(id);
        self.status = RuntimeExecutionStatus::Waiting;
        Ok(())
    }

    pub fn resume_confirmation(&mut self) {
        if self.status == RuntimeExecutionStatus::Waiting
            && let Some(id) = self.ready.front().copied()
            && self.activations[&id].status == ActivationStatus::Waiting
        {
            self.activations
                .get_mut(&id)
                .expect("queued confirmation activation exists")
                .status = ActivationStatus::Ready;
            self.status = RuntimeExecutionStatus::Running;
        }
    }

    pub fn start_attempt(&mut self, id: NodeExecutionId) -> Result<AttemptId, MachineError> {
        self.ensure_active()?;
        let activation = self
            .activations
            .get_mut(&id)
            .ok_or(MachineError::ActivationNotFound(id))?;
        if activation.status != ActivationStatus::Ready {
            return Err(MachineError::InvalidTransition {
                id,
                from: activation.status,
                to: ActivationStatus::Running,
            });
        }
        let attempt_id = AttemptId::new();
        activation.attempts.push(RuntimeAttempt {
            id: attempt_id,
            attempt_number: activation.attempts.len() as u16 + 1,
            status: AttemptStatus::Running,
            error_code: None,
            error_message: None,
        });
        activation.status = ActivationStatus::Running;
        Ok(attempt_id)
    }

    pub fn complete(
        &mut self,
        id: NodeExecutionId,
        mut outputs: BTreeMap<String, Vec<Item>>,
    ) -> Result<(), MachineError> {
        self.transition_running(id, ActivationStatus::Succeeded, AttemptStatus::Succeeded)?;
        let node_index = self.activations[&id].node_index;
        if self.workflow.nodes[node_index].settings.always_output_data
            && outputs.values().all(Vec::is_empty)
        {
            outputs.insert(
                "main".into(),
                vec![Item {
                    json: json!({}),
                    ..Item::default()
                }],
            );
        }
        self.emit_outputs(id, &outputs)?;
        self.update_terminal_status();
        Ok(())
    }

    pub fn fail(
        &mut self,
        id: NodeExecutionId,
        code: &str,
        message: &str,
        retryable: bool,
    ) -> Result<(), MachineError> {
        self.ensure_active()?;
        let (node_index, should_retry, policy) = {
            let activation = self
                .activations
                .get_mut(&id)
                .ok_or(MachineError::ActivationNotFound(id))?;
            if activation.status != ActivationStatus::Running {
                return Err(MachineError::InvalidTransition {
                    id,
                    from: activation.status,
                    to: ActivationStatus::Failed,
                });
            }
            let attempt = activation
                .attempts
                .last_mut()
                .expect("running activation has attempt");
            attempt.status = AttemptStatus::Failed;
            attempt.error_code = Some(code.into());
            attempt.error_message = Some(message.into());
            let node = &self.workflow.nodes[activation.node_index];
            let should_retry = retryable
                && node.settings.retry_on_fail
                && activation.attempts.len() < usize::from(node.settings.max_tries);
            (activation.node_index, should_retry, node.settings.on_error)
        };
        if should_retry {
            self.activations
                .get_mut(&id)
                .expect("activation exists")
                .status = ActivationStatus::Ready;
            self.ready.push_back(id);
            return Ok(());
        }
        self.activations
            .get_mut(&id)
            .expect("activation exists")
            .status = ActivationStatus::Failed;
        match policy {
            NodeErrorPolicy::StopWorkflow => self.status = RuntimeExecutionStatus::Failed,
            NodeErrorPolicy::ContinueRegularOutput | NodeErrorPolicy::ContinueErrorOutput => {
                let port = if policy == NodeErrorPolicy::ContinueErrorOutput {
                    "error"
                } else {
                    "main"
                };
                let mut outputs = BTreeMap::new();
                outputs.insert(port.into(), vec![Item {
                    json: json!({"error":{"code":code,"message":message},"sourceNode":self.workflow.nodes[node_index].id}),
                    ..Item::default()
                }]);
                self.emit_outputs(id, &outputs)?;
                self.update_terminal_status();
            }
        }
        Ok(())
    }

    pub fn suspend(&mut self, id: NodeExecutionId) -> Result<(), MachineError> {
        self.transition_running(id, ActivationStatus::Waiting, AttemptStatus::Suspended)?;
        self.status = RuntimeExecutionStatus::Waiting;
        Ok(())
    }

    pub fn resume(
        &mut self,
        id: NodeExecutionId,
        port: &str,
        items: Vec<Item>,
    ) -> Result<(), MachineError> {
        self.ensure_active()?;
        let node_index = self
            .activations
            .get(&id)
            .ok_or(MachineError::ActivationNotFound(id))?
            .node_index;
        if !self.workflow.nodes[node_index]
            .output_ports
            .iter()
            .any(|value| value == port)
        {
            return Err(MachineError::InvalidResumePort(port.into()));
        }
        let activation = self.activations.get_mut(&id).expect("activation exists");
        if activation.status != ActivationStatus::Waiting {
            return Err(MachineError::InvalidTransition {
                id,
                from: activation.status,
                to: ActivationStatus::Succeeded,
            });
        }
        activation.status = ActivationStatus::Succeeded;
        let attempt = activation
            .attempts
            .last_mut()
            .expect("waiting activation has attempt");
        attempt.status = AttemptStatus::Succeeded;
        self.status = RuntimeExecutionStatus::Running;
        self.emit_outputs(id, &BTreeMap::from([(port.into(), items)]))?;
        self.update_terminal_status();
        Ok(())
    }

    pub fn cancel(&mut self) {
        if is_terminal(self.status) {
            return;
        }
        self.status = RuntimeExecutionStatus::Cancelled;
        self.ready.clear();
        for activation in self.activations.values_mut() {
            if matches!(
                activation.status,
                ActivationStatus::Ready | ActivationStatus::Running | ActivationStatus::Waiting
            ) {
                activation.status = ActivationStatus::Cancelled;
                if let Some(attempt) = activation.attempts.last_mut()
                    && matches!(
                        attempt.status,
                        AttemptStatus::Running | AttemptStatus::Suspended
                    )
                {
                    attempt.status = AttemptStatus::Cancelled;
                }
            }
        }
    }

    fn transition_running(
        &mut self,
        id: NodeExecutionId,
        target: ActivationStatus,
        attempt_target: AttemptStatus,
    ) -> Result<(), MachineError> {
        self.ensure_active()?;
        let activation = self
            .activations
            .get_mut(&id)
            .ok_or(MachineError::ActivationNotFound(id))?;
        if activation.status != ActivationStatus::Running {
            return Err(MachineError::InvalidTransition {
                id,
                from: activation.status,
                to: target,
            });
        }
        activation.status = target;
        activation
            .attempts
            .last_mut()
            .expect("running activation has attempt")
            .status = attempt_target;
        Ok(())
    }

    fn emit_outputs(
        &mut self,
        source_id: NodeExecutionId,
        outputs: &BTreeMap<String, Vec<Item>>,
    ) -> Result<(), MachineError> {
        let source = self.activations[&source_id].clone();
        let outgoing = self.workflow.nodes[source.node_index]
            .outgoing_connections
            .clone();
        let mut targets = Vec::new();
        for connection_index in outgoing {
            let connection = self.workflow.connections[connection_index].clone();
            let generation = source.generation + u32::from(connection.back_edge);
            let mut items = outputs
                .get(&connection.source_port)
                .cloned()
                .unwrap_or_default();
            for (item_index, item) in items.iter_mut().enumerate() {
                item.lineage.push(ItemSource {
                    node_execution_id: source_id,
                    node_id: self.workflow.nodes[source.node_index].id.clone(),
                    run_index: source.run_index,
                    output_index: connection.branch_order,
                    item_index: item_index as u32,
                });
            }
            let kind = if items.is_empty() {
                DeliveryKind::ClosedWithoutData
            } else {
                DeliveryKind::Data(items)
            };
            self.deliveries.push(EdgeDelivery {
                id: Uuid::now_v7(),
                sequence: self.next_delivery_sequence,
                connection_index,
                source_node_execution_id: source_id,
                target_node: connection.target_node,
                target_generation: generation,
                kind,
            });
            self.next_delivery_sequence += 1;
            targets.push((connection.target_node, generation, connection_index));
        }
        for (target, generation, edge) in targets {
            self.evaluate_target(target, generation, edge)?;
        }
        Ok(())
    }

    fn evaluate_target(
        &mut self,
        node_index: usize,
        generation: u32,
        triggering_edge: usize,
    ) -> Result<(), MachineError> {
        let node = self.workflow.nodes[node_index].clone();
        let deliveries = node
            .incoming_connections
            .iter()
            .filter_map(|edge| {
                self.deliveries.iter().rev().find(|delivery| {
                    delivery.connection_index == *edge && delivery.target_generation == generation
                })
            })
            .collect::<Vec<_>>();
        match node.readiness {
            ReadinessPolicy::Any => {
                let Some(delivery) = self.deliveries.iter().rev().find(|delivery| {
                    delivery.connection_index == triggering_edge
                        && delivery.target_generation == generation
                }) else {
                    return Ok(());
                };
                if matches!(delivery.kind, DeliveryKind::Data(_)) {
                    let slot = triggering_edge as u32 + 1;
                    if !self
                        .activation_keys
                        .contains_key(&activation_key(node_index, generation, slot))
                    {
                        let (inputs, sequences) =
                            inputs_from_deliveries(&self.workflow, [delivery]);
                        self.create_activation(
                            node_index,
                            generation,
                            slot,
                            inputs,
                            sequences,
                            ActivationStatus::Ready,
                        )?;
                    }
                } else if deliveries.len() == node.incoming_connections.len()
                    && deliveries
                        .iter()
                        .all(|delivery| matches!(delivery.kind, DeliveryKind::ClosedWithoutData))
                {
                    self.skip_activation(node_index, generation)?;
                }
            }
            ReadinessPolicy::All | ReadinessPolicy::Required => {
                if deliveries.len() != node.incoming_connections.len() {
                    return Ok(());
                }
                let has_data = deliveries
                    .iter()
                    .any(|delivery| matches!(delivery.kind, DeliveryKind::Data(_)));
                let required_ready = node.required_input_ports.iter().all(|required| {
                    deliveries.iter().any(|delivery| {
                        let connection = &self.workflow.connections[delivery.connection_index];
                        port_family(&connection.target_port) == required
                            && matches!(delivery.kind, DeliveryKind::Data(_))
                    })
                });
                if has_data && (node.readiness == ReadinessPolicy::All || required_ready) {
                    if !self
                        .activation_keys
                        .contains_key(&activation_key(node_index, generation, 0))
                    {
                        let (inputs, sequences) =
                            inputs_from_deliveries(&self.workflow, deliveries.iter().copied());
                        self.create_activation(
                            node_index,
                            generation,
                            0,
                            inputs,
                            sequences,
                            ActivationStatus::Ready,
                        )?;
                    }
                } else {
                    self.skip_activation(node_index, generation)?;
                }
            }
        }
        Ok(())
    }

    fn skip_activation(&mut self, node_index: usize, generation: u32) -> Result<(), MachineError> {
        if self
            .activation_keys
            .contains_key(&activation_key(node_index, generation, 0))
        {
            return Ok(());
        }
        let id = self.create_activation(
            node_index,
            generation,
            0,
            BTreeMap::new(),
            Vec::new(),
            ActivationStatus::Skipped,
        )?;
        self.emit_outputs(id, &BTreeMap::new())
    }

    fn create_activation(
        &mut self,
        node_index: usize,
        generation: u32,
        slot: u32,
        inputs: BTreeMap<String, Vec<Item>>,
        sequences: Vec<u64>,
        status: ActivationStatus,
    ) -> Result<NodeExecutionId, MachineError> {
        if self.activations.len() >= self.workflow.activation_budget as usize {
            self.status = RuntimeExecutionStatus::Failed;
            return Err(MachineError::ActivationBudgetExceeded(
                self.workflow.activation_budget,
            ));
        }
        let id = NodeExecutionId::new();
        let run_index = self.run_counts[node_index];
        self.run_counts[node_index] += 1;
        self.activations.insert(
            id,
            NodeActivation {
                id,
                node_index,
                generation,
                slot,
                run_index,
                status,
                inputs,
                input_delivery_sequences: sequences,
                attempts: Vec::new(),
            },
        );
        self.activation_keys
            .insert(activation_key(node_index, generation, slot), id);
        if status == ActivationStatus::Ready {
            self.ready.push_back(id);
        }
        Ok(id)
    }

    fn ensure_active(&self) -> Result<(), MachineError> {
        if is_terminal(self.status) {
            Err(MachineError::ExecutionTerminal)
        } else {
            Ok(())
        }
    }

    fn update_terminal_status(&mut self) {
        if is_terminal(self.status) || self.status == RuntimeExecutionStatus::Waiting {
            return;
        }
        let open = self.activations.values().any(|activation| {
            matches!(
                activation.status,
                ActivationStatus::Ready | ActivationStatus::Running | ActivationStatus::Waiting
            )
        });
        if !open && self.ready.is_empty() {
            self.status = if self
                .activations
                .values()
                .any(|activation| activation.status == ActivationStatus::Failed)
            {
                RuntimeExecutionStatus::Failed
            } else {
                RuntimeExecutionStatus::Succeeded
            };
        }
    }
}

fn reachable_nodes(workflow: &CompiledWorkflow, selected: usize, reverse: bool) -> BTreeSet<usize> {
    let mut included = BTreeSet::from([selected]);
    let mut frontier = VecDeque::from([selected]);
    while let Some(node) = frontier.pop_front() {
        let connections = if reverse {
            &workflow.nodes[node].incoming_connections
        } else {
            &workflow.nodes[node].outgoing_connections
        };
        for connection in connections {
            let connection = &workflow.connections[*connection];
            let next = if reverse {
                connection.source_node
            } else {
                connection.target_node
            };
            if included.insert(next) {
                frontier.push_back(next);
            }
        }
    }
    included
}

fn subgraph(
    source: &CompiledWorkflow,
    included: &BTreeSet<usize>,
    mode: PartialExecutionMode,
    selected: usize,
) -> (CompiledWorkflow, BTreeMap<usize, usize>) {
    let selected_source = selected;
    let indexes = included
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect::<BTreeMap<_, _>>();
    let mut nodes = included
        .iter()
        .map(|old| {
            let mut node = source.nodes[*old].clone();
            node.index = indexes[old];
            node.incoming_connections.clear();
            node.outgoing_connections.clear();
            node
        })
        .collect::<Vec<_>>();
    let mut connections = Vec::new();
    for connection in &source.connections {
        if let (Some(&from), Some(&to)) = (
            indexes.get(&connection.source_node),
            indexes.get(&connection.target_node),
        ) {
            let mut connection = connection.clone();
            connection.index = connections.len();
            connection.source_node = from;
            connection.target_node = to;
            nodes[from].outgoing_connections.push(connection.index);
            nodes[to].incoming_connections.push(connection.index);
            connections.push(connection);
        }
    }
    let selected = indexes[&selected_source];
    let start_nodes = if matches!(
        mode,
        PartialExecutionMode::Node | PartialExecutionMode::FromNode
    ) {
        vec![selected]
    } else {
        source
            .start_nodes
            .iter()
            .filter_map(|old| indexes.get(old).copied())
            .collect()
    };
    let components: Vec<Vec<usize>> = source
        .strongly_connected_components
        .iter()
        .filter_map(|component| {
            let mapped = component
                .iter()
                .filter_map(|old| indexes.get(old).copied())
                .collect::<Vec<_>>();
            (!mapped.is_empty()).then_some(mapped)
        })
        .collect();
    for (component_index, component) in components.iter().enumerate() {
        for node in component {
            nodes[*node].component_index = component_index;
        }
    }
    let mut workflow = source.clone();
    workflow.canonical_hash = format!(
        "{}:partial:{mode:?}:{}",
        source.canonical_hash, source.nodes[selected_source].id
    );
    workflow.nodes = nodes;
    workflow.connections = connections;
    workflow.start_nodes = start_nodes;
    workflow.strongly_connected_components = components;
    (workflow, indexes)
}

fn apply_input_overrides(inputs: &mut BTreeMap<String, Vec<Item>>, overrides: &Value) {
    if overrides.is_null() {
        return;
    }
    if inputs.values().all(Vec::is_empty) {
        inputs.insert(
            "main".into(),
            vec![Item {
                json: overrides.clone(),
                ..Item::default()
            }],
        );
        return;
    }
    for item in inputs.values_mut().flatten() {
        if let (Some(target), Some(values)) = (item.json.as_object_mut(), overrides.as_object()) {
            target.extend(values.clone());
        } else {
            item.json = overrides.clone();
        }
    }
}

fn inputs_from_deliveries<'a>(
    workflow: &CompiledWorkflow,
    deliveries: impl IntoIterator<Item = &'a EdgeDelivery>,
) -> (BTreeMap<String, Vec<Item>>, Vec<u64>) {
    let mut inputs = BTreeMap::<String, Vec<Item>>::new();
    let mut sequences = BTreeSet::new();
    for delivery in deliveries {
        sequences.insert(delivery.sequence);
        if let DeliveryKind::Data(items) = &delivery.kind {
            let port = workflow.connections[delivery.connection_index]
                .target_port
                .clone();
            inputs.entry(port).or_default().extend(items.clone());
        }
    }
    (inputs, sequences.into_iter().collect())
}

fn port_family(port: &str) -> &str {
    port.split_once(':').map_or(port, |(family, _)| family)
}

fn activation_key(node_index: usize, generation: u32, slot: u32) -> String {
    format!("{node_index}:{generation}:{slot}")
}

const fn is_terminal(status: RuntimeExecutionStatus) -> bool {
    matches!(
        status,
        RuntimeExecutionStatus::Succeeded
            | RuntimeExecutionStatus::Failed
            | RuntimeExecutionStatus::Cancelled
            | RuntimeExecutionStatus::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompileContext, NodeRegistry, WorkflowCompiler};
    use agentx_domain::WorkflowDefinition;

    fn compile(value: serde_json::Value) -> CompiledWorkflow {
        let registry = NodeRegistry::m4_defaults();
        WorkflowCompiler::new(&registry)
            .compile(
                &serde_json::from_value::<WorkflowDefinition>(value).unwrap(),
                &CompileContext::default(),
            )
            .unwrap()
    }

    fn item(value: i64) -> Item {
        Item {
            json: json!({"value":value}),
            ..Item::default()
        }
    }

    #[test]
    fn closes_unselected_branch_without_blocking_merge() {
        let workflow = compile(json!({
            "schemaVersion":"2.0",
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","position":{"x":0,"y":0}},
                {"id":"if","type":"if","typeVersion":1,"name":"IF","position":{"x":1,"y":0}},
                {"id":"merge","type":"merge","typeVersion":1,"name":"Merge","position":{"x":2,"y":0}}
            ],
            "connections":[
                {"id":"a","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"if","targetHandle":"main"},
                {"id":"b","sourceNodeId":"if","sourceHandle":"true","targetNodeId":"merge","targetHandle":"main:0"},
                {"id":"c","sourceNodeId":"if","sourceHandle":"false","targetNodeId":"merge","targetHandle":"main:1"}
            ]
        }));
        let mut machine = ExecutionMachine::new(workflow, vec![item(1)]).unwrap();
        let trigger = machine.next_ready().unwrap();
        machine.start_attempt(trigger).unwrap();
        machine
            .complete(trigger, BTreeMap::from([("main".into(), vec![item(1)])]))
            .unwrap();
        let condition = machine.next_ready().unwrap();
        machine.start_attempt(condition).unwrap();
        machine
            .complete(condition, BTreeMap::from([("true".into(), vec![item(1)])]))
            .unwrap();
        let merge = machine.next_ready().expect("merge becomes ready");
        assert_eq!(machine.activations[&merge].inputs["main:0"].len(), 1);
    }

    #[test]
    fn retry_adds_attempt_to_same_activation_and_late_transitions_fail() {
        let workflow = compile(json!({
            "schemaVersion":"2.0",
            "nodes":[{"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","position":{"x":0,"y":0},"settings":{"retryOnFail":true,"maxTries":2}}],
            "connections":[]
        }));
        let mut machine = ExecutionMachine::new(workflow, vec![]).unwrap();
        let activation = machine.next_ready().unwrap();
        machine.start_attempt(activation).unwrap();
        machine.fail(activation, "TEMP", "temporary", true).unwrap();
        assert_eq!(machine.next_ready(), Some(activation));
        machine.start_attempt(activation).unwrap();
        machine.complete(activation, BTreeMap::new()).unwrap();
        assert_eq!(machine.activations[&activation].attempts.len(), 2);
        assert_eq!(machine.status(), RuntimeExecutionStatus::Succeeded);
        assert_eq!(
            machine.start_attempt(activation),
            Err(MachineError::ExecutionTerminal)
        );
    }

    #[test]
    fn wait_releases_execution_and_resumes_once() {
        let workflow = compile(json!({
            "schemaVersion":"2.0",
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","position":{"x":0,"y":0}},
                {"id":"wait","type":"wait","typeVersion":1,"name":"Wait","position":{"x":1,"y":0}}
            ],
            "connections":[{"id":"a","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"wait","targetHandle":"main"}]
        }));
        let mut machine = ExecutionMachine::new(workflow, vec![item(1)]).unwrap();
        let trigger = machine.next_ready().unwrap();
        machine.start_attempt(trigger).unwrap();
        machine
            .complete(trigger, BTreeMap::from([("main".into(), vec![item(1)])]))
            .unwrap();
        let wait = machine.next_ready().unwrap();
        machine.start_attempt(wait).unwrap();
        machine.suspend(wait).unwrap();
        assert_eq!(machine.status(), RuntimeExecutionStatus::Waiting);
        machine.resume(wait, "resumed", vec![item(2)]).unwrap();
        assert_eq!(machine.status(), RuntimeExecutionStatus::Succeeded);
        assert_eq!(
            machine.resume(wait, "resumed", vec![]),
            Err(MachineError::ExecutionTerminal)
        );
    }

    #[test]
    fn partial_forks_select_the_expected_subgraph_and_inputs() {
        let workflow = compile(json!({
            "schemaVersion":"2.0",
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","position":{"x":0,"y":0}},
                {"id":"first","type":"set","typeVersion":1,"name":"First","position":{"x":1,"y":0}},
                {"id":"last","type":"set","typeVersion":1,"name":"Last","position":{"x":2,"y":0}}
            ],
            "connections":[
                {"id":"a","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"first","targetHandle":"main"},
                {"id":"b","sourceNodeId":"first","sourceHandle":"main","targetNodeId":"last","targetHandle":"main"}
            ]
        }));
        let mut source = ExecutionMachine::new(workflow, vec![item(1)]).unwrap();
        let trigger = source.next_ready().unwrap();
        source.start_attempt(trigger).unwrap();
        source
            .complete(trigger, BTreeMap::from([("main".into(), vec![item(1)])]))
            .unwrap();

        let from = source
            .fork_from_checkpoint(
                PartialExecutionMode::FromNode,
                Some("first"),
                vec![],
                &json!({"override":true}),
            )
            .unwrap();
        assert_eq!(
            from.workflow
                .nodes
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "last"]
        );
        let first = from.activations().next().unwrap();
        assert_eq!(first.inputs["main"][0].json["override"], true);

        let to = source
            .fork_from_checkpoint(
                PartialExecutionMode::ToNode,
                Some("first"),
                vec![item(9)],
                &Value::Null,
            )
            .unwrap();
        assert_eq!(
            to.workflow
                .nodes
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            vec!["trigger", "first"]
        );

        let node = source
            .fork_from_checkpoint(
                PartialExecutionMode::Node,
                Some("first"),
                vec![],
                &Value::Null,
            )
            .unwrap();
        assert_eq!(node.workflow.nodes.len(), 1);
        assert!(node.workflow.connections.is_empty());
    }

    #[test]
    fn checkpoint_state_round_trips_through_json() {
        let workflow = compile(json!({
            "schemaVersion":"2.0",
            "nodes":[{"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","position":{"x":0,"y":0}}],
            "connections":[]
        }));
        let machine = ExecutionMachine::new(workflow, vec![item(1)]).unwrap();
        let value = serde_json::to_value(&machine).unwrap();
        let restored: ExecutionMachine = serde_json::from_value(value).unwrap();
        assert_eq!(restored.status(), RuntimeExecutionStatus::Running);
        assert_eq!(restored.activations().count(), 1);
    }

    #[test]
    fn confirmation_wait_is_visible_and_resumes_the_same_activation() {
        let workflow = compile(json!({
            "schemaVersion":"2.0",
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","position":{"x":0,"y":0}},
                {"id":"remote","type":"remote_action","typeVersion":1,"name":"Remote","position":{"x":1,"y":0},"parameters":{"endpoint":"http://node"}}
            ],
            "connections":[{"id":"start","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"remote","targetHandle":"main"}]
        }));
        let mut machine = ExecutionMachine::new(workflow, vec![item(1)]).unwrap();
        let trigger = machine.next_ready().unwrap();
        machine.start_attempt(trigger).unwrap();
        machine
            .complete(trigger, BTreeMap::from([("main".into(), vec![item(1)])]))
            .unwrap();
        let activation = machine.next_ready().unwrap();
        machine.defer_for_confirmation(activation).unwrap();
        assert_eq!(machine.status(), RuntimeExecutionStatus::Waiting);
        assert_eq!(
            machine.activation(activation).unwrap().status,
            ActivationStatus::Waiting
        );
        machine.resume_confirmation();
        assert_eq!(machine.status(), RuntimeExecutionStatus::Running);
        assert_eq!(machine.next_ready(), Some(activation));
        assert_eq!(
            machine.activation(activation).unwrap().status,
            ActivationStatus::Ready
        );
    }

    #[test]
    fn ordinary_cycle_stops_at_the_activation_budget() {
        let workflow = compile(json!({
            "schemaVersion":"2.0",
            "settings":{"activationBudget":7},
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","position":{"x":0,"y":0}},
                {"id":"step","type":"set","typeVersion":1,"name":"Step","position":{"x":1,"y":0}},
                {"id":"branch","type":"if","typeVersion":1,"name":"Branch","position":{"x":2,"y":0},"parameters":{"condition":true}}
            ],
            "connections":[
                {"id":"start","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"step","targetHandle":"main"},
                {"id":"forward","sourceNodeId":"step","sourceHandle":"main","targetNodeId":"branch","targetHandle":"main"},
                {"id":"back","sourceNodeId":"branch","sourceHandle":"true","targetNodeId":"step","targetHandle":"main"}
            ]
        }));
        let mut machine = ExecutionMachine::new(workflow, vec![item(1)]).unwrap();
        let error = loop {
            let activation = machine.next_ready().unwrap();
            let node = machine.workflow.nodes[machine.activation(activation).unwrap().node_index]
                .node_type
                .clone();
            machine.start_attempt(activation).unwrap();
            let port = if node == "if" { "true" } else { "main" };
            if let Err(error) =
                machine.complete(activation, BTreeMap::from([(port.into(), vec![item(1)])]))
            {
                break error;
            }
        };
        assert_eq!(error, MachineError::ActivationBudgetExceeded(7));
        assert_eq!(machine.status(), RuntimeExecutionStatus::Failed);
        assert_eq!(machine.activations().count(), 7);
    }
}
