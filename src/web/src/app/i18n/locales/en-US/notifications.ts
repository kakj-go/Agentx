const translations = {
  "readAll": "Mark all read",
  "notificationsDescription": "Review business messages produced by approvals, executions, publications, and evaluations.",
  "noNotificationsDescription": "New business events will be projected into this inbox.",
  "execution": {"title": "A tool failed during execution", "description": "The contract review workflow's file parser call failed."},
  "approvalReassigned": {"title": "An approval was assigned to you", "body": "Open the approval details to claim and process this task."},
  "resourceGrantRequested": {"title": "New resource access request", "body": "Open the request and review the item for your department."},
  "resourceGrantApproved": {"title": "Resource access approved", "body": "The resource can now be selected on the workflow canvas."},
  "resourceGrantRejected": {"title": "Resource access rejected", "body": "Review the decision and submit another request if needed."},
  "resourceGrantStale": {"title": "Resource access request is stale", "body": "The dependencies, workflow, or requester access changed. Submit a new request."},
  "resourceGrantCancelled": {"title": "Resource access request cancelled", "body": "The request was cancelled and no resource grants were created."}
} as const

export default translations
