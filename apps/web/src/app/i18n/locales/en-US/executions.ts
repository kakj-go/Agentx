const translations = {
  "trace": "View trace",
  "traceDelayed": "Trace delayed",
  "traceDelayedDescription": "The authoritative execution state is available; trace ingestion is still catching up.",
  "traceUnavailable": "Trace unavailable",
  "traceUnavailableDescription": "The authoritative execution state is still available while observability recovers.",
  "runVersion": "Run version",
  "loadingRuntime": "Loading Agent Runtime details…",
  "noRuntimeDetails": "This Execution has no Agent, Runtime Call, or Sandbox records.",
  "runtimeDetails": "Agent Runtime details",
  "agentRuns": "Agent runs",
  "inputTokens": "Input tokens",
  "outputTokens": "Output tokens",
  "cost": "Cost",
  "agents": "Agents",
  "runtimeCalls": "Runtime calls",
  "sandboxes": "Sandboxes",
  "sandbox": "Sandbox",
  "iterations": "Iterations",
  "modelCalls": "Model calls",
  "toolCalls": "Tool calls",
  "tokens": "Tokens",
  "stopReason": "Stop reason",
  "state": "State",
  "iterationLedger": "Iteration ledger",
  "kind": "Kind",
  "resource": "Resource",
  "fingerprint": "Call fingerprint",
  "sideEffect": "Side effect",
  "result": "Result",
  "profileVersion": "Profile version",
  "expires": "Expires",
  "terminationAttempts": "Cleanup attempts",
  "lastError": "Last error",
  "title": "Executions",
  "description": "Explore Workflow executions, traces, and checkpoints.",
  "search": "Search executions or workflows",
  "trigger": "Trigger",
  "executionIdLabel": "Execution ID",
  "startedAt": "Started",
  "duration": "Duration"
  ,"nodePanel": {
    "selectNode": "Select a node to inspect its runtime data", "ariaLabel": "Node runtime data", "outlineAriaLabel": "Execution node outline", "outline": "Execution outline", "runCount": "{{count}} runs", "noNodes": "No node runs", "run": "Run", "input": "Input", "output": "Output", "lineage": "Lineage", "attempts": "Attempts", "logs": "Logs", "noInput": "This node has no input", "noOutput": "This node has no output yet", "noLineage": "No lineage records", "noAttempts": "No attempt records", "noLogs": "No node logs", "attempt": "Attempt {{number}}", "worker": "Worker", "deadline": "Deadline", "started": "Started", "sourceSummary": "Run {{run}} · Output {{output}} · Item {{source}} → Item {{target}}"
  }
  ,"recovery": {
    "ariaLabel": "Recovery and events", "title": "Timeline and recovery", "wait": "Wait", "approval": "Approval", "sideEffectConfirmation": "Side-effect confirmation", "wake": "Wake", "timeout": "Timeout", "openApproval": "Open approval", "irreversibleWaiting": "This irreversible node is waiting for a resume decision.", "handle": "Handle", "checkpoints": "Checkpoints · {{count}}", "noCheckpoints": "No checkpoints", "events": "Events", "noEvents": "No timeline events", "activationsAndDeliveries": "{{activations}} activations · {{deliveries}} deliveries", "download": "Download"
  }
  ,"downloadArtifact": "Download Artifact {{id}}"
  ,"callKinds": { "model": "Model call", "tool": "Tool call", "memory": "Memory call", "skill": "Skill call", "mcp_tool": "MCP tool call" }
  ,"sideEffects": { "none": "None", "reversible": "Reversible", "irreversible": "Irreversible" }
  ,"stopReasons": { "completed": "Completed", "max_iterations": "Maximum iterations reached", "cancelled": "Cancelled", "tool_error": "Tool error", "failed": "Failed" }
  ,"forkDialog": {
    "title": "Fork execution", "description": "Preview the nodes that the fork will reuse, rerun, or require confirmation for", "intro": "The original execution remains read-only. The new execution has independent state and Trace data.", "checkpoint": "Checkpoint", "scope": "Execution scope", "targetNode": "Target node", "inputOverrides": "Input overrides", "selectCheckpoint": "Select a checkpoint", "selectNode": "Select a node", "preview": "Execution preview", "previewSummary": "{{rerun}} rerun · {{reuse}} reuse", "run": "Run", "rerun": "Rerun", "reuseOutput": "Reuse output", "willExecute": "Will execute", "noSideEffect": "No side effect", "decision": "Side-effect decision for {{name}}", "dryRun": "Dry run", "reusePreviousOutput": "Reuse previous output", "confirmExecute": "Confirm execute", "risk": "{{count}} irreversible nodes require an explicit decision. Dry run does not invoke remote side effects, and reusing output does not rerun the node.", "noNodes": "No nodes to preview", "creating": "Creating…", "create": "Create fork", "modes": { "whole": "Whole", "node": "Node", "to_node": "To node", "from_node": "From node" }
  }
  ,"sideEffectDialog": { "title": "Side-effect confirmation", "description": "Choose a recovery policy for the irreversible node", "intro": "This fork will pass through an irreversible node. Confirm execute produces real side effects again; reuse previous output uses the parent execution result; dry run only passes input marked with the decision.", "decision": "Side-effect decision", "submitting": "Submitting…", "confirm": "Confirm decision" }
  ,"loadingExecution": "Loading execution"
  ,"noPermission": "You do not have permission to view this execution"
  ,"loadFailedTitle": "Failed to load execution"
  ,"loadingDescription": "Loading the runtime snapshot, nodes, and recovery state."
  ,"cancelledToast": "Execution cancelled"
  ,"sideEffectSubmitted": "Side-effect decision submitted"
  ,"backToList": "Back to executions"
  ,"executionId": "Execution {{id}}"
  ,"workflowVersion": "Version {{version}}"
  ,"durationLabel": "Duration"
  ,"triggerLabel": "Trigger"
  ,"refresh": "Refresh execution"
  ,"cancelExecution": "Cancel execution"
  ,"forkExecution": "Fork execution"
  ,"parentExecution": "Parent execution {{id}}"
  ,"callerExecution": "Caller execution {{id}}"
  ,"return": "Back"
  ,"cancelTitle": "Cancel execution?"
  ,"cancelDescription": "Cancelling persists the terminal state, releases the lease, and invalidates later resumes. Runtime history is retained."
  ,"executionTypes": {
    "whole": "Full execution",
    "node": "Single-node execution",
    "to_node": "Run to node",
    "from_node": "Continue from node",
    "fork": "Fork execution",
    "sub_workflow": "Sub-workflow execution"
  }
  ,"triggerTypes": {
    "manual": "Manual",
    "api": "API",
    "application": "Application",
    "webhook": "Webhook",
    "schedule": "Schedule",
    "event": "Event",
    "sub_workflow": "Sub-workflow"
  }
} as const

export default translations
