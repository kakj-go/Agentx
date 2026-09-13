pub(super) fn adjacency(
    node_count: usize,
    connections: &[(&agentx_domain::WorkflowConnection, usize, usize, u32)],
) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); node_count];
    for (_, source, target, _) in connections {
        adjacency[*source].push(*target);
    }
    for targets in &mut adjacency {
        targets.sort_unstable();
        targets.dedup();
    }
    adjacency
}

pub(super) fn strongly_connected_components(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    fn visit(node: usize, graph: &[Vec<usize>], seen: &mut [bool], order: &mut Vec<usize>) {
        if seen[node] {
            return;
        }
        seen[node] = true;
        for target in &graph[node] {
            visit(*target, graph, seen, order);
        }
        order.push(node);
    }
    fn collect(node: usize, graph: &[Vec<usize>], seen: &mut [bool], component: &mut Vec<usize>) {
        if seen[node] {
            return;
        }
        seen[node] = true;
        component.push(node);
        for target in &graph[node] {
            collect(*target, graph, seen, component);
        }
    }
    let mut order = Vec::new();
    let mut seen = vec![false; adjacency.len()];
    for node in 0..adjacency.len() {
        visit(node, adjacency, &mut seen, &mut order);
    }
    let mut reverse = vec![Vec::new(); adjacency.len()];
    for (source, targets) in adjacency.iter().enumerate() {
        for target in targets {
            reverse[*target].push(source);
        }
    }
    let mut components = Vec::new();
    seen.fill(false);
    for node in order.into_iter().rev() {
        if !seen[node] {
            let mut component = Vec::new();
            collect(node, &reverse, &mut seen, &mut component);
            component.sort_unstable();
            components.push(component);
        }
    }
    components.sort_by_key(|component| component[0]);
    components
}
